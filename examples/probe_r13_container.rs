//! Does an R13-R15 drawing's container match what ODA spec §3, §9, §10.1,
//! §14.2, §23.1 and §26 print? One verdict per structure, per file.
//!
//! # What this proves
//!
//! For each drawing named on the command line it re-reads the bytes with
//! its own framing code (not the reader's) and prints:
//!
//! ```text
//! file  AC10xx  pad  locators  crc  sentinel  image  vars  classes  map  second-header
//! ```
//!
//! - `pad`: bytes 0x06-0x0C (§3.2.1: six zeros, or five and `ACADMAINTVER`,
//!   then a 1).
//! - `crc`: the §3.2.6 locator CRC, seeded 0 and XORed with the constant
//!   for the locator count (3: 0xA598, 4: 0x8101, 5: 0x3CC4, 6: 0x8461).
//! - `image`: the §14.2 start sentinel at the §3.2.2 image seeker.
//! - `vars`, `classes`: start sentinel, `RL` size, CRC (seed 0xC0C1) and end
//!   sentinel of the §9 and §10.1 sections, and that locator size = 38 + RL.
//! - `map`: every §23.1 chunk's big-endian CRC, the size of the largest
//!   chunk, and that the final size-2 chunk ends on the locator's end.
//! - `second-header`: where the §26 sentinel sits, its self-address and version
//!   string (the verdict), then, read as a bit stream (`L` as `BL`, `S` as
//!   `BS`), whether the rest follows §26: the locator copies against the first
//!   header's, the handle records.
//!
//! After each drawing it prints the object map's entry count and whether the
//! §9 header variables decode strictly and end on the size they state.
//!
//! ```sh
//! cargo run --release --example probe_r13_container -- a.dwg b.dwg
//! ```

use dwg::crc::{crc8, r13_locator_seed};
use dwg::streaming::{CLASS_SENTINELS, END_SENTINEL, HEADER_VARS_SENTINELS};
use dwg::{BitCursor, DwgFile, HeaderVars, Version};
use std::process::ExitCode;

const IMAGE_SENTINEL: [u8; 16] = [
    0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D, 0x44, 0x10, 0x2B,
];
const SECOND_HEADER_SENTINEL: [u8; 16] = [
    0xD4, 0x7B, 0x21, 0xCE, 0x28, 0x93, 0x9F, 0xBF, 0x53, 0x24, 0x40, 0x09, 0x12, 0x3C, 0xAA, 0x01,
];

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn verdict(ok: bool) -> &'static str {
    if ok { "ok" } else { "FAIL" }
}

fn section(b: &[u8], locator: Option<&(u8, u32, u32)>, sentinels: &[[u8; 16]; 2]) -> String {
    let Some(&(_, seeker, total)) = locator else {
        return "absent".into();
    };
    let at = seeker as usize;
    let Some(size) = u32le(b, at + 16) else {
        return "FAIL(truncated)".into();
    };
    let data_end = at + 20 + size as usize;
    let Some(stored) = b.get(data_end..data_end + 2) else {
        return "FAIL(truncated)".into();
    };
    let stored = u16::from_le_bytes([stored[0], stored[1]]);
    let framed = b.get(at..at + 16) == Some(&sentinels[0][..])
        && b.get(data_end + 2..data_end + 18) == Some(&sentinels[1][..])
        && u64::from(total) == 38 + u64::from(size);
    let crc = crc8(0xC0C1, &b[at + 16..data_end]) == stored;
    format!("{}(rl {size})", verdict(framed && crc))
}

fn map(b: &[u8], locator: Option<&(u8, u32, u32)>) -> String {
    let Some(&(_, seeker, total)) = locator else {
        return "absent".into();
    };
    let end = seeker as usize + total as usize;
    let (mut at, mut chunks, mut max, mut all_ok) = (seeker as usize, 0usize, 0usize, true);
    loop {
        let Some(head) = b.get(at..at + 2) else {
            return "FAIL(truncated)".into();
        };
        let n = usize::from(u16::from_be_bytes([head[0], head[1]]));
        let Some(stored) = b.get(at + n..at + n + 2) else {
            return "FAIL(truncated)".into();
        };
        all_ok &= crc8(0xC0C1, &b[at..at + n]) == u16::from_be_bytes([stored[0], stored[1]]);
        chunks += 1;
        max = max.max(n);
        at += n + 2;
        if n == 2 || at >= end {
            break;
        }
    }
    format!(
        "{}({chunks} chunks, max {max}, ends {})",
        verdict(all_ok && at == end),
        at == end
    )
}

