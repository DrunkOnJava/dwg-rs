//! Does the §20.4.46 MTEXT field list close on every release, and do its
//! values agree with AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every MTEXT record of a drawing this reports whether the crate's
//! decoder closed exactly on the record's data-stream boundary
//! (ARCHITECTURE.md §7i). With a second argument — AutoCAD's DXF export
//! of the same drawing — it compares each decoded value with the
//! `AcDbMText` group of the DXF entity whose handle (group 5) matches:
//! insertion point (10/20/30), extrusion (210/220/230), x-axis direction
//! (11/21/31), reference rectangle width (41) and height (46), text height
//! (40), attachment (71), drawing direction (72), line spacing style (73)
//! and factor (44), the R2004+ background flags (90), scale (45), true
//! colour (421) and transparency (441), and the text itself, which the DXF splits into
//! 250-character `3` groups ending in a `1`, with control characters
//! caret-escaped (`^J` for a line feed). A group the DXF leaves out is
//! counted as omitted, not compared against an assumed default. A record
//! with no MTEXT of its handle in the DXF is counted, not failed.
//!
//! ```sh
//! cargo run --release --example probe_mtext -- sample_AC1018.dwg sample_AC1018_ascii.dxf
//! ```
//!
//! Prints one line per record that errors or disagrees and a summary;
//! exit status is non-zero when either happens.

use dwg::entities::DecodedEntity;
use dwg::entities::mtext::MText;
use dwg::{DwgFile, ObjectType};
use std::collections::HashMap;
use std::process::ExitCode;

/// Every `MTEXT` entity of an ASCII DXF, keyed by handle, as the
/// `(code, value)` pairs of its `AcDbMText` group.
fn dxf_mtexts(path: &str) -> std::io::Result<HashMap<u64, Vec<(u16, String)>>> {
    let bytes = std::fs::read(path)?;
    // R2007+ DXF is UTF-8; earlier releases write the drawing's ANSI
    // code page, read here byte for byte.
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect());
    let lines: Vec<&str> = text.lines().collect();
    let mut out = HashMap::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i].trim() == "0" && lines[i + 1].trim() == "MTEXT" {
            let (mut handle, mut body, mut inside) = (None, Vec::new(), false);
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j].trim() != "0" {
                let code: u16 = lines[j].trim().parse().unwrap_or(u16::MAX);
                let value = lines[j + 1].trim_end_matches('\r').to_string();
                if code == 5 {
                    handle = u64::from_str_radix(value.trim(), 16).ok();
                } else if code == 100 || code == 101 {
                    inside = value.trim() == "AcDbMText";
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

fn expected(m: &MText) -> Vec<(u16, String)> {
    let n = |v: f64| v.to_string();
    let mut out = vec![
        (10, n(m.insertion_point.x)),
        (20, n(m.insertion_point.y)),
        (30, n(m.insertion_point.z)),
        (210, n(m.extrusion.x)),
        (220, n(m.extrusion.y)),
        (230, n(m.extrusion.z)),
        (11, n(m.x_axis_direction.x)),
        (21, n(m.x_axis_direction.y)),
        (31, n(m.x_axis_direction.z)),
        (41, n(m.rect_width)),
        (40, n(m.nominal_text_height)),
        (71, m.attachment_point.to_string()),
        (72, m.drawing_direction.to_string()),
    ];
    if let Some(h) = m.rect_height {
        out.push((46, n(h)));
    }
    out
}

/// Undo DXF's caret escapes for control characters: `^J` is a line
/// feed (`'J'` minus 0x40), and `^ ` a literal caret. The twins write
/// `^J` exactly where the record holds `\n`.
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
        eprintln!("usage: probe_mtext <file.dwg> [twin.dxf]");
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
    let twin = match dxf.as_deref().map(dxf_mtexts).transpose() {
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

    let (mut records, mut closed, mut compared, mut differ, mut omitted, mut no_twin) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    for object in objects.iter().filter(|o| o.kind == ObjectType::MText) {
        records += 1;
        let handle = object.handle.value;
        let m = match dwg::entities::decode_from_raw(object, version) {
            DecodedEntity::MText(m) => m,
            DecodedEntity::Error { message, .. } => {
                println!("{name} ({version}) MTEXT 0x{handle:X} ERROR {message}");
                continue;
            }
            other => {
                println!(
                    "{name} ({version}) MTEXT 0x{handle:X} decoded as {:?}",
                    other.type_code()
                );
                continue;
            }
        };
        closed += 1;
        let Some(twin) = &twin else { continue };
        let Some(theirs) = twin.get(&handle) else {
            no_twin += 1;
            println!("{name} ({version}) MTEXT 0x{handle:X}: no MTEXT with this handle in the DXF");
            continue;
        };
        let mut check = |code: u16, ours: &str, theirs: Option<&str>| match theirs {
            Some(v) => {
                compared += 1;
                if !same(ours, v) {
                    differ += 1;
                    println!(
                        "{name} ({version}) MTEXT 0x{handle:X} {code}: dwg {ours:?} dxf {v:?}"
                    );
                }
            }
            None => omitted += 1,
        };
        let group = |code: u16| {
            theirs
                .iter()
                .find(|(c, _)| *c == code)
                .map(|(_, v)| v.as_str())
        };
        for (code, ours) in expected(&m) {
            check(code, &ours, group(code));
        }
        // The R2004+ background block.
        if version.is_r2004_plus() {
            check(90, &m.background_flags.to_string(), group(90));
            if let Some(b) = &m.background {
                check(45, &b.scale.to_string(), group(45));
                check(441, &b.transparency.to_string(), group(441));
                // A true colour (0xC2) is compared through group 421,
                // which §20.4.75 pairs with the CMC's `BL` RGB word.
                if b.color.rgb >> 24 == 0xC2 {
                    check(421, &(b.color.rgb & 0xFF_FFFF).to_string(), group(421));
                } else if b.color.rgb >> 24 == 0xC3 {
                    check(63, &(b.color.rgb & 0xFF).to_string(), group(63));
                }
            }
        }
        // R2000+ line spacing; R13/R14 do not store it.
        if version != dwg::Version::R14 {
            check(73, &m.linespace_style.to_string(), group(73));
            check(44, &m.linespace_factor.to_string(), group(44));
        }
        let text: String = theirs
            .iter()
            .filter(|(c, _)| *c == 3 || *c == 1)
            .map(|(_, v)| v.as_str())
            .collect();
        check(1, &m.text, Some(&uncaret(&text)));
    }
    let mut line = format!("{name} ({version}) MTEXT: {records} records, {closed} close");
    if twin.is_some() {
        line.push_str(&format!(
            "; DXF: {compared} values compared, {differ} differ, {omitted} omitted by the DXF, \
             {no_twin} records not in it"
        ));
    }
    println!("{line}");
    if closed < records || differ > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
