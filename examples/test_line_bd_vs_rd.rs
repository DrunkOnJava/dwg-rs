//! Test whether the LINE decoder should use BD (bit-double, tag-encoded,
//! 2-66 bits) vs RD (raw double, byte-aligned, 64 bits) for start coords.
//!
//! If the file's start coordinates are at-origin (0,0,0), BD encoding
//! takes 2 bits per coord (tag 10 = 0.0); RD encoding takes 64 bits.
//! That accounts for ~190 bits of decoder slop on a single LINE.

use dwg::DwgFile;
use dwg::bitcursor::BitCursor;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = DwgFile::open("../../samples/line_2013.dwg")?;
    let objects = file.all_objects().unwrap()?;
    let line_obj = objects.iter().find(|o| o.type_code == 0x13).unwrap();
    let payload = &line_obj.raw;
    println!("payload: {} bits", payload.len() * 8);

    // Match preamble end at bit 86 (from the prior tracer).
    // Skip header (34 bits) + preamble (52 bits) = 86 bits inline,
    // then decode the LINE body two ways and compare.
    let mut c = BitCursor::new(payload);
    skip_to_preamble_end(&mut c)?;
    let body_start = c.position_bits();
    println!("entity body starts at bit {}\n", body_start);

    // Variant 1: original line.rs encoding (RD for start coords).
    println!("--- Variant 1: RD start coords (current line.rs) ---");
    {
        let mut cc = BitCursor::new(payload);
        skip_to_preamble_end(&mut cc)?;
        match decode_line_rd(&mut cc) {
            Ok((line, end)) => println!(
                "  DECODED bit {}→{}: 2d={} start=({:.3e},{:.3e},{:.3e}) end=({:.3e},{:.3e},{:.3e}) thick={:.3e}",
                body_start,
                end,
                line.is_2d,
                line.sx,
                line.sy,
                line.sz,
                line.ex,
                line.ey,
                line.ez,
                line.thick
            ),
            Err((stage, pos, e)) => println!("  FAILED at {stage}, bit {pos}: {e}"),
        }
    }

    // Variant 2: BD for start coords (alternative spec interpretation).
    println!("\n--- Variant 2: BD start coords (alternative spec) ---");
    {
        let mut cc = BitCursor::new(payload);
        skip_to_preamble_end(&mut cc)?;
        match decode_line_bd(&mut cc) {
            Ok((line, end)) => println!(
                "  DECODED bit {}→{}: 2d={} start=({:.3e},{:.3e},{:.3e}) end=({:.3e},{:.3e},{:.3e}) thick={:.3e}",
                body_start,
                end,
                line.is_2d,
                line.sx,
                line.sy,
                line.sz,
                line.ex,
                line.ey,
                line.ez,
                line.thick
            ),
            Err((stage, pos, e)) => println!("  FAILED at {stage}, bit {pos}: {e}"),
        }
    }

    // Variant 3: BD for start AND end (delta encoding kept on end).
    println!("\n--- Variant 3: BD start + BD end as full BD (no delta) ---");
    {
        let mut cc = BitCursor::new(payload);
        skip_to_preamble_end(&mut cc)?;
        match decode_line_bd_no_delta(&mut cc) {
            Ok((line, end)) => println!(
                "  DECODED bit {}→{}: 2d={} start=({:.3e},{:.3e},{:.3e}) end=({:.3e},{:.3e},{:.3e}) thick={:.3e}",
                body_start,
                end,
                line.is_2d,
                line.sx,
                line.sy,
                line.sz,
                line.ex,
                line.ey,
                line.ez,
                line.thick
            ),
            Err((stage, pos, e)) => println!("  FAILED at {stage}, bit {pos}: {e}"),
        }
    }

    // Variant 4: BD start + delta end + INVERTED zflag interpretation.
    // If zflag=1 means 2D in spec but my decoder treats zflag=0 as 2D,
    // inverting saves 130+ bits on a flat 2D line.
    println!("\n--- Variant 4: BD + INVERTED zflag (zflag=0 → 2D) ---");
    {
        let mut cc = BitCursor::new(payload);
        skip_to_preamble_end(&mut cc)?;
        match decode_line_bd_inverted_zflag(&mut cc) {
            Ok((line, end)) => println!(
                "  DECODED bit {}→{}: 2d={} start=({:.3e},{:.3e},{:.3e}) end=({:.3e},{:.3e},{:.3e}) thick={:.3e}",
                body_start,
                end,
                line.is_2d,
                line.sx,
                line.sy,
                line.sz,
                line.ex,
                line.ey,
                line.ez,
                line.thick
            ),
            Err((stage, pos, e)) => println!("  FAILED at {stage}, bit {pos}: {e}"),
        }
    }

    // Variant 5: BD (no delta on end) + INVERTED zflag.
    println!("\n--- Variant 5: BD no-delta + INVERTED zflag ---");
    {
        let mut cc = BitCursor::new(payload);
        skip_to_preamble_end(&mut cc)?;
        match decode_line_bd_no_delta_inv_zflag(&mut cc) {
            Ok((line, end)) => println!(
                "  DECODED bit {}→{}: 2d={} start=({:.3e},{:.3e},{:.3e}) end=({:.3e},{:.3e},{:.3e}) thick={:.3e}",
                body_start,
                end,
                line.is_2d,
                line.sx,
                line.sy,
                line.sz,
                line.ex,
                line.ey,
                line.ez,
                line.thick
            ),
            Err((stage, pos, e)) => println!("  FAILED at {stage}, bit {pos}: {e}"),
        }
    }

    // Variant 6: BD + inverted zflag, NO BE (extrusion always default).
    // Spec might say BE is omitted on R2007+ entities (ext is in handle stream).
    println!("\n--- Variant 6: BD + inverted zflag + skip BE (default extrusion) ---");
    {
        let mut cc = BitCursor::new(payload);
        skip_to_preamble_end(&mut cc)?;
        match decode_line_bd_inv_zflag_no_be(&mut cc) {
            Ok((line, end)) => println!(
                "  DECODED bit {}→{}: 2d={} start=({:.3e},{:.3e},{:.3e}) end=({:.3e},{:.3e},{:.3e}) thick={:.3e}",
                body_start,
                end,
                line.is_2d,
                line.sx,
                line.sy,
                line.sz,
                line.ex,
                line.ey,
                line.ez,
                line.thick
            ),
            Err((stage, pos, e)) => println!("  FAILED at {stage}, bit {pos}: {e}"),
        }
    }

    // Variant 7: same as 6 but also drop BT thickness (always 0).
    println!("\n--- Variant 7: BD + inverted zflag + skip BT + skip BE ---");
    {
        let mut cc = BitCursor::new(payload);
        skip_to_preamble_end(&mut cc)?;
        match decode_line_bd_inv_zflag_no_bt_be(&mut cc) {
            Ok((line, end)) => println!(
                "  DECODED bit {}→{}: 2d={} start=({:.3e},{:.3e},{:.3e}) end=({:.3e},{:.3e},{:.3e}) thick={:.3e}",
                body_start,
                end,
                line.is_2d,
                line.sx,
                line.sy,
                line.sz,
                line.ex,
                line.ey,
                line.ez,
                line.thick
            ),
            Err((stage, pos, e)) => println!("  FAILED at {stage}, bit {pos}: {e}"),
        }
    }

    Ok(())
}