fn second_header(b: &[u8], loc: &[(u8, u32, u32)], version: &[u8]) -> String {
    let Some(at) = b.windows(16).position(|w| w == SECOND_HEADER_SENTINEL) else {
        return "FAIL(no second-header sentinel)".into();
    };
    let ends = |n: u8| {
        loc.iter()
            .find(|l| l.0 == n && l.2 > 0)
            .map(|l| l.1 as usize + l.2 as usize)
    };
    let place = format!(
        "{at:#x} (locator 3 end {}, object map end {})",
        ends(3).is_some_and(|e| e == at),
        ends(2).is_some_and(|e| e == at)
    );
    let mut c = BitCursor::new(&b[at + 16..]);
    let mut stage = "size";
    let mut read = || -> dwg::Result<(bool, String)> {
        let size = c.read_rl()?;
        stage = "self-address";
        let own = c.read_bl()? as u32;
        stage = "version string";
        let magic: Vec<u8> = (0..6).map(|_| c.read_rc()).collect::<dwg::Result<_>>()?;
        let head_ok = own as usize == at && magic == version;
        stage = "pad and flags";
        let pad: Vec<u8> = (0..6).map(|_| c.read_rc()).collect::<dwg::Result<_>>()?;
        let nibble = (0..4)
            .map(|_| c.read_b())
            .collect::<dwg::Result<Vec<_>>>()?;
        let tail: Vec<u8> = (0..4).map(|_| c.read_rc()).collect::<dwg::Result<_>>()?;
        stage = "locator copies";
        let mut copies = Vec::new();
        for _ in 0..4 {
            let number = c.read_rc()?;
            copies.push((number, c.read_bl()? as u32, c.read_bl()? as u32));
        }
        let agree = copies.iter().all(|k| {
            loc.iter().any(|l| (l.0, l.1, l.2) == (k.0, k.1, k.2))
                || (k.1 == 0 && k.2 == 0 && !loc.iter().any(|l| l.0 == k.0))
        });
        if !agree {
            return Ok((
                head_ok,
                format!(
                    "body differs from section 26 at the locator copies (size {size}, pad {pad:02x?}, tail {tail:02x?})"
                ),
            ));
        }
        stage = "handle records";
        let records = c.read_bs()?;
        let mut seed = None;
        for i in 0..records.max(0) {
            let n = c.read_rc()?;
            let _index = c.read_rc()?;
            let mut value = 0u64;
            for _ in 0..n {
                value = value << 8 | u64::from(c.read_rc()?);
            }
            if i == 0 {
                seed = Some(value);
            }
        }
        Ok((
            head_ok,
            format!(
                "body follows section 26 (size {size}, pad {pad:02x?}, flag bits {}, tail {tail:02x?}, locator copies agree, {records} handle records, handseed {seed:?}, {} of {} bits)",
                nibble
                    .iter()
                    .map(|&x| u8::from(x).to_string())
                    .collect::<String>(),
                c.position_bits(),
                u64::from(size) * 8
            ),
        ))
    };
    match read() {
        Ok((head_ok, body)) => format!("{}({place}; {body})", verdict(head_ok)),
        Err(e) => format!("FAIL({place}; stops at {stage}: {e})"),
    }
}

fn main() -> ExitCode {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: probe_r13_container <file.dwg>...");
        return ExitCode::FAILURE;
    }
    let mut failed = false;
    for path in &paths {
        let b = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{path}: {e}");
                failed = true;
                continue;
            }
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        let count = u32le(&b, 0x15).unwrap_or(0) as usize;
        let loc: Vec<(u8, u32, u32)> = (0..count)
            .filter_map(|i| {
                let at = 0x19 + 9 * i;
                Some((*b.get(at)?, u32le(&b, at + 1)?, u32le(&b, at + 5)?))
            })
            .collect();
        let crc_at = 0x19 + 9 * count;
        let stored = b
            .get(crc_at..crc_at + 2)
            .map(|s| u16::from_le_bytes([s[0], s[1]]));
        let crc_ok = r13_locator_seed(count)
            .zip(stored)
            .is_some_and(|(seed, st)| crc8(0, &b[..crc_at]) ^ seed == st);
        let sentinel_ok = b.get(crc_at + 2..crc_at + 18) == Some(&END_SENTINEL[..]);
        let image_at = u32le(&b, 0x0D).unwrap_or(0) as usize;
        let image = if image_at == 0 {
            "absent".to_string()
        } else {
            let ok = b.get(image_at..image_at + 16) == Some(&IMAGE_SENTINEL[..]);
            format!(
                "{}(@{image_at:#x}, rl {:?})",
                verdict(ok),
                u32le(&b, image_at + 16)
            )
        };
        let second = second_header(&b, &loc, &b[..6]);
        let line = format!(
            "{name}  {}  pad {:02x?}  codepage {}  locators {count}  crc {}  sentinel {}  image {image}  vars {}  classes {}  map {}  second-header {second}",
            String::from_utf8_lossy(&b[..6.min(b.len())]),
            &b[6..13.min(b.len())],
            u16::from_le_bytes([b[0x13], b[0x14]]),
            verdict(crc_ok),
            verdict(sentinel_ok),
            section(&b, loc.iter().find(|l| l.0 == 0), &HEADER_VARS_SENTINELS),
            section(&b, loc.iter().find(|l| l.0 == 1), &CLASS_SENTINELS),
            map(&b, loc.iter().find(|l| l.0 == 2)),
        );
        failed |= line.contains("FAIL");
        println!("{line}");
        if let Ok(file) = DwgFile::from_bytes(b) {
            if let Some(Ok(handles)) = file.handle_map() {
                println!("    {} handle-map entries", handles.entries.len());
            }
            if let Some(Ok(bytes)) = file.read_section("AcDb:Header") {
                match HeaderVars::parse_strict(&bytes, file.version()) {
                    Ok(h) => println!(
                        "    header variables: {} slots, crc {}, end sentinel {}, fields {} on the stated end, {} unknown slots, {} diagnostics",
                        h.entries().len(),
                        verdict(h.crc_ok),
                        verdict(h.end_sentinel_ok),
                        if h.closed { "end" } else { "do not end" },
                        h.unknowns().count(),
                        h.diagnostics.len()
                    ),
                    Err(e) => {
                        let closes = |v: Version| {
                            HeaderVars::parse_strict(&bytes, v)
                                .map(|h| h.closed)
                                .unwrap_or(false)
                        };
                        println!(
                            "    header variables: FAIL({e}); with the R14 table {}, with the R2000 table {}, with the R2004 table {}",
                            closes(Version::R14),
                            closes(Version::R2000),
                            closes(Version::R2004)
                        );
                    }
                }
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
