//! Where do the non-entity objects' data fields have to end?
//!
//! # What this measures
//!
//! Every object record carries a boundary the decoder can check itself
//! against ([`dwg::object::data_end_bit`]):
//!
//! - R2010+ — the first bit of the object's string stream (§19.1), or
//!   the start of its handle stream when it holds no strings;
//! - R2000-R2007 — the `RL` object-data-size-in-bits from the object
//!   prologue, which marks the same boundary inline.
//!
//! For every DICTIONARY / XRECORD / LAYOUT / GROUP / MLINESTYLE /
//! `*_CONTROL` / custom-class object this probe prints the bit budget
//! between the end of the common object prefix and that boundary, for
//! both candidate prefixes:
//!
//! - **A** = `EED` chain, `B` xdictionary-missing (R2004+), `B` AcDs
//!   binary-data flag (R2013+) — the shape
//!   `crate::tables::modern::skip_object_prefix` measured for R2007+
//!   symbol-table entries;
//! - **B** = the same plus the `BL` reactor count
//!   ([`dwg::common_entity::read_common_object_data`]).
//!
//! A candidate body layout is correct only when its field widths sum
//! exactly to one of those budgets. Anything else is a guess.
//!
//! ```sh
//! cargo run --release --example probe_object_layout -- samples/sample_AC1032.dwg 0x2A
//! ```
//!
//! With no type-code argument every non-entity object is printed.

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::string_stream::{self, StringReader};
use dwg::{DwgFile, ObjectType};

/// Advance `c` past the EED chain (`BS` size, `H` appid, `size` bytes,
/// repeating until a zero size).
fn skip_eed(c: &mut BitCursor<'_>) -> Result<()> {
    for _ in 0..256 {
        let size = c.read_bs_u()? as usize;
        if size == 0 {
            return Ok(());
        }
        let _appid = c.read_handle()?;
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: <file.dwg> [type-code]");
    let want: Option<u16> = args.next().map(|s| {
        let t = s.trim_start_matches("0x");
        u16::from_str_radix(t, if s.starts_with("0x") { 16 } else { 10 })
            .expect("type code must be decimal or 0x-hex")
    });

    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file
        .all_objects()
        .expect("this version has no object-stream walk")?;
    let class_map = file.class_map().and_then(std::result::Result::ok);

    println!("{path}  ({version})");
    println!(
        "{:<6} {:<20} {:>10} {:>8} {:>8} {:>7} {:>7}  strings",
        "code", "kind/class", "handle", "data_end", "body", "budgetA", "budgetB"
    );

    for object in &objects {
        if let Some(code) = want {
            if object.type_code != code {
                continue;
            }
        } else if object.is_entity() || object.kind.is_table_entry() {
            continue;
        }

        let label = match object.kind {
            ObjectType::Custom(code) => class_map
                .as_ref()
                .and_then(|m| m.by_type_code(code))
                .map(|d| d.dxf_class_name.clone())
                .unwrap_or_else(|| format!("{}", object.kind)),
            other => format!("{other}"),
        };

        let data_end = dwg::object::data_end_bit(object, version);
        let body = dwg::object::body_cursor(object, version).map(|c| c.position_bits());

        let budget = |extra_reactor_count: bool| -> Option<isize> {
            let mut c = dwg::object::body_cursor(object, version).ok()?;
            skip_eed(&mut c).ok()?;
            if extra_reactor_count {
                c.read_bl().ok()?;
            }
            if version.is_r2004_plus() {
                c.read_b().ok()?;
            }
            if matches!(version, dwg::Version::R2013 | dwg::Version::R2018) {
                let has_binary = c.read_b().ok()?;
                if has_binary {
                    c.read_rc().ok()?;
                }
            }
            Some(data_end? as isize - c.position_bits() as isize)
        };

        let mut strings = Vec::new();
        if let Some(stream) = string_stream::locate(&object.raw, version) {
            if let Ok(mut r) = StringReader::new(&object.raw, stream) {
                while !r.is_exhausted() {
                    match r.read_tv() {
                        Ok(s) => strings.push(s.chars().take(32).collect::<String>()),
                        Err(_) => break,
                    }
                }
            }
        }

        let mut bits = String::new();
        if let (Some(from), Some(to)) = (body.as_ref().ok().copied(), data_end) {
            for bit in from..to.min(from + 256) {
                let byte = object.raw.get(bit / 8).copied().unwrap_or(0);
                bits.push(if (byte >> (7 - (bit % 8))) & 1 != 0 {
                    '1'
                } else {
                    '0'
                });
            }
        }

        println!(
            "0x{:04X} {:<20} {:>10} {:>8} {:>8} {:>7} {:>7}  {} {:?}",
            object.type_code,
            label,
            object.handle.value,
            data_end.map(|v| v as isize).unwrap_or(-1),
            body.map(|v| v as isize).unwrap_or(-1),
            budget(false).unwrap_or(isize::MIN),
            budget(true).unwrap_or(isize::MIN),
            bits,
            strings
        );
    }
    Ok(())
}