fn decode_line_bd_inv_zflag_no_be(
    c: &mut BitCursor<'_>,
) -> Result<(LineFields, usize), (&'static str, usize, dwg::Error)> {
    let z_is_3d = c.read_b().map_err(|e| ("zflag", c.position_bits(), e))?;
    let is_2d = !z_is_3d;
    let sx = c
        .read_bd()
        .map_err(|e| ("BD start.x", c.position_bits(), e))?;
    let ex_delta = c
        .read_bd()
        .map_err(|e| ("BD end.x", c.position_bits(), e))?;
    let sy = c
        .read_bd()
        .map_err(|e| ("BD start.y", c.position_bits(), e))?;
    let ey_delta = c
        .read_bd()
        .map_err(|e| ("BD end.y", c.position_bits(), e))?;
    let (sz, ez) = if is_2d {
        (0.0, 0.0)
    } else {
        let sz = c
            .read_bd()
            .map_err(|e| ("BD start.z", c.position_bits(), e))?;
        let ez_delta = c
            .read_bd()
            .map_err(|e| ("BD end.z", c.position_bits(), e))?;
        (sz, sz + ez_delta)
    };
    let thick = read_bt(c).map_err(|e| ("BT thickness", c.position_bits(), e))?;
    Ok((
        LineFields {
            sx,
            sy,
            sz,
            ex: sx + ex_delta,
            ey: sy + ey_delta,
            ez,
            thick,
            is_2d,
        },
        c.position_bits(),
    ))
}

