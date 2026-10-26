//! Does the §20.4.47 LEADER field list close on every release, and do
//! its values agree with AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every LEADER record (type 45) of a drawing this prints whether
//! the crate's decoder closed exactly on the record's data-stream
//! boundary (ARCHITECTURE.md §7i) and the values it read. With a second
//! argument — AutoCAD's DXF export of the same drawing — it compares
//! them with the DXF entity whose handle (group 5) matches:
//!
//! - groups 73 (annotation type), 72 (path type), 71 (arrowhead on),
//!   74 (hook line on the x direction), 40 and 41 (box height and width)
//!   wherever the DXF writes them; a group the DXF leaves out is listed
//!   as omitted rather than compared against an assumed default;
//! - the vertices (groups 10/20/30). AutoCAD's export can hold one more
//!   vertex than the record when the leader has a hook line (group 75
//!   is 1): it inserts the hook line's start before the last point. The
//!   record's first `n - 1` points are then compared with the DXF's
//!   first `n - 1` and its last point with the DXF's last.
//!
//! From R2010 the record carries no box height or width (§7z), so 40
//! and 41 are reported as not in the record.
//!
//! ```sh
//! cargo run --release --example probe_leader -- sample_AC1015.dwg sample_AC1015_ascii.dxf
//! ```
//!
//! Exit status is non-zero when a record errors or a value differs.

use dwg::entities::DecodedEntity;
use dwg::entities::leader::Leader;
use dwg::{DwgFile, ObjectType};
use std::collections::HashMap;
use std::process::ExitCode;

/// Every `LEADER` entity of an ASCII DXF, keyed by its handle, as the
/// `(code, value)` pairs after its `AcDbLeader` subclass marker.
fn dxf_leaders(path: &str) -> std::io::Result<HashMap<u64, Vec<(u16, String)>>> {
    let bytes = std::fs::read(path)?;
    let text: String = bytes.iter().map(|&b| b as char).collect();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = HashMap::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i].trim() == "0" && lines[i + 1].trim() == "LEADER" {
            let (mut handle, mut body, mut inside) = (None, Vec::new(), false);
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j].trim() != "0" {
                let code: u16 = lines[j].trim().parse().unwrap_or(u16::MAX);
                let value = lines[j + 1].trim().to_string();
                if code == 5 {
                    handle = u64::from_str_radix(&value, 16).ok();
                } else if code == 100 {
                    inside = value == "AcDbLeader";
                } else if inside {
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

fn same(ours: f64, theirs: &str) -> bool {
    theirs
        .parse::<f64>()
        .is_ok_and(|b| (ours - b).abs() <= 1e-9 * ours.abs().max(b.abs()).max(1.0))
}

fn describe(l: &Leader) -> String {
    let mut s = format!(
        "annot {} path {} points {} origin ({}, {}, {}) endptproj ({}, {}, {}) hook_x {} arrow {}",
        l.annot_type,
        l.path_type,
        l.points.len(),
        l.origin.x,
        l.origin.y,
        l.origin.z,
        l.end_point_projection.x,
        l.end_point_projection.y,
        l.end_point_projection.z,
        u8::from(l.hookline_on_x_dir),
        u8::from(l.arrowhead_on)
    );
    match (l.box_height, l.box_width) {
        (Some(h), Some(w)) => s.push_str(&format!(" box {h} x {w}")),
        _ => s.push_str(" box (not in record)"),
    }
    if let Some(g) = l.dimgap {
        s.push_str(&format!(" dimgap {g}"));
    }
    if let Some(t) = &l.r14 {
        s.push_str(&format!(
            " arrowhead_type {} dimasz {} unknown {:?} byblock_color {} tail {:?}",
            t.arrowhead_type, t.dimasz, t.unknown, t.byblock_color, t.unknown_tail
        ));
    }
    if let Some(u) = l.unknown_r2000 {
        s.push_str(&format!(" unknown {u:?}"));
    }
    s
}

/// Compare one record with its DXF entity; returns the disagreements
/// and the number of values compared.
fn compare(l: &Leader, dxf: &[(u16, String)]) -> (Vec<String>, usize, Vec<u16>) {
    let mut bad = Vec::new();
    let mut compared = 0usize;
    let mut omitted = Vec::new();
    let group = |code: u16| dxf.iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone());
    let mut scalar = |code: u16, ours: f64| match group(code) {
        Some(v) => {
            compared += 1;
            if !same(ours, &v) {
                bad.push(format!("group {code}: dwg {ours} dxf {v}"));
            }
        }
        None => omitted.push(code),
    };
    scalar(73, f64::from(l.annot_type));
    scalar(72, f64::from(l.path_type));
    scalar(71, f64::from(u8::from(l.arrowhead_on)));
    scalar(74, f64::from(u8::from(l.hookline_on_x_dir)));
    if let (Some(h), Some(w)) = (l.box_height, l.box_width) {
        scalar(40, h);
        scalar(41, w);
    }
    let coords = |code: u16| -> Vec<String> {
        dxf.iter()
            .filter(|(c, _)| *c == code)
            .map(|(_, v)| v.clone())
            .collect()
    };
    let (xs, ys, zs) = (coords(10), coords(20), coords(30));
    let n = l.points.len();
    let hook = group(75).as_deref() == Some("1");
    let expect = if hook { n + 1 } else { n };
    if xs.len() != expect {
        bad.push(format!(
            "vertices: dwg {n}, dxf {} (hook line {})",
            xs.len(),
            u8::from(hook)
        ));
    } else {
        for (k, p) in l.points.iter().enumerate() {
            let at = if k + 1 == n { xs.len() - 1 } else { k };
            for (ours, theirs, axis) in [(p.x, &xs[at], 10), (p.y, &ys[at], 20), (p.z, &zs[at], 30)]
            {
                compared += 1;
                if !same(ours, theirs) {
                    bad.push(format!("vertex {k} group {axis}: dwg {ours} dxf {theirs}"));
                }
            }
        }
    }
    (bad, compared, omitted)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_leader <file.dwg> [twin.dxf]");
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
    let twin = match dxf.as_deref().map(dxf_leaders).transpose() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("read failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(path.clone());

    let (mut records, mut closed, mut failures) = (0usize, 0usize, 0usize);
    for object in objects.iter().filter(|o| o.kind == ObjectType::Leader) {
        records += 1;
        let handle = object.handle.value;
        match dwg::entities::decode_from_raw(object, version) {
            DecodedEntity::Leader(l) => {
                closed += 1;
                println!("{name} ({version}) 0x{handle:X} closes  {}", describe(&l));
                if let Some(twin) = &twin {
                    match twin.get(&handle) {
                        Some(theirs) => {
                            let (bad, compared, omitted) = compare(&l, theirs);
                            println!(
                                "  DXF: {compared} values compared, {} differ, omitted groups {omitted:?}",
                                bad.len()
                            );
                            for b in &bad {
                                println!("    {b}");
                            }
                            failures += bad.len();
                        }
                        None => {
                            println!("  DXF: no LEADER with this handle");
                            failures += 1;
                        }
                    }
                }
            }
            DecodedEntity::Error { message, .. } => {
                failures += 1;
                println!("{name} ({version}) 0x{handle:X} ERROR   {message}");
            }
            other => println!(
                "{name} ({version}) 0x{handle:X} type {:?}",
                other.type_code()
            ),
        }
    }
    println!("{name}: {records} LEADER, {closed} close");
    if failures > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
