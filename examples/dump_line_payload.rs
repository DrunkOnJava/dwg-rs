//! Dump the full LINE payload from line_2013.dwg + bit-walk what
//! position_cursor_at_entity_body does to it. This is the forensic
//! instrument for finding the 5-bit offset bug.

use dwg::DwgFile;
use dwg::bitcursor::BitCursor;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).expect("path arg");
    let file = DwgFile::open(&path)?;
    let objects = file.all_objects().expect("R2013 has handle walk")?;

    // The LINE we identified in the prior session:
    let line = objects
        .iter()
        .find(|o| o.type_code == 0x13)
        .expect("no LINE entity found");

    println!("=== LINE entity (type 0x13) ===");
    println!(
        "handle: 0x{:X}, stream_offset: {}, size_bytes: {}, payload len: {}",
        line.handle.value,
        line.stream_offset,
        line.size_bytes,
        line.raw.len()
    );
    println!();
    println!("full payload (hex):");
    for (i, chunk) in line.raw.chunks(16).enumerate() {
        print!("  {:04X}:", i * 16);
        for b in chunk {
            print!(" {:02X}", b);
        }
        println!();
    }
    println!();
    println!("full payload (binary, first 12 bytes):");
    for (i, b) in line.raw.iter().take(12).enumerate() {
        println!("  byte {:2} = 0x{:02X} = {:08b}", i, b, b);
    }
    println!();

    // Now replay position_cursor_at_entity_body step-by-step,
    // printing the cursor position after each field.
    println!("=== bit-walk ===");
    let mut c = BitCursor::new(&line.raw);
    println!("start: position_bits = {}", c.position_bits());

    // R2010+: MC unsigned (handle-stream-size-in-bits, byte-aligned).
    let mut mc_value: u64 = 0;
    let mut mc_shift: u32 = 0;
    loop {
        let b = c.read_rc()? as u64;
        let cont = (b & 0x80) != 0;
        let data = b & 0x7F;
        mc_value |= data << mc_shift;
        mc_shift += 7;
        if !cont || mc_shift >= 64 {
            break;
        }
    }
    println!(
        "after MC(handle-stream-size)={}: position_bits = {}",
        mc_value,
        c.position_bits()
    );

    // read_object_type (R2010+): BB dispatch tag + 1-2 bytes
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
    println!(
        "after read_object_type: tag={} type_code=0x{:04X} position_bits = {}",
        tag,
        type_code,
        c.position_bits()
    );

    // Handle: 4-bit code + 4-bit counter + counter bytes
    let handle = c.read_handle()?;
    println!(
        "after handle: code={} counter={} value=0x{:X} position_bits = {}",
        handle.code,
        handle.counter,
        handle.value,
        c.position_bits()
    );
    println!();
    println!("total payload bits: {}", line.raw.len() * 8);
    println!("bits consumed by preamble: {}", c.position_bits());
    println!(
        "bits remaining for entity body: {}",
        line.raw.len() * 8 - c.position_bits()
    );
    println!();

    // The LINE entity body (per spec §19.4.20) needs, for R2010+:
    // - common entity preamble
    // - B (is_2d flag)
    // - BD x3 for start (or special BBs if default)
    // - BD x3 for end (or BBs)
    // - BT (thickness)
    // - BE (extrusion)
    //
    // Common entity preamble (spec §19.2):
    // - BL (obj_size — R2000 only, not R2010+)
    // - BS (entmode)
    // - BL (num_reactors)
    // - BB (xdict flag, R2007+)
    // - B (no-links flag)
    // - CMC (entity color)
    // - BD (linetype_scale)
    // - BB (ltype flag)
    // - BB (plotstyle flag)
    // - BB (material flag, R2007+)
    // - BB (shadow flag, R2007+)
    // - BB (hasfullvisualstyle ...) R2010+
    // - BS (invisibility)
    // - RC (lineweight, R2000+)
    //
    // That's a LOT of bits. The error "5 bits remain" suggests the
    // decoder tried to read 8 bits but only 5 remained — so the
    // preamble overshoots the payload by 3 bits (cursor past end),
    // which means position_cursor_at_entity_body EITHER left the
    // cursor too far right, OR the common_entity preamble itself
    // is consuming more bits than the spec specifies.
    //
    // Report the raw next bits so we can eyeball what the common
    // entity preamble is about to read.
    println!("next ~80 bits at cursor position {}:", c.position_bits());
    let remaining = c.remaining_bits();
    let to_show = remaining.min(80);
    print!("  ");
    for i in 0..to_show {
        let bit = if c.read_b()? { 1 } else { 0 };
        if i > 0 && i % 8 == 0 {
            print!(" ");
        }
        print!("{}", bit);
    }
    println!();

    Ok(())
}