fn decode_line_bd_inv_zflag_no_bt_be(
    c: &mut BitCursor<'_>,
) -> Result<(LineFields, usize), (&'static str, usize, dwg::Error)> {
    let z_is_3d = c.read_b().map_err(|e| ("zflag", c.position_bits(), e))?;
    let is_2d = !z_is_3d;
    let sx = c
        .read_bd()
        .map_err(|e| ("BD start.x", c.position_bits(), e))?;
    let ex_delta = c
        .read_bd()
        .map_err(|e| ("BD end.x", c.position_bits(), e))?;
    let sy = c
        .read_bd()
        .map_err(|e| ("BD start.y", c.position_bits(), e))?;
    let ey_delta = c
        .read_bd()
        .map_err(|e| ("BD end.y", c.position_bits(), e))?;
    let (sz, ez) = if is_2d {
        (0.0, 0.0)
    } else {
        let sz = c
            .read_bd()
            .map_err(|e| ("BD start.z", c.position_bits(), e))?;
        let ez_delta = c
            .read_bd()
            .map_err(|e| ("BD end.z", c.position_bits(), e))?;
        (sz, sz + ez_delta)
    };
    Ok((
        LineFields {
            sx,
            sy,
            sz,
            ex: sx + ex_delta,
            ey: sy + ey_delta,
            ez,
            thick: 0.0,
            is_2d,
        },
        c.position_bits(),
    ))
}

fn decode_line_bd_inverted_zflag(
    c: &mut BitCursor<'_>,
) -> Result<(LineFields, usize), (&'static str, usize, dwg::Error)> {
    let z_is_3d = c.read_b().map_err(|e| ("zflag", c.position_bits(), e))?;
    let is_2d = !z_is_3d;
    let sx = c
        .read_bd()
        .map_err(|e| ("BD start.x", c.position_bits(), e))?;
    let ex_delta = c
        .read_bd()
        .map_err(|e| ("BD end.x", c.position_bits(), e))?;
    let sy = c
        .read_bd()
        .map_err(|e| ("BD start.y", c.position_bits(), e))?;
    let ey_delta = c
        .read_bd()
        .map_err(|e| ("BD end.y", c.position_bits(), e))?;
    let (sz, ez) = if is_2d {
        (0.0, 0.0)
    } else {
        let sz = c
            .read_bd()
            .map_err(|e| ("BD start.z", c.position_bits(), e))?;
        let ez_delta = c
            .read_bd()
            .map_err(|e| ("BD end.z", c.position_bits(), e))?;
        (sz, sz + ez_delta)
    };
    let thick = read_bt(c).map_err(|e| ("BT thickness", c.position_bits(), e))?;
    let _ext = read_be(c).map_err(|e| ("BE extrusion", c.position_bits(), e))?;
    Ok((
        LineFields {
            sx,
            sy,
            sz,
            ex: sx + ex_delta,
            ey: sy + ey_delta,
            ez,
            thick,
            is_2d,
        },
        c.position_bits(),
    ))
}

