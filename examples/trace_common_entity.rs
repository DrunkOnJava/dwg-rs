//! Forensic per-field tracer for `read_common_entity_data`.
//!
//! Decodes the common entity preamble of the LINE at offset 11884
//! in line_2013.dwg one field at a time, printing (field name,
//! bit start, bit end, raw bits read, decoded value). The output is
//! meant to be compared against a clean-room reading of ODA OpenDS
//! §19.4.1 to localize field-order drift.
//!
//! This exists because the R2013/R2018 boundary is easy to regress:
//! two stale compatibility bits before CMC color and a 2-bit shadow
//! read pushed the LINE body off by 68 bits. The corrected R2013 path
//! stops at bit 74 for `line_2013.dwg`, exactly where the typed LINE
//! body decodes as `(50,50,0) -> (100,100,0)`.

use dwg::DwgFile;
use dwg::Version;
use dwg::bitcursor::BitCursor;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = DwgFile::open("../../samples/line_2013.dwg")?;
    let objects = file.all_objects().unwrap()?;
    let line = objects.iter().find(|o| o.type_code == 0x13).unwrap();
    let payload = &line.raw;
    println!(
        "payload: {} bytes = {} bits",
        payload.len(),
        payload.len() * 8
    );

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
    report(
        &mut c,
        "BB + type_code",
        format!("tag={tag} type=0x{type_code:04X}"),
    );
    let h = c.read_handle()?;
    report(
        &mut c,
        "Handle code/counter/value",
        format!(
            "code={} counter={} value=0x{:X}",
            h.code, h.counter, h.value
        ),
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
        format!(
            "{entmode} ({})",
            match entmode {
                0 => "ByLayer",
                1 => "ByPreviousEntity",
                2 => "InBlock",
                _ => "Reserved",
            }
        ),
    );

    let reactors = c.read_bl()?;
    report(&mut c, "BL num_reactors", format!("{reactors}"));

    let no_xdict = c.read_b()?;
    report(&mut c, "B no_xdictionary", format!("{no_xdict}"));

    let has_ds_data = c.read_b()?; // R2013+
    report(
        &mut c,
        "B has_ds_binary_data (R2013+)",
        format!("{has_ds_data}"),
    );

    // CMC color (R2004+: BS index + optional complex suffix).
    let color_raw = c.read_bs_u()?;
    report(
        &mut c,
        "BS CMC raw color",
        format!("{color_raw} (0x{color_raw:04X})"),
    );
    let color_flags = color_raw >> 8;
    if color_flags & 0x20 != 0 {
        let alpha = c.read_bl()?;
        report(&mut c, "BL CMC alpha suffix", format!("0x{alpha:08X}"));
    }
    if color_flags & 0x40 == 0 && color_flags & 0x80 != 0 {
        let rgb = c.read_bl()?;
        report(&mut c, "BL CMC rgb suffix", format!("0x{rgb:08X}"));
    }
    if color_flags & 0x41 == 0x41 {
        let name = read_tv(&mut c, file.version())?;
        report(&mut c, "TV CMC color name suffix", format!("{name:?}"));
    }
    if color_flags & 0x42 == 0x42 {
        let book_name = read_tv(&mut c, file.version())?;
        report(&mut c, "TV CMC book name suffix", format!("{book_name:?}"));
    }

    // BD linetype_scale.
    let lts = c.read_bd()?;
    report(&mut c, "BD linetype_scale", format!("{lts}"));

    // BB ltype_flags.
    let ltf = c.read_bb()?;
    report(&mut c, "BB ltype_flags", format!("{ltf}"));

    let plotstyle = c.read_bb()?;
    report(&mut c, "BB plotstyle_flag", format!("{plotstyle}"));

    let material = c.read_bb()?; // R2007+
    report(&mut c, "BB material (R2007+)", format!("{material}"));

    let shadow = c.read_rc()?; // R2007+
    report(&mut c, "RC shadow_flags (R2007+)", format!("{shadow}"));

    let vs_full = c.read_b()?; // R2010+
    let vs_face = c.read_b()?;
    let vs_edge = c.read_b()?;
    report(
        &mut c,
        "3B visualstyle full/face/edge (R2010+)",
        format!("full={vs_full} face={vs_face} edge={vs_edge}"),
    );

    let inv = c.read_bs()?;
    report(
        &mut c,
        "BS invisibility",
        format!("{inv} (0x{:04X} as i16; valid values: 0 or 1)", inv as u16),
    );

    let lw = c.read_rc()?;
    report(&mut c, "RC lineweight (R2000+)", format!("0x{lw:02X}"));

    println!();
    println!("=== remaining bits after preamble ===");
    let pos = c.position_bits();
    let total = payload.len() * 8;
    println!(
        "cursor: bit {}/{} ({} bits remain in full payload)",
        pos,
        total,
        total - pos
    );
    let data_stream_end = total - (mc as usize);
    println!(
        "data stream ends at bit {} → {} bits remain in data stream",
        data_stream_end,
        data_stream_end.saturating_sub(pos),
    );
    println!();
    println!("Expected next: LINE body (§19.4.20)");
    println!("  B zflag → RD start.x → DD end.x → RD start.y → DD end.y");
    println!("  → (if !zflag: RD start.z → DD end.z) → BT thickness → BE extrusion");
    println!();
    println!("Minimum 2D LINE body = 1 + 64 + 2 + 64 + 2 + 1 + 1 = 135 bits");
    println!("Minimum 3D LINE body = 135 + 64 + 2 = 201 bits");
    println!();
    println!("Boundary check:");
    println!(
        "  Run `cargo run --example trace_entity_boundary -- ../../samples/line_2013.dwg 0x13 1`."
    );
    println!("  For this sample, the corrected common-entity reader stops at bit 74.");

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

fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String, Box<dyn std::error::Error>> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if version.is_r2007_plus() {
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            let lo = c.read_rc()? as u16;
            let hi = c.read_rc()? as u16;
            units.push((hi << 8) | lo);
        }
        if units.last() == Some(&0) {
            units.pop();
        }
        Ok(String::from_utf16(&units)?)
    } else {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(c.read_rc()?);
        }
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}
