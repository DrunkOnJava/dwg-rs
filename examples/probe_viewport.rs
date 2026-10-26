//! Does the §20.4.38 VIEWPORT field list close, and are its values
//! AutoCAD's?
//!
//! # What this proves
//!
//! For every VIEWPORT record (type 34) of a drawing this prints whether
//! the crate's decoder closed exactly on the record's data-stream
//! boundary (the check every entity runs through, ARCHITECTURE.md §7i)
//! and the values it read, keyed by the DXF group code §20.4.38 gives
//! each field. With a second argument — AutoCAD's DXF export of the
//! same drawing — it also compares every one of those values with the
//! `AcDbViewport` group of the DXF entity whose handle (group 5)
//! matches, and prints each disagreement:
//!
//! ```text
//! sample_AC1032.dwg (AC1032 (2018)): 6 VIEWPORT, 6 close, 0 error
//!   0x240   closes  10=7.616174164671399 20=3.9999998836066784 ... 421=3355443
//! DXF sample_AC1032_ascii.dxf: 6 matched by handle, 294 values compared, 0 differ
//! ```
//!
//! Doubles are compared to 1e-9 relative; the DXF writes them with
//! 16 significant digits. An ambient light colour stored as a true
//! colour is compared through group 421, the group §20.4.75 pairs with
//! a CMC's `BL` RGB word; the index AutoCAD writes beside it in group
//! 63 is not stored in the record and is not compared.
//!
//! ```sh
//! cargo run --release --example probe_viewport -- sample_AC1032.dwg sample_AC1032_ascii.dxf
//! ```
//!
//! Exit status is non-zero when a record errors or a value differs.

use dwg::entities::DecodedEntity;
use dwg::entities::viewport::Viewport;
use dwg::{DwgFile, ObjectType};
use std::collections::HashMap;
use std::process::ExitCode;

/// `(group code, value)` pairs in the order the DXF writes them.
fn fields(v: &Viewport) -> Vec<(u16, String)> {
    let f = |x: f64| format!("{x}");
    let mut out = vec![
        (10, f(v.center.x)),
        (20, f(v.center.y)),
        (30, f(v.center.z)),
        (40, f(v.width)),
        (41, f(v.height)),
    ];
    let Some(w) = &v.view else {
        return out;
    };
    out.extend([
        (12, f(w.view_center.x)),
        (22, f(w.view_center.y)),
        (13, f(w.snap_base.x)),
        (23, f(w.snap_base.y)),
        (14, f(w.snap_spacing.x)),
        (24, f(w.snap_spacing.y)),
        (15, f(w.grid_spacing.x)),
        (25, f(w.grid_spacing.y)),
        (16, f(w.view_direction.x)),
        (26, f(w.view_direction.y)),
        (36, f(w.view_direction.z)),
        (17, f(w.view_target.x)),
        (27, f(w.view_target.y)),
        (37, f(w.view_target.z)),
        (42, f(w.lens_length)),
        (43, f(w.front_clip_z)),
        (44, f(w.back_clip_z)),
        (45, f(w.view_height)),
        (50, f(w.snap_angle)),
        (51, f(w.view_twist_angle)),
        (72, w.circle_zoom.to_string()),
        (90, w.status_flags.to_string()),
        (1, w.style_sheet.clone()),
        (281, w.render_mode.to_string()),
        (71, u8::from(w.ucs_per_viewport).to_string()),
        (74, u8::from(w.ucs_at_origin).to_string()),
        (110, f(w.ucs_origin.x)),
        (120, f(w.ucs_origin.y)),
        (130, f(w.ucs_origin.z)),
        (111, f(w.ucs_x_axis.x)),
        (121, f(w.ucs_x_axis.y)),
        (131, f(w.ucs_x_axis.z)),
        (112, f(w.ucs_y_axis.x)),
        (122, f(w.ucs_y_axis.y)),
        (132, f(w.ucs_y_axis.z)),
        (79, w.ucs_ortho_view_type.to_string()),
        (146, f(w.ucs_elevation)),
    ]);
    if let Some(mode) = w.shade_plot_mode {
        out.push((170, mode.to_string()));
    }
    if let Some(major) = w.grid_major {
        out.push((61, major.to_string()));
    }
    if let Some(l) = &w.lighting {
        out.extend([
            (292, u8::from(l.use_default_lights).to_string()),
            (282, l.default_lighting_type.to_string()),
            (141, f(l.brightness)),
            (142, f(l.contrast)),
        ]);
        // A true colour (0xC2 in the RGB word's top byte) is compared
        // through group 421, which §20.4.75 pairs with the `BL` RGB.
        // Group 63 then holds whatever index AutoCAD chose to pair with
        // it, which the record does not store, so it is not compared.
        let cmc = &l.ambient_light_color;
        if cmc.rgb >> 24 == 0xC2 {
            out.push((421, (cmc.rgb & 0xFF_FFFF).to_string()));
        } else {
            out.push((63, cmc.index.to_string()));
        }
    }
    out
}