fn decode_line_bd_no_delta_inv_zflag(
    c: &mut BitCursor<'_>,
) -> Result<(LineFields, usize), (&'static str, usize, dwg::Error)> {
    let z_is_3d = c.read_b().map_err(|e| ("zflag", c.position_bits(), e))?;
    let is_2d = !z_is_3d;
    let sx = c
        .read_bd()
        .map_err(|e| ("BD start.x", c.position_bits(), e))?;
    let ex = c
        .read_bd()
        .map_err(|e| ("BD end.x", c.position_bits(), e))?;
    let sy = c
        .read_bd()
        .map_err(|e| ("BD start.y", c.position_bits(), e))?;
    let ey = c
        .read_bd()
        .map_err(|e| ("BD end.y", c.position_bits(), e))?;
    let (sz, ez) = if is_2d {
        (0.0, 0.0)
    } else {
        let sz = c
            .read_bd()
            .map_err(|e| ("BD start.z", c.position_bits(), e))?;
        let ez = c
            .read_bd()
            .map_err(|e| ("BD end.z", c.position_bits(), e))?;
        (sz, ez)
    };
    let thick = read_bt(c).map_err(|e| ("BT thickness", c.position_bits(), e))?;
    let _ext = read_be(c).map_err(|e| ("BE extrusion", c.position_bits(), e))?;
    Ok((
        LineFields {
            sx,
            sy,
            sz,
            ex,
            ey,
            ez,
            thick,
            is_2d,
        },
        c.position_bits(),
    ))
}

#[derive(Debug)]
struct LineFields {
    sx: f64,
    sy: f64,
    sz: f64,
    ex: f64,
    ey: f64,
    ez: f64,
    thick: f64,
    is_2d: bool,
}

fn decode_line_rd(
    c: &mut BitCursor<'_>,
) -> Result<(LineFields, usize), (&'static str, usize, dwg::Error)> {
    let zflag = c.read_b().map_err(|e| ("zflag", c.position_bits(), e))?;
    let sx = c
        .read_rd()
        .map_err(|e| ("RD start.x", c.position_bits(), e))?;
    let ex_delta = c
        .read_bd()
        .map_err(|e| ("BD end.x", c.position_bits(), e))?;
    let sy = c
        .read_rd()
        .map_err(|e| ("RD start.y", c.position_bits(), e))?;
    let ey_delta = c
        .read_bd()
        .map_err(|e| ("BD end.y", c.position_bits(), e))?;
    let (sz, ez) = if zflag {
        (0.0, 0.0)
    } else {
        let sz = c
            .read_rd()
            .map_err(|e| ("RD start.z", c.position_bits(), e))?;
        let ez_delta = c
            .read_bd()
            .map_err(|e| ("BD end.z", c.position_bits(), e))?;
        (sz, sz + ez_delta)
    };
    let thick = read_bt(c).map_err(|e| ("BT thickness", c.position_bits(), e))?;
    let _ext = read_be(c).map_err(|e| ("BE extrusion", c.position_bits(), e))?;
    Ok((
        LineFields {
            sx,
            sy,
            sz,
            ex: sx + ex_delta,
            ey: sy + ey_delta,
            ez,
            thick,
            is_2d: zflag,
        },
        c.position_bits(),
    ))
}

fn decode_line_bd(
    c: &mut BitCursor<'_>,
) -> Result<(LineFields, usize), (&'static str, usize, dwg::Error)> {
    let zflag = c.read_b().map_err(|e| ("zflag", c.position_bits(), e))?;
    let sx = c
        .read_bd()
        .map_err(|e| ("BD start.x", c.position_bits(), e))?;
    let ex_delta = c
        .read_bd()
        .map_err(|e| ("BD end.x", c.position_bits(), e))?;
    let sy = c
        .read_bd()
        .map_err(|e| ("BD start.y", c.position_bits(), e))?;
    let ey_delta = c
        .read_bd()
        .map_err(|e| ("BD end.y", c.position_bits(), e))?;
    let (sz, ez) = if zflag {
        (0.0, 0.0)
    } else {
        let sz = c
            .read_bd()
            .map_err(|e| ("BD start.z", c.position_bits(), e))?;
        let ez_delta = c
            .read_bd()
            .map_err(|e| ("BD end.z", c.position_bits(), e))?;
        (sz, sz + ez_delta)
    };
    let thick = read_bt(c).map_err(|e| ("BT thickness", c.position_bits(), e))?;
    let _ext = read_be(c).map_err(|e| ("BE extrusion", c.position_bits(), e))?;
    Ok((
        LineFields {
            sx,
            sy,
            sz,
            ex: sx + ex_delta,
            ey: sy + ey_delta,
            ez,
            thick,
            is_2d: zflag,
        },
        c.position_bits(),
    ))
}

