//! Walk the R2007+ MTEXT data fields and report the distance to the
//! string-stream start bit after each one (ODA §19.1, §19.4.44).
//!
//! # What this proves
//!
//! Two things, both used by `src/entities/mtext.rs`:
//!
//! 1. **The text belongs in the string stream.** Every MTEXT record's
//!    stream holds exactly one `TV` and is consumed to the bit by it,
//!    so `read_tv` on a freshly opened reader returns the text and
//!    leaves the reader exhausted — a self-validating check.
//! 2. **The R2018 field list is not finished.** The fields the decoder
//!    understands stop well short of the string stream (567 bits short
//!    on the first MTEXT of `sample_AC1032.dwg`), and the raw bits in
//!    that gap repeat the record's own geometry, so `mtext.rs`
//!    deliberately asserts `data_end <= string_start` rather than the
//!    exact equality the symbol-table decoders use.
//!
//! # How to verify
//!
//! ```sh
//! cargo run --release --example probe_mtext_fields -- samples/sample_AC1032.dwg
//! ```
//!
//! Each record prints its strings, then one line per field with the
//! bit reached and the bits still to go before the string stream.

use dwg::bitcursor::BitCursor;
use dwg::entities::read_bd3;
use dwg::error::Result;
use dwg::string_stream::{self, StringReader};
use dwg::{DwgFile, Version};

/// MTEXT's fixed object type code (§5 Table 4).
const OBJECT_TYPE_MTEXT: u16 = 0x2C;

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

fn walk(raw: &[u8], version: Version, start: usize) -> Result<()> {
    let mut c = seek_past_object_header(raw, version)?;
    dwg::common_entity::read_common_entity_data(&mut c, version)?;
    let step = |label: &str, c: &BitCursor<'_>| {
        println!(
            "    {label:34} bit {:5}   to string stream {:6}",
            c.position_bits(),
            start as isize - c.position_bits() as isize
        );
    };
    step("common entity preamble", &c);
    read_bd3(&mut c)?;
    step("3BD insertion point", &c);
    read_bd3(&mut c)?;
    step("3BD extrusion", &c);
    read_bd3(&mut c)?;
    step("3BD x-axis direction", &c);
    let v = c.read_bd()?;
    step(&format!("BD rect width = {v}"), &c);
    let v = c.read_bd()?;
    step(&format!("BD rect height = {v}"), &c);
    let v = c.read_bd()?;
    step(&format!("BD text height = {v}"), &c);
    let v = c.read_bs()?;
    step(&format!("BS attachment = {v}"), &c);
    let v = c.read_bs()?;
    step(&format!("BS drawing direction = {v}"), &c);
    let v = c.read_bd()?;
    step(&format!("BD extents height = {v}"), &c);
    let v = c.read_bd()?;
    step(&format!("BD extents width = {v}"), &c);
    let v = c.read_bs()?;
    step(&format!("BS linespace style = {v}"), &c);
    let v = c.read_bd()?;
    step(&format!("BD linespace factor = {v}"), &c);
    let v = c.read_b()?;
    step(&format!("B unknown = {v}"), &c);
    let flags = c.read_bl()?;
    step(&format!("BL background flags = {flags}"), &c);
    if flags & 0x01 != 0 {
        let v = c.read_bd()?;
        step(&format!("BD background scale = {v}"), &c);
        let v = c.read_bs_u()?;
        step(&format!("BS background colour index = {v}"), &c);
        let v = c.read_bl_u()?;
        step(&format!("BL background colour rgb = 0x{v:08X}"), &c);
        let v = c.read_rc()?;
        step(&format!("RC background colour byte = {v}"), &c);
        let v = c.read_bl()?;
        step(&format!("BL background transparency = {v}"), &c);
    }
    Ok(())
}

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg>");
    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file
        .all_objects()
        .expect("this version has no object-stream walk")?;

    for object in objects.iter().filter(|o| o.type_code == OBJECT_TYPE_MTEXT) {
        let Some(stream) = string_stream::locate(&object.raw, version) else {
            println!(
                "=== handle 0x{:X}  NO string stream (empty text), payload_bits={} ===",
                object.handle.value,
                object.raw.len() * 8
            );
            continue;
        };
        println!(
            "=== handle 0x{:X}  payload_bits={}  string_start={}  string_bits={} ===",
            object.handle.value,
            object.raw.len() * 8,
            stream.start_bit,
            stream.len_bits()
        );
        if let Ok(mut reader) = StringReader::new(&object.raw, stream) {
            let mut index = 0;
            while !reader.is_exhausted() {
                match reader.read_tv() {
                    Ok(s) => println!("    string[{index}] = {s:?}"),
                    Err(e) => {
                        println!("    string[{index}] error: {e}");
                        break;
                    }
                }
                index += 1;
            }
        }
        if let Err(e) = walk(&object.raw, version, stream.start_bit) {
            println!("    field walk stopped: {e}");
        }
    }
    Ok(())
}
