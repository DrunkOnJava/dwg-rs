//! Empirical test of H2: the decoder runs past the data-stream
//! boundary (= payload_bits - handle_stream_size_bits). Construct a
//! truncated BitCursor view over just the data-stream portion, skip
//! past the object header, run common_entity + line::decode, and
//! see whether the LINE decodes to plausible values.
//!
//! If it does → H2 is confirmed; the fix is to bound the cursor to
//! the data stream in position_cursor_at_entity_body (or an analog).
//!
//! If it still errors → the data-stream boundary is not the whole
//! story and more investigation is needed.

use dwg::DwgFile;
use dwg::bitcursor::BitCursor;
use dwg::common_entity::read_common_entity_data;
use dwg::entities::line;
use dwg::Version;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = DwgFile::open("../../samples/line_2013.dwg")?;
    let objects = file.all_objects().unwrap()?;
    let line_obj = objects.iter().find(|o| o.type_code == 0x13).unwrap();

    let payload = &line_obj.raw;
    let payload_bits = payload.len() * 8;
    println!("payload: {} bytes = {} bits", payload.len(), payload_bits);

    // Full-payload decode (current behavior, expected to fail).
    println!("\n--- baseline: full-payload cursor ---");
    let r = try_decode(payload, "full-payload");
    println!("{r}");

    // Read the handle_stream_size ourselves.
    let mut probe = BitCursor::new(payload);
    let handle_stream_bits = read_mc_unsigned_local(&mut probe)?;
    println!("\nhandle_stream_size_bits = {}", handle_stream_bits);
    let data_stream_end_bits = payload_bits - handle_stream_bits as usize;
    let data_stream_end_bytes = data_stream_end_bits / 8;
    println!(
        "data_stream ends at bit {} (byte {})",
        data_stream_end_bits, data_stream_end_bytes
    );

    // H2 test: truncate the payload view to the data-stream bytes.
    let truncated = &payload[..data_stream_end_bytes];
    println!(
        "\n--- H2 test: truncated cursor ({} bytes, {} bits) ---",
        truncated.len(),
        truncated.len() * 8
    );
    let r = try_decode(truncated, "truncated");
    println!("{r}");

    Ok(())
}

fn try_decode(payload: &[u8], label: &str) -> String {
    let mut c = BitCursor::new(payload);

    // Skip the object header — same arithmetic as
    // position_cursor_at_entity_body.
    if let Err(e) = read_mc_unsigned_local(&mut c) {
        return format!("[{label}] MC read failed: {e}");
    }
    if let Err(e) = read_object_type_local(&mut c) {
        return format!("[{label}] read_object_type failed: {e}");
    }
    if let Err(e) = c.read_handle() {
        return format!("[{label}] read_handle failed: {e}");
    }
    let cursor_after_header = c.position_bits();

    let ced = match read_common_entity_data(&mut c, Version::R2013) {
        Ok(v) => v,
        Err(e) => {
            return format!(
                "[{label}] common_entity failed at bit {}: {e}",
                c.position_bits()
            );
        }
    };
    let cursor_after_common = c.position_bits();

    let line_decoded = line::decode(&mut c);
    let cursor_after_line = c.position_bits();

    match line_decoded {
        Ok(line) => format!(
            "[{label}] DECODED\n  after header: bit {}\n  after common: bit {}\n  after line:   bit {}\n  line = {:#?}\n  common = {:?}",
            cursor_after_header, cursor_after_common, cursor_after_line, line, ced
        ),
        Err(e) => format!(
            "[{label}] line::decode errored at bit {}: {e}\n  after header: bit {}\n  after common: bit {}\n  common = {:?}",
            cursor_after_line, cursor_after_header, cursor_after_common, ced
        ),
    }
}

fn read_mc_unsigned_local(c: &mut BitCursor<'_>) -> Result<u64, dwg::Error> {
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

/// Inlined R2010+ object-type dispatch (BB tag + 1–2 bytes), per spec §2.12.
fn read_object_type_local(c: &mut BitCursor<'_>) -> Result<u16, dwg::Error> {
    let tag = c.read_bb()?;
    let type_code = match tag {
        0 => c.read_rc()? as u16,
        1 => (c.read_rc()? as u16) + 0x1F0,
        _ => {
            let lsb = c.read_rc()? as u16;
            let msb = c.read_rc()? as u16;
            (msb << 8) | lsb
        }
    };
    Ok(type_code)
}
