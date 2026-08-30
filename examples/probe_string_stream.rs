//! Probe: locate the R2007+ per-object string stream from bytes alone.
//!
//! # What fact this proves
//!
//! ODA Open Design Specification v5.4.1 §19.1 says that in R2007 and
//! later every `TV` (variable text) field of an object is moved out of
//! the object's data stream into a separate *string stream* that sits
//! between the data stream and the handle stream. The spec describes
//! the locator as a trailer at the very end of the data area: the last
//! bit is a "strings present" flag, the 16 bits before it hold the
//! string-stream size in bits, and a set high bit on that size means a
//! second 16-bit word extends it.
//!
//! This probe brute-forces every bit position that could be that
//! trailer, applies the rule, and reports the candidates whose implied
//! string stream decodes as a run of plausible `TU` strings that ends
//! exactly at the trailer. A single surviving candidate per object is
//! the evidence that the rule above is the real layout.
//!
//! # How to verify
//!
//! ```bash
//! cargo run --release --example probe_string_stream -- path/to/file.dwg 0x35 4
//! ```
//!
//! The reported `strings=[...]` for a STYLE (0x35) object should be the
//! style name plus its font file names, exactly as a DWG viewer shows
//! them.

use dwg::Version;
use dwg::bitcursor::BitCursor;
use dwg::{DwgFile, Error};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_string_stream <file.dwg> [type-code] [limit]");
        return ExitCode::FAILURE;
    };
    let type_code = args
        .next()
        .map(|s| parse_code(&s))
        .unwrap_or(Some(0x35))
        .unwrap_or(0x35);
    let limit = args
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(usize::MAX);

    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("open failed ({path}): {e}");
            return ExitCode::FAILURE;
        }
    };
    let version = file.version();
    let Some(Ok(objects)) = file.all_objects() else {
        eprintln!("no handle-driven object walk for {version}");
        return ExitCode::FAILURE;
    };

    println!("=== {path} ({version}) type_code=0x{type_code:02X} ===");
    let mut shown = 0usize;
    for (i, raw) in objects.iter().enumerate() {
        if raw.type_code != type_code || shown >= limit {
            continue;
        }
        shown += 1;
        let Ok((header_end, data_end, mc_bits)) = replay(&raw.raw, version) else {
            println!("#{i}: header replay failed");
            continue;
        };
        let bits = raw.raw.len() * 8;
        println!();
        let predicted = data_end.map(|d| d + mc_bits);
        println!(
            "#{i} handle=0x{:X} size={} bits={bits} header_end={header_end} \
             data_end={data_end:?} mc_bits={mc_bits} predicted_end={predicted:?}",
            raw.handle.value, raw.size_bytes
        );
        for cand in candidates(&raw.raw, header_end, version) {
            let delta = predicted.map(|p| cand.end as isize - p as isize);
            println!(
                "  cand end={} delta_vs_predicted={delta:?} size={} start={} strings={:?}",
                cand.end, cand.size, cand.start, cand.strings
            );
            let mut bits = String::new();
            for b in header_end..cand.start {
                bits.push(if read_bit_at(&raw.raw, b) == Some(true) {
                    '1'
                } else {
                    '0'
                });
                if (b - header_end) % 8 == 7 {
                    bits.push(' ');
                }
            }
            println!("  data bits [{header_end}..{}]: {bits}", cand.start);
        }
    }
    ExitCode::SUCCESS
}

struct Candidate {
    end: usize,
    size: usize,
    start: usize,
    strings: Vec<String>,
}

fn candidates(payload: &[u8], header_end: usize, version: Version) -> Vec<Candidate> {
    let bits = payload.len() * 8;
    let mut out = Vec::new();
    for end in (header_end + 18)..=bits {
        if read_bit_at(payload, end - 1) != Some(true) {
            continue;
        }
        let Some(lo) = read_rs_at(payload, end - 17) else {
            continue;
        };
        let (size, trailer_start) = if lo & 0x8000 != 0 {
            let Some(hi) = end.checked_sub(33).and_then(|b| read_rs_at(payload, b)) else {
                continue;
            };
            (
                ((lo & 0x7FFF) as usize) | ((hi as usize) << 15),
                end - 33 - 16,
            )
        } else {
            (lo as usize, end - 17)
        };
        let Some(start) = trailer_start.checked_sub(size) else {
            continue;
        };
        if start < header_end || size == 0 {
            continue;
        }
        let Some(strings) = read_string_run(payload, start, trailer_start, version) else {
            continue;
        };
        out.push(Candidate {
            end,
            size,
            start,
            strings,
        });
    }
    out
}

/// Read `TU` strings from `start` and require the run to land exactly on `stop`.
fn read_string_run(
    payload: &[u8],
    start: usize,
    stop: usize,
    version: Version,
) -> Option<Vec<String>> {
    let mut c = BitCursor::new(payload);
    skip_to(&mut c, start).ok()?;
    let mut out = Vec::new();
    for _ in 0..32 {
        if c.position_bits() == stop {
            return Some(out);
        }
        if c.position_bits() > stop {
            return None;
        }
        let s = read_tv(&mut c, version).ok()?;
        if !s.chars().all(|ch| (' '..='~').contains(&ch)) {
            return None;
        }
        out.push(s);
    }
    None
}

fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String, Error> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if version.is_r2007_plus() {
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            let lo = c.read_rc()? as u16;
            let hi = c.read_rc()? as u16;
            units.push((hi << 8) | lo);
        }
        String::from_utf16(&units).map_err(|_| Error::SectionMap("not utf-16".into()))
    } else {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(c.read_rc()?);
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn replay(payload: &[u8], version: Version) -> Result<(usize, Option<usize>, usize), Error> {
    let mut c = BitCursor::new(payload);
    let mut mc_bits = 0usize;
    let handle_bits = if version.is_r2010_plus() {
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            let b = c.read_rc()? as u64;
            value |= (b & 0x7F) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 64 {
                break;
            }
        }
        mc_bits = c.position_bits();
        Some(value as usize)
    } else {
        None
    };
    let _ = read_object_type(&mut c, version)?;
    if matches!(version, Version::R2000) {
        let _ = c.read_rl()?;
    }
    let _ = c.read_handle()?;
    let data_end = handle_bits.and_then(|h| (payload.len() * 8).checked_sub(h));
    Ok((c.position_bits(), data_end, mc_bits))
}

fn read_object_type(c: &mut BitCursor<'_>, version: Version) -> Result<u16, Error> {
    if version.is_r2010_plus() {
        let tag = c.read_bb()?;
        match tag {
            0 => Ok(c.read_rc()? as u16),
            1 => Ok((c.read_rc()? as u16) + 0x1F0),
            _ => {
                let lsb = c.read_rc()? as u16;
                let msb = c.read_rc()? as u16;
                Ok((msb << 8) | lsb)
            }
        }
    } else {
        Ok(c.read_bs_u()?)
    }
}

fn skip_to(c: &mut BitCursor<'_>, bit: usize) -> Result<(), Error> {
    while c.position_bits() < bit {
        let _ = c.read_b()?;
    }
    Ok(())
}

fn read_bit_at(bytes: &[u8], bit: usize) -> Option<bool> {
    let byte = *bytes.get(bit / 8)?;
    Some((byte >> (7 - (bit % 8))) & 1 != 0)
}

fn read_rs_at(bytes: &[u8], bit: usize) -> Option<u16> {
    let mut c = BitCursor::new(bytes);
    skip_to(&mut c, bit).ok()?;
    c.read_rs().ok().map(|v| v as u16)
}

fn parse_code(s: &str) -> Option<u16> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}
