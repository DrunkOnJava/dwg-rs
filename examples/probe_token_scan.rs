//! Where are the anchors in a record whose field list is unknown?
//!
//! # What this proves
//!
//! A field list derived from bytes alone needs anchor points: bit
//! offsets that can only plausibly be one thing. Two token forms are
//! self-identifying because they carry redundancy the surrounding bits
//! do not:
//!
//! - a full-form `BD` (2-bit prefix `00` then eight IEEE-754 bytes)
//!   whose value comes out as a short decimal — `0.09`, `0.18`, `1.5`,
//!   `45` — rather than a denormal or a 1e+240;
//! - a `CMC` whose true-colour `BL` word carries one of the colour
//!   method octets in its top byte (`0xC0` ByLayer, `0xC1` ByBlock,
//!   `0xC2` RGB, `0xC3` ACI index, `0xC8` none) and a colour byte
//!   below 4.
//!
//! Neither pattern survives a one-bit misalignment, so every hit is a
//! candidate field start, and the gaps between hits are what a
//! candidate token sequence has to fill exactly. Combined with the
//! data-stream boundary the record itself carries
//! (`dwg::object::data_end_bit`), that is enough to derive a field list
//! for an object the ODA specification does not prescribe.
//!
//! ```sh
//! cargo run --release --example probe_token_scan -- samples/arc_2013.dwg 107
//! ```
//!
//! The scan covers the record's whole data stream, so it reports hits
//! that are coincidence as well as hits that are fields; a hit is
//! evidence only once a token sequence reaches it exactly.

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::{DwgFile, Version};

/// Advance `c` to absolute bit `bit` (the cursor has no random access).
fn skip_to(c: &mut BitCursor<'_>, bit: usize) -> Result<()> {
    while c.position_bits() < bit {
        let _ = c.read_b()?;
    }
    Ok(())
}

/// Is `v` the kind of double a CAD author would have typed?
fn is_nice_double(v: f64) -> bool {
    if !v.is_finite() {
        return false;
    }
    if v == 0.0 {
        return true;
    }
    let a = v.abs();
    if !(1e-4..=1e7).contains(&a) {
        return false;
    }
    // Round-trips through five decimals — excludes the 0.10000000149
    // style noise a misaligned read produces.
    (v * 1e5).round() / 1e5 == v || format!("{v:.6}").parse::<f64>() == Ok(v)
}

/// Colour method octets a real `CMC` true-colour word carries.
fn is_color_method(rgb: u32) -> bool {
    matches!(rgb >> 24, 0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC8)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: <file.dwg> <handle>");
    let handle: u64 = args
        .next()
        .expect("usage: <file.dwg> <handle>")
        .parse()
        .expect("handle must be decimal");

    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file
        .all_objects()
        .expect("this version has no object-stream walk")?;
    let object = objects
        .iter()
        .find(|o| o.handle.value == handle)
        .unwrap_or_else(|| panic!("no object with handle {handle}"));

    let body = dwg::object::body_cursor(object, version)?.position_bits();
    let end = dwg::object::data_end_bit(object, version).expect("record has no data-stream end");
    println!("{path}  ({version})  handle {handle}");
    println!("body starts at bit {body}, data stream ends at {end}");

    for at in body..end {
        let mut c = BitCursor::new(&object.raw);
        if skip_to(&mut c, at).is_err() {
            break;
        }
        if let Ok(v) = c.read_bd() {
            if c.position_bits() == at + 66 && is_nice_double(v) {
                println!("  @{at:<6} BD  {v}");
            }
        }
        let mut c = BitCursor::new(&object.raw);
        if skip_to(&mut c, at).is_err() {
            break;
        }
        let cmc = (|| -> Result<(u16, u32, u8)> {
            let index = c.read_bs_u()?;
            if !version.is_r2004_plus() {
                return Ok((index, 0, 0));
            }
            let rgb = c.read_bl_u()?;
            let flag = c.read_rc()?;
            Ok((index, rgb, flag))
        })();
        if let Ok((index, rgb, flag)) = cmc {
            if is_color_method(rgb) && flag < 4 && index < 8 {
                println!(
                    "  @{at:<6} CMC index {index} rgb {rgb:#010X} flag {flag} (w{})",
                    c.position_bits() - at
                );
            }
        }
    }

    if version.is_r2010_plus() {
        let mut strings = Vec::new();
        if let Some(stream) = dwg::string_stream::locate(&object.raw, version) {
            let mut r = dwg::string_stream::StringReader::new(&object.raw, stream)?;
            while !r.is_exhausted() {
                match r.read_tv() {
                    Ok(s) => strings.push(s),
                    Err(_) => break,
                }
            }
        }
        println!("strings: {strings:?}");
    }
    let _ = Version::R2018;
    Ok(())
}
