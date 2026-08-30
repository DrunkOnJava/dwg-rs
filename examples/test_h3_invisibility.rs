//! Verify the R2013 common-entity invisibility read on `line_2013.dwg`.
//!
//! This started as an experiment for alternate invisibility encodings.
//! The boundary fix proved the invisibility field was not the root cause:
//! stale compatibility bits before CMC color and the shadow-field width
//! had shifted the cursor. The example now uses the production reader and
//! prints the decoded common preamble plus typed LINE geometry.

use dwg::bitcursor::BitCursor;
use dwg::common_entity::read_common_entity_data;
use dwg::entities::line;
use dwg::{DwgFile, Version};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = DwgFile::open("../../samples/line_2013.dwg")?;
    let objects = file.all_objects().unwrap()?;
    let line_obj = objects.iter().find(|o| o.type_code == 0x13).unwrap();
    let payload = &line_obj.raw;
    let payload_bits = payload.len() * 8;

    let mut c = BitCursor::new(payload);
    let handle_stream_bits = read_mc_unsigned(&mut c)?;
    let data_stream_end = payload_bits - handle_stream_bits as usize;

    let type_tag = c.read_bb()?;
    match type_tag {
        0 | 1 => {
            let _ = c.read_rc()?;
        }
        _ => {
            let _ = c.read_rc()?;
            let _ = c.read_rc()?;
        }
    }
    let _handle = c.read_handle()?;

    let common = read_common_entity_data(&mut c, Version::R2013)?;
    let body_start = c.position_bits();
    let decoded_line = line::decode(&mut c)?;

    println!(
        "payload_bits={payload_bits} handle_stream_bits={handle_stream_bits} data_stream_end={data_stream_end}"
    );
    println!(
        "common_end={body_start} invisibility={} lineweight=0x{:02X}",
        common.invisibility, common.lineweight
    );
    println!(
        "line=({:.6},{:.6},{:.6}) -> ({:.6},{:.6},{:.6}) 2d={}",
        decoded_line.start.x,
        decoded_line.start.y,
        decoded_line.start.z,
        decoded_line.end.x,
        decoded_line.end.y,
        decoded_line.end.z,
        decoded_line.is_2d
    );

    Ok(())
}

fn read_mc_unsigned(c: &mut BitCursor<'_>) -> Result<u64, dwg::Error> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let b = c.read_rc()? as u64;
        let cont = (b & 0x80) != 0;
        let data = b & 0x7F;
        value |= data << shift;
        shift += 7;
        if !cont || shift >= 64 {
            return Ok(value);
        }
    }
}
