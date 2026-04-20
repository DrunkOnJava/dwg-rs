//! Forensic per-field tracer for `read_common_entity_data`.
//!
//! Decodes the common entity preamble of the LINE at offset 11884
//! in line_2013.dwg one field at a time, printing (field name,
//! bit start, bit end, raw bits read, decoded value). The output is
//! meant to be compared against a clean-room reading of ODA OpenDS
//! §19.4.1 to localize field-order drift.
//!
//! This exists because:
//!   - The full decode fails with "wanted 8 bits, 5 remain" 3 bits
//!     past the data-stream boundary.
//!   - `invisibility` reads as -10207 (0xD821 LE u16), which is
//!     impossible for a real LINE.
//!   - The preamble consumes 52 bits (bit 34 → bit 86). Expected
//!     minimum is 36 for R2013+ per the current module's field list,
//!     so invisibility is taking the BS tag=00 literal path (18 bits).
//!
//! If an expected-but-missing field is inserted between plotstyle
//! and invisibility (CMC color, BD linetype_scale, etc.), that same
//! bit window gets eaten by the new field, invisibility lands on the
//! real 16-bit invisibility value (0 or 1), and the LINE decoder
//! starts at a cursor position ~20-25 bits further, eliminating the
//! overshoot.

use dwg::DwgFile;
use dwg::bitcursor::BitCursor;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = DwgFile::open("../../samples/line_2013.dwg")?;
    let objects = file.all_objects().unwrap()?;
    let line = objects.iter().find(|o| o.type_code == 0x13).unwrap();
    let payload = &line.raw;
    println!("payload: {} bytes = {} bits", payload.len(), payload.len() * 8);

    let mut c = BitCursor::new(payload);
    println!();
    println!("=== object header ===");
    let mc = read_mc_unsigned(&mut c)?;
    report(&mut c, "MC handle_stream_size", format!("{mc} bits"));
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
    report(&mut c, "BB + type_code", format!("tag={tag} type=0x{type_code:04X}"));
    let h = c.read_handle()?;
    report(
        &mut c,
        "Handle code/counter/value",
        format!("code={} counter={} value=0x{:X}", h.code, h.counter, h.value),
    );

    println!();
    println!("=== common_entity preamble (spec §19.4.1) ===");

    // Extended-data loop. Terminates on BS_u == 0.
    let size0 = c.read_bs_u()?;
    report(&mut c, "BS_u XDATA size", format!("{size0}"));
    if size0 != 0 {
        println!("WARN: XDATA loop didn't terminate — multi-iteration case not traced");
    }

    // Graphics-preview flag.
    let had_gfx = c.read_b()?;
    report(&mut c, "B had_graphics", format!("{had_gfx}"));

    let entmode = c.read_bb()?;
    report(
        &mut c,
        "BB entmode",
        format!("{entmode} ({})", match entmode {
            0 => "ByLayer",
            1 => "ByPreviousEntity",
            2 => "InBlock",
            _ => "Reserved",
        }),
    );

    let reactors = c.read_bl()?;
    report(&mut c, "BL num_reactors", format!("{reactors}"));

    let no_xdict = c.read_b()?;
    report(&mut c, "B no_xdictionary", format!("{no_xdict}"));

    let binary_chain = c.read_b()?; // R2004+
    report(&mut c, "B binary_chain (R2004+)", format!("{binary_chain}"));

    let is_on_layer = c.read_b()?;
    report(&mut c, "B is_on_layer", format!("{is_on_layer}"));

    let non_fixed_ltype = c.read_b()?;
    report(&mut c, "B non_fixed_ltype", format!("{non_fixed_ltype}"));

    let plotstyle = c.read_bb()?;
    report(&mut c, "BB plotstyle_flag", format!("{plotstyle}"));

    let material = c.read_bb()?; // R2007+
    report(&mut c, "BB material (R2007+)", format!("{material}"));

    let shadow = c.read_rc()?; // R2007+
    report(&mut c, "RC shadow_flags (R2007+)", format!("0x{shadow:02X}"));

    let vs_full = c.read_b()?; // R2010+
    let vs_face = c.read_b()?;
    let vs_edge = c.read_b()?;
    report(
        &mut c,
        "3B visualstyle full/face/edge (R2010+)",
        format!("full={vs_full} face={vs_face} edge={vs_edge}"),
    );

    // ---- THIS IS WHERE H3 SAYS MORE FIELDS BELONG ---- //
    // Expected per spec §19.4.1 R2004+ before invisibility:
    //   - CMC entity_color (BS + flags, ~2-18 bits)
    //   - BD linetype_scale (2-66 bits)
    //   - H handles (deferred to handle stream, no data-stream bits)
    // Current decoder reads BS invisibility here — likely consuming
    // bits that should belong to CMC / BD above.

    let inv = c.read_bs()?;
    report(
        &mut c,
        "BS invisibility (SUSPECT — likely misaligned)",
        format!("{inv} (0x{:04X} as i16; valid values: 0 or 1)", inv as u16),
    );

    let lw = c.read_rc()?;
    report(&mut c, "RC lineweight (R2000+)", format!("0x{lw:02X}"));

    println!();
    println!("=== remaining bits after preamble ===");
    let pos = c.position_bits();
    let total = payload.len() * 8;
    println!("cursor: bit {}/{} ({} bits remain in full payload)", pos, total, total - pos);
    let data_stream_end = total - (mc as usize);
    println!(
        "data stream ends at bit {} → {} bits remain in data stream",
        data_stream_end,
        data_stream_end.saturating_sub(pos),
    );
    println!();
    println!("Expected next: LINE body (§19.4.20)");
    println!("  B zflag → RD start.x → BD end.x delta → RD start.y → BD end.y delta");
    println!("  → (if !zflag: RD start.z → BD end.z delta) → BT thickness → BE extrusion");
    println!();
    println!("Minimum 2D LINE body = 1 + 64 + 2 + 64 + 2 + 1 + 1 = 135 bits");
    println!("Minimum 3D LINE body = 135 + 64 + 2 = 201 bits");
    println!();
    println!("If H3 holds, the preamble is missing fields that consume ~20-25 bits");
    println!("before invisibility, so the real cursor position after preamble should");
    println!("be ~bit 106-111, not bit 86 as currently decoded.");

    Ok(())
}

fn report(c: &mut BitCursor<'_>, name: &str, value: String) {
    let pos = c.position_bits();
    println!("  [bit {:3}] {:<44} = {}", pos, name, value);
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
