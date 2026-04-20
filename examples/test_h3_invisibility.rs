//! Empirical test of H3 (revised): the BS invisibility read in the
//! common-entity preamble is consuming 18 bits (tag 00 → 16-bit
//! literal) when it should consume far fewer. Re-decode the LINE
//! preamble inline with EXPERIMENTAL changes:
//!
//! Variant A — invisibility as a single B (1 bit) instead of BS.
//! Variant B — skip invisibility entirely (assume it lives in the
//!             handle stream for R2010+).
//! Variant C — read invisibility as RS (raw 16-bit short, 16 bits)
//!             instead of BS tag-prefix encoded.
//!
//! For each variant: walk the preamble inline (no reliance on the
//! crate's read_common_entity_data), then run line::decode against
//! the resulting cursor and report success/failure with bit
//! position. The variant that yields a clean LINE decode is the
//! right one.

use dwg::DwgFile;
use dwg::bitcursor::BitCursor;
use dwg::entities::line;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = DwgFile::open("../../samples/line_2013.dwg")?;
    let objects = file.all_objects().unwrap()?;
    let line_obj = objects.iter().find(|o| o.type_code == 0x13).unwrap();
    let payload = &line_obj.raw;
    let payload_bits = payload.len() * 8;

    // Compute data-stream end.
    let mut probe = BitCursor::new(payload);
    let handle_stream_bits = read_mc_unsigned(&mut probe)?;
    let data_stream_end = payload_bits - handle_stream_bits as usize;
    println!(
        "payload {} bits, handle_stream {} bits, data_stream ends at bit {}",
        payload_bits, handle_stream_bits, data_stream_end
    );

    for label in ["baseline (BS, current)", "A: B (1 bit)", "B: skip invis", "C: RS (16 bits)"] {
        println!("\n=== {label} ===");
        match try_variant(payload, label) {
            Ok((before, after, line)) => println!(
                "  DECODED  preamble→bit {before}  line→bit {after} (consumed {} bits)\n  line = {:?}",
                after - before, line
            ),
            Err((stage, pos, e)) => println!("  FAILED at {stage}, bit {pos}: {e}"),
        }
    }

    Ok(())
}

fn try_variant(
    payload: &[u8],
    label: &str,
) -> Result<(usize, usize, line::Line), (&'static str, usize, dwg::Error)> {
    let mut c = BitCursor::new(payload);
    // Object header: MC handle_stream_size, BB+RC type_code, Handle.
    read_mc_unsigned(&mut c).map_err(|e| ("header MC", c.position_bits(), e))?;
    let tag = c.read_bb().map_err(|e| ("header BB", c.position_bits(), e))?;
    match tag {
        0 => {
            c.read_rc().map_err(|e| ("type_code", c.position_bits(), e))?;
        }
        1 => {
            c.read_rc().map_err(|e| ("type_code", c.position_bits(), e))?;
        }
        _ => {
            c.read_rc().map_err(|e| ("type_code lsb", c.position_bits(), e))?;
            c.read_rc().map_err(|e| ("type_code msb", c.position_bits(), e))?;
        }
    }
    c.read_handle().map_err(|e| ("handle", c.position_bits(), e))?;

    // Preamble — inline copy of read_common_entity_data with the
    // variant-under-test substitution.
    // XDATA loop: BS_u terminator.
    loop {
        let size = c.read_bs_u().map_err(|e| ("xdata size", c.position_bits(), e))?;
        if size == 0 {
            break;
        }
        c.read_handle().map_err(|e| ("xdata appid", c.position_bits(), e))?;
        for _ in 0..size {
            c.read_rc().map_err(|e| ("xdata payload", c.position_bits(), e))?;
        }
    }
    // Graphics flag.
    if c.read_b().map_err(|e| ("graphics flag", c.position_bits(), e))? {
        let n = c.read_rl().map_err(|e| ("graphics size", c.position_bits(), e))?;
        for _ in 0..n {
            c.read_rc().map_err(|e| ("graphics payload", c.position_bits(), e))?;
        }
    }
    // Mode + reactors + dict markers.
    c.read_bb().map_err(|e| ("entmode", c.position_bits(), e))?;
    c.read_bl().map_err(|e| ("num_reactors", c.position_bits(), e))?;
    c.read_b().map_err(|e| ("no_xdict", c.position_bits(), e))?;
    c.read_b().map_err(|e| ("binary_chain", c.position_bits(), e))?;
    c.read_b().map_err(|e| ("is_on_layer", c.position_bits(), e))?;
    c.read_b().map_err(|e| ("non_fixed_ltype", c.position_bits(), e))?;
    c.read_bb().map_err(|e| ("plotstyle", c.position_bits(), e))?;
    // R2007+: material + shadow
    c.read_bb().map_err(|e| ("material", c.position_bits(), e))?;
    c.read_rc().map_err(|e| ("shadow", c.position_bits(), e))?;
    // R2010+: visualstyle 3B
    c.read_b().map_err(|e| ("vs_full", c.position_bits(), e))?;
    c.read_b().map_err(|e| ("vs_face", c.position_bits(), e))?;
    c.read_b().map_err(|e| ("vs_edge", c.position_bits(), e))?;

    // Variant-under-test: invisibility encoding.
    match label {
        "baseline (BS, current)" => {
            c.read_bs().map_err(|e| ("invis BS", c.position_bits(), e))?;
        }
        "A: B (1 bit)" => {
            c.read_b().map_err(|e| ("invis B", c.position_bits(), e))?;
        }
        "B: skip invis" => {
            // no read
        }
        "C: RS (16 bits)" => {
            c.read_rs().map_err(|e| ("invis RS", c.position_bits(), e))?;
        }
        _ => unreachable!(),
    }
    // Lineweight (R2000+): RC = 8 bits.
    c.read_rc().map_err(|e| ("lineweight", c.position_bits(), e))?;

    let preamble_end = c.position_bits();
    let line = line::decode(&mut c).map_err(|e| ("line::decode", c.position_bits(), e))?;
    let line_end = c.position_bits();
    Ok((preamble_end, line_end, line))
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
