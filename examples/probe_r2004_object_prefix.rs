//! Which common-object prefix does a pre-R2007 non-entity record carry
//! between its handle and its first field? (ODA §19.4.2.)
//!
//! # What this proves
//!
//! Symbol-table entries, dictionaries and the other non-drawable
//! objects do not use the entity preamble of §19.4.1 — they carry a
//! shorter one. The candidates differ only in whether the reactor
//! count and the xdictionary-missing flag are present, so the cheapest
//! discriminator is the record's own name: read each candidate prefix,
//! then read the `TV` that must follow it, and see which one yields
//! text.
//!
//! Running this over `line_2004.dwg` shows only `EED + BL num_reactors
//! + B xdic-missing` (a 5-bit prefix on records with no EED) produces
//! readable names — `0`, `Standard`, `Annotative`, `ACAD`,
//! `AcadAnnotative`, `ByBlock`, `ByLayer`, `Continuous`.
//!
//! Dropping the `BL` turns every one of them into a `TV` length prefix
//! of 65-73 characters, and reading nothing at all yields an empty
//! string.
//!
//! # How to verify
//!
//! ```sh
//! # 0x33 LAYER, 0x35 STYLE, 0x39 LTYPE, 0x43 APPID
//! cargo run --release --example probe_r2004_object_prefix -- samples/line_2004.dwg 33
//! ```

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::{DwgFile, Version};

/// Position a cursor just past the object header (§19.1 prologue).
fn seek_past_object_header<'a>(raw: &'a [u8], version: Version) -> Result<BitCursor<'a>> {
    let mut c = BitCursor::new(raw);
    if version.is_r2010_plus() {
        while c.read_rc()? & 0x80 != 0 {}
        match c.read_bb()? {
            0 | 1 => {
                c.read_rc()?;
            }
            _ => {
                c.read_rc()?;
                c.read_rc()?;
            }
        }
    } else {
        c.read_bs_u()?;
    }
    if version.has_object_size_field() {
        c.read_rl()?;
    }
    c.read_handle()?;
    Ok(c)
}

/// Consume the EED chain (§19.4.1) — `<BS size, H appid, RC*size>`
/// groups terminated by `size == 0`.
fn skip_eed(c: &mut BitCursor<'_>) -> Result<()> {
    for _ in 0..64 {
        let size = c.read_bs_u()?;
        if size == 0 {
            return Ok(());
        }
        c.read_handle()?;
        for _ in 0..size {
            c.read_rc()?;
        }
    }
    Ok(())
}

/// Read the `TV` a table entry's name occupies on pre-R2007 files,
/// rendering an implausible length as a marker rather than a string.
fn read_name(c: &mut BitCursor<'_>) -> String {
    let Ok(len) = c.read_bs_u() else {
        return "<read error>".into();
    };
    if len as usize > 64 {
        return format!("<implausible TV length {len}>");
    }
    let mut bytes = Vec::with_capacity(len as usize);
    for _ in 0..len {
        match c.read_rc() {
            Ok(b) => bytes.push(b),
            Err(_) => return "<truncated>".into(),
        }
    }
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The prefixes worth testing, as (label, reads-BL, reads-B) triples.
const CANDIDATES: &[(&str, bool, bool, bool)] = &[
    ("nothing", false, false, false),
    ("eed", true, false, false),
    ("eed+bl", true, true, false),
    ("eed+xdic", true, false, true),
    ("eed+bl+xdic", true, true, true),
];

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: <file.dwg> <type-code-hex>");
    let want = u16::from_str_radix(&args.next().expect("type-code-hex"), 16)
        .expect("type code must be hexadecimal");

    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file
        .all_objects()
        .expect("this version has no object-stream walk")?;

    for object in objects.iter().filter(|o| o.type_code == want) {
        println!(
            "--- handle 0x{:X}  type 0x{:02X}  obj_size={:?}  payload_bits={} ---",
            object.handle.value,
            object.type_code,
            object.obj_size_bits,
            object.raw.len() * 8
        );
        for &(label, eed, bl, xdic) in CANDIDATES {
            let mut c = seek_past_object_header(&object.raw, version)?;
            let base = c.position_bits();
            let read = (|| -> Result<()> {
                if eed {
                    skip_eed(&mut c)?;
                }
                if bl {
                    c.read_bl()?;
                }
                if xdic {
                    c.read_b()?;
                }
                Ok(())
            })();
            if read.is_err() {
                println!("  {label:12} prefix unreadable");
                continue;
            }
            let prefix_bits = c.position_bits() - base;
            println!(
                "  {label:12} prefix={prefix_bits:3} bits  name={:?}",
                read_name(&mut c)
            );
        }
    }
    Ok(())
}