/// Every `VIEWPORT` entity of an ASCII DXF, keyed by its handle, as the
/// `(code, value)` pairs after its `AcDbViewport` subclass marker.
fn dxf_viewports(path: &str) -> std::io::Result<HashMap<u64, Vec<(u16, String)>>> {
    let bytes = std::fs::read(path)?;
    let text: String = bytes.iter().map(|&b| b as char).collect();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = HashMap::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i].trim() == "0" && lines[i + 1].trim() == "VIEWPORT" {
            let mut handle = None;
            let mut body = Vec::new();
            let mut in_viewport = false;
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j].trim() != "0" {
                let code: u16 = lines[j].trim().parse().unwrap_or(u16::MAX);
                let value = lines[j + 1].trim().to_string();
                if code == 5 {
                    handle = u64::from_str_radix(&value, 16).ok();
                } else if code == 100 {
                    in_viewport = value == "AcDbViewport";
                } else if in_viewport {
                    body.push((code, value));
                }
                j += 2;
            }
            if let Some(h) = handle {
                out.insert(h, body);
            }
            i = j;
        } else {
            i += 2;
        }
    }
    Ok(out)
}

fn same(ours: &str, theirs: &str) -> bool {
    if ours == theirs {
        return true;
    }
    match (ours.parse::<f64>(), theirs.parse::<f64>()) {
        (Ok(a), Ok(b)) => (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0),
        _ => false,
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_viewport <file.dwg> [twin.dxf]");
        return ExitCode::FAILURE;
    };
    let dxf = args.next();
    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("open failed ({path}): {e}");
            return ExitCode::FAILURE;
        }
    };
    let version = file.version();
    let Some(Ok(objects)) = file.all_objects() else {
        eprintln!("{path}: no object-stream walk for {version}");
        return ExitCode::FAILURE;
    };

    let mut decoded = Vec::new();
    let mut errors = 0usize;
    let mut lines = Vec::new();
    for object in objects.iter().filter(|o| o.kind == ObjectType::Viewport) {
        match dwg::entities::decode_from_raw(object, version) {
            DecodedEntity::Viewport(v) => {
                let values = fields(&v);
                lines.push(format!(
                    "  0x{:<5X} closes  {}",
                    object.handle.value,
                    values
                        .iter()
                        .map(|(c, v)| format!("{c}={v}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
                decoded.push((object.handle.value, values));
            }
            DecodedEntity::Error { message, .. } => {
                errors += 1;
                lines.push(format!("  0x{:<5X} ERROR   {message}", object.handle.value));
            }
            other => lines.push(format!(
                "  0x{:<5X} {:?}",
                object.handle.value,
                other.type_code()
            )),
        }
    }
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(path.clone());
    println!(
        "{name} ({version}): {} VIEWPORT, {} close, {errors} error",
        decoded.len() + errors,
        decoded.len()
    );
    for line in &lines {
        println!("{line}");
    }

    let mut differ = 0usize;
    if let Some(dxf) = dxf {
        let twin = match dxf_viewports(&dxf) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("read failed ({dxf}): {e}");
                return ExitCode::FAILURE;
            }
        };
        let (mut matched, mut compared) = (0usize, 0usize);
        for (handle, values) in &decoded {
            let Some(theirs) = twin.get(handle) else {
                println!("  0x{handle:X}: no VIEWPORT with this handle in the DXF");
                differ += 1;
                continue;
            };
            matched += 1;
            for (code, ours) in values {
                compared += 1;
                match theirs.iter().find(|(c, _)| c == code) {
                    Some((_, v)) if same(ours, v) => {}
                    Some((_, v)) => {
                        differ += 1;
                        println!("  0x{handle:X} group {code}: dwg {ours} dxf {v}");
                    }
                    None => {
                        differ += 1;
                        println!("  0x{handle:X} group {code}: dwg {ours}, absent from the DXF");
                    }
                }
            }
        }
        let dxf_name = std::path::Path::new(&dxf)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(dxf.clone());
        println!(
            "DXF {dxf_name}: {matched} matched by handle, {compared} values compared, {differ} differ"
        );
    }
    if errors > 0 || differ > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
