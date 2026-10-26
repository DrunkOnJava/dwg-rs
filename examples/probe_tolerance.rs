//! Does the §20.4.49 TOLERANCE field list close on every release, and do
//! its values agree with AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every TOLERANCE record of a drawing this reports whether the
//! crate's decoder closed exactly on the record's data-stream boundary
//! (ARCHITECTURE.md §7i) and, with AutoCAD's DXF export of the same
//! drawing as a second argument, compares the insertion point (10/20/30),
//! x direction (11/21/31), extrusion (210/220/230) and text (1) with the
//! `AcDbFcf` group of the DXF entity whose handle (group 5) matches. A
//! group the DXF leaves out is counted as omitted, not compared against
//! an assumed default; the text's caret escapes (`^J` for a line feed)
//! are undone, and an R2007+ DXF is read as UTF-8.
//!
//! ```sh
//! cargo run --release --example probe_tolerance -- sample_AC1015.dwg sample_AC1015_ascii.dxf
//! ```
//!
//! Exit status is non-zero when a record errors or a value differs.

use dwg::entities::DecodedEntity;
use dwg::{DwgFile, ObjectType};
use std::collections::HashMap;
use std::process::ExitCode;

fn dxf_tolerances(path: &str) -> std::io::Result<HashMap<u64, Vec<(u16, String)>>> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect());
    let lines: Vec<&str> = text.lines().collect();
    let mut out = HashMap::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i].trim() == "0" && lines[i + 1].trim() == "TOLERANCE" {
            let (mut handle, mut body, mut inside) = (None, Vec::new(), false);
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j].trim() != "0" {
                let code: u16 = lines[j].trim().parse().unwrap_or(u16::MAX);
                let value = lines[j + 1].trim_end_matches('\r').to_string();
                if code == 5 {
                    handle = u64::from_str_radix(value.trim(), 16).ok();
                } else if code == 100 {
                    inside = value.trim() == "AcDbFcf";
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

/// Undo DXF's caret escapes: `^J` is a line feed, `^ ` a literal caret.
fn uncaret(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(' ') => out.push('^'),
            Some(n @ '@'..='_') => out.push(char::from(n as u8 - 0x40)),
            Some(n) => {
                out.push('^');
                out.push(n);
            }
            None => out.push('^'),
        }
    }
    out
}

fn same(ours: &str, theirs: &str) -> bool {
    match (ours.parse::<f64>(), theirs.trim().parse::<f64>()) {
        (Ok(a), Ok(b)) => (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0),
        _ => ours == theirs,
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_tolerance <file.dwg> [twin.dxf]");
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
    let twin = match dxf.as_deref().map(dxf_tolerances).transpose() {
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

    let (mut records, mut closed, mut compared, mut differ, mut omitted) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for object in objects.iter().filter(|o| o.kind == ObjectType::Tolerance) {
        records += 1;
        let handle = object.handle.value;
        let t = match dwg::entities::decode_from_raw(object, version) {
            DecodedEntity::Tolerance(t) => t,
            DecodedEntity::Error { message, .. } => {
                println!("{name} ({version}) TOLERANCE 0x{handle:X} ERROR {message}");
                continue;
            }
            other => {
                println!(
                    "{name} ({version}) TOLERANCE 0x{handle:X} decoded as {:?}",
                    other.type_code()
                );
                continue;
            }
        };
        closed += 1;
        println!(
            "{name} ({version}) TOLERANCE 0x{handle:X} closes  r14 ({}, {}, {}) text {:?}",
            t.unknown_short, t.height, t.dimgap, t.text_string
        );
        let Some(twin) = &twin else { continue };
        let Some(theirs) = twin.get(&handle) else {
            println!("  no TOLERANCE with this handle in the DXF");
            differ += 1;
            continue;
        };
        let n = |v: f64| v.to_string();
        let ours = [
            (10, n(t.insertion_point.x)),
            (20, n(t.insertion_point.y)),
            (30, n(t.insertion_point.z)),
            (11, n(t.x_axis_direction.x)),
            (21, n(t.x_axis_direction.y)),
            (31, n(t.x_axis_direction.z)),
            (210, n(t.extrusion.x)),
            (220, n(t.extrusion.y)),
            (230, n(t.extrusion.z)),
            (1, t.text_string.clone()),
        ];
        for (code, value) in ours {
            match theirs.iter().find(|(c, _)| *c == code) {
                Some((_, v)) => {
                    compared += 1;
                    let v = if code == 1 { uncaret(v) } else { v.clone() };
                    if !same(&value, &v) {
                        differ += 1;
                        println!("  group {code}: dwg {value:?} dxf {v:?}");
                    }
                }
                None => omitted += 1,
            }
        }
    }
    let mut line = format!("{name} ({version}) TOLERANCE: {records} records, {closed} close");
    if twin.is_some() {
        line.push_str(&format!(
            "; DXF: {compared} values compared, {differ} differ, {omitted} omitted by the DXF"
        ));
    }
    println!("{line}");
    if closed < records || differ > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