fn decode_line_bd_no_delta(
    c: &mut BitCursor<'_>,
) -> Result<(LineFields, usize), (&'static str, usize, dwg::Error)> {
    let zflag = c.read_b().map_err(|e| ("zflag", c.position_bits(), e))?;
    let sx = c
        .read_bd()
        .map_err(|e| ("BD start.x", c.position_bits(), e))?;
    let ex = c
        .read_bd()
        .map_err(|e| ("BD end.x", c.position_bits(), e))?;
    let sy = c
        .read_bd()
        .map_err(|e| ("BD start.y", c.position_bits(), e))?;
    let ey = c
        .read_bd()
        .map_err(|e| ("BD end.y", c.position_bits(), e))?;
    let (sz, ez) = if zflag {
        (0.0, 0.0)
    } else {
        let sz = c
            .read_bd()
            .map_err(|e| ("BD start.z", c.position_bits(), e))?;
        let ez = c
            .read_bd()
            .map_err(|e| ("BD end.z", c.position_bits(), e))?;
        (sz, ez)
    };
    let thick = read_bt(c).map_err(|e| ("BT thickness", c.position_bits(), e))?;
    let _ext = read_be(c).map_err(|e| ("BE extrusion", c.position_bits(), e))?;
    Ok((
        LineFields {
            sx,
            sy,
            sz,
            ex,
            ey,
            ez,
            thick,
            is_2d: zflag,
        },
        c.position_bits(),
    ))
}

fn read_bt(c: &mut BitCursor<'_>) -> Result<f64, dwg::Error> {
    if c.read_b()? { Ok(0.0) } else { c.read_bd() }
}

fn read_be(c: &mut BitCursor<'_>) -> Result<(f64, f64, f64), dwg::Error> {
    if c.read_b()? {
        Ok((0.0, 0.0, 1.0))
    } else {
        let x = c.read_bd()?;
        let y = c.read_bd()?;
        let z = c.read_bd()?;
        Ok((x, y, z))
    }
}

fn skip_to_preamble_end(c: &mut BitCursor<'_>) -> Result<(), Box<dyn std::error::Error>> {
    // header (34 bits): MC + BB + RC + Handle.
    let _ = read_mc_unsigned(c)?;
    let tag = c.read_bb()?;
    if tag == 0 || tag == 1 {
        let _ = c.read_rc()?;
    } else {
        let _ = c.read_rc()?;
        let _ = c.read_rc()?;
    }
    let _ = c.read_handle()?;
    // preamble (52 bits matching prior tracer):
    let _ = c.read_bs_u()?; // XDATA terminator
    let _ = c.read_b()?; // graphics
    let _ = c.read_bb()?; // entmode
    let _ = c.read_bl()?; // num_reactors
    let _ = c.read_b()?; // no_xdict
    let _ = c.read_b()?; // binary_chain
    let _ = c.read_b()?; // is_on_layer
    let _ = c.read_b()?; // non_fixed_ltype
    let _ = c.read_bb()?; // plotstyle
    let _ = c.read_bb()?; // material (R2007+)
    let _ = c.read_rc()?; // shadow (R2007+)
    let _ = c.read_b()?; // visualstyle full (R2010+)
    let _ = c.read_b()?; // visualstyle face
    let _ = c.read_b()?; // visualstyle edge
    let _ = c.read_bs()?; // invisibility
    let _ = c.read_rc()?; // lineweight
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
