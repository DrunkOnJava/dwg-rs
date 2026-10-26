//! Do the §20.4.3 TEXT, §20.4.4 ATTRIB and §20.4.5 ATTDEF field lists
//! close on every release, and do their values agree with AutoCAD's DXF
//! export?
//!
//! # What this proves
//!
//! For every TEXT, ATTRIB and ATTDEF record of a drawing this reports
//! whether the crate's decoder closed exactly on the record's data-stream
//! boundary (ARCHITECTURE.md §7i). With a second argument — AutoCAD's DXF
//! export of the same drawing — it compares each decoded value with the
//! DXF entity whose handle (group 5) matches:
//!
//! | Field | DXF |
//! |---|---|
//! | insertion point, elevation | `AcDbText` 10, 20, 30 |
//! | alignment point (when present) | `AcDbText` 11, 21 |
//! | height, width factor | `AcDbText` 40, 41 |
//! | rotation, oblique (radians in the record, degrees in DXF) | `AcDbText` 50, 51 |
//! | thickness | `AcDbText` 39 |
//! | generation, horizontal alignment | `AcDbText` 71, 72 |
//! | text value | `AcDbText` 1 |
//! | vertical alignment | `AcDbText` 73 (TEXT), `AcDbAttribute` / `AcDbAttributeDefinition` 74 |
//! | tag, flags, field length | `AcDbAttribute` / `AcDbAttributeDefinition` 2, 70, 73 |
//! | prompt | `AcDbAttributeDefinition` 3 |
//!
//! A group the DXF leaves out is not compared against an assumed default;
//! the probe counts it as omitted. Doubles are compared to 1e-9 relative,
//! angles modulo 360 degrees.
//!
//! ```sh
//! cargo run --release --example probe_text -- sample_AC1015.dwg sample_AC1015_ascii.dxf
//! ```
//!
//! Prints one line per record that errors or disagrees and a summary per
//! type; exit status is non-zero when either happens.

use dwg::entities::DecodedEntity;
use dwg::entities::text::Text;
use dwg::{DwgFile, ObjectType};
use std::collections::{BTreeMap, HashMap};
use std::process::ExitCode;

/// One DXF entity: `(subclass, how many earlier markers of that
/// subclass the entity has, code, value)` in order. TEXT writes the
/// `AcDbText` marker twice, and its vertical alignment sits after the
/// second.
type DxfEntity = Vec<(String, usize, u16, String)>;

/// AC1021 (R2007) and later DXF is UTF-8, as `src/dxf_reader.rs` reads
/// it; earlier releases are read a byte per character.
fn dxf_text(bytes: &[u8]) -> String {
    let lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').take(64).collect();
    let utf8 = lines
        .iter()
        .position(|l| l.trim_ascii() == b"$ACADVER")
        .and_then(|i| lines.get(i + 2))
        .is_some_and(|v| v.trim_ascii() >= b"AC1021".as_slice());
    if utf8 {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

fn dxf_entities(path: &str) -> std::io::Result<HashMap<u64, DxfEntity>> {
    let text = dxf_text(&std::fs::read(path)?);
    let lines: Vec<&str> = text.lines().collect();
    let mut out = HashMap::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i].trim() != "0" {
            i += 2;
            continue;
        }
        let (mut handle, mut body, mut subclass) = (None, Vec::new(), String::new());
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut nth = 0usize;
        let mut j = i + 2;
        while j + 1 < lines.len() && lines[j].trim() != "0" {
            let code: u16 = lines[j].trim().parse().unwrap_or(u16::MAX);
            let value = lines[j + 1].trim_end_matches('\r').to_string();
            match code {
                5 => handle = u64::from_str_radix(value.trim(), 16).ok(),
                // 101 opens an embedded object (the MTEXT of a
                // multi-line attribute); its groups are not the
                // attribute's own.
                100 | 101 => {
                    subclass = value.trim().to_string();
                    let count = seen.entry(subclass.clone()).or_insert(0);
                    nth = *count;
                    *count += 1;
                }
                _ => body.push((subclass.clone(), nth, code, value)),
            }
            j += 2;
        }
        if let Some(h) = handle {
            out.entry(h).or_insert(body);
        }
        i = j;
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum Kind {
    Number,
    Angle,
    Text,
}

/// `(subclass, nth occurrence of that subclass, code, kind, value)`.
type Expect = (&'static str, usize, u16, Kind, String);

fn text_fields(t: &Text, out: &mut Vec<Expect>) {
    let n = |v: f64| v.to_string();
    out.extend([
        ("AcDbText", 0, 10, Kind::Number, n(t.insertion_point.x)),
        ("AcDbText", 0, 20, Kind::Number, n(t.insertion_point.y)),
        ("AcDbText", 0, 30, Kind::Number, n(t.elevation)),
        ("AcDbText", 0, 40, Kind::Number, n(t.height)),
        ("AcDbText", 0, 41, Kind::Number, n(t.width_factor)),
        (
            "AcDbText",
            0,
            50,
            Kind::Angle,
            n(t.rotation_angle.to_degrees()),
        ),
        (
            "AcDbText",
            0,
            51,
            Kind::Angle,
            n(t.oblique_angle.to_degrees()),
        ),
        ("AcDbText", 0, 39, Kind::Number, n(t.thickness)),
        ("AcDbText", 0, 71, Kind::Number, t.generation.to_string()),
        ("AcDbText", 0, 72, Kind::Number, t.h_align.to_string()),
        ("AcDbText", 0, 1, Kind::Text, t.text.clone()),
    ]);
    if let Some(a) = t.alignment_point {
        out.push(("AcDbText", 0, 11, Kind::Number, n(a.x)));
        out.push(("AcDbText", 0, 21, Kind::Number, n(a.y)));
    }
}

fn find<'a>(entity: &'a DxfEntity, subclass: &str, nth: usize, code: u16) -> Option<&'a str> {
    entity
        .iter()
        .find(|(sub, n, c, _)| sub == subclass && *n == nth && *c == code)
        .map(|(_, _, _, v)| v.as_str())
}

fn agrees(kind: Kind, ours: &str, theirs: &str) -> bool {
    match kind {
        Kind::Text => ours == theirs,
        Kind::Number | Kind::Angle => {
            let (Ok(a), Ok(b)) = (ours.parse::<f64>(), theirs.trim().parse::<f64>()) else {
                return false;
            };
            let d = if matches!(kind, Kind::Angle) {
                let r = (a - b).rem_euclid(360.0);
                r.min(360.0 - r)
            } else {
                a - b
            };
            d.abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
        }
    }
}

#[derive(Default)]
struct Tally {
    records: usize,
    closed: usize,
    compared: usize,
    differ: usize,
    omitted: usize,
    no_twin: usize,
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_text <file.dwg> [twin.dxf]");
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
    let twin = match dxf.as_deref().map(dxf_entities).transpose() {
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

    let mut tallies: BTreeMap<&'static str, Tally> = BTreeMap::new();
    for object in &objects {
        let label = match object.kind {
            ObjectType::Text => "TEXT",
            ObjectType::Attrib => "ATTRIB",
            ObjectType::Attdef => "ATTDEF",
            _ => continue,
        };
        let tally = tallies.entry(label).or_default();
        tally.records += 1;
        let handle = object.handle.value;
        let mut expect: Vec<Expect> = Vec::new();
        match dwg::entities::decode_from_raw(object, version) {
            DecodedEntity::Text(t) => {
                text_fields(&t, &mut expect);
                expect.push(("AcDbText", 1, 73, Kind::Number, t.v_align.to_string()));
            }
            DecodedEntity::Attrib(a) => {
                text_fields(&a.text, &mut expect);
                let sub = "AcDbAttribute";
                expect.extend([
                    (sub, 0, 74, Kind::Number, a.text.v_align.to_string()),
                    (sub, 0, 2, Kind::Text, a.tag.clone()),
                    (sub, 0, 70, Kind::Number, a.flags.to_string()),
                    (sub, 0, 73, Kind::Number, a.field_length.to_string()),
                ]);
            }
            DecodedEntity::AttDef(a) => {
                text_fields(&a.text, &mut expect);
                let sub = "AcDbAttributeDefinition";
                expect.extend([
                    (sub, 0, 74, Kind::Number, a.text.v_align.to_string()),
                    (sub, 0, 3, Kind::Text, a.prompt.clone()),
                    (sub, 0, 2, Kind::Text, a.tag.clone()),
                    (sub, 0, 70, Kind::Number, a.flags.to_string()),
                    (sub, 0, 73, Kind::Number, a.field_length.to_string()),
                ]);
            }
            DecodedEntity::Error { message, .. } => {
                println!("{name} ({version}) {label} 0x{handle:X} ERROR {message}");
                continue;
            }
            other => {
                println!(
                    "{name} ({version}) {label} 0x{handle:X} decoded as {:?}",
                    other.type_code()
                );
                continue;
            }
        }
        tally.closed += 1;
        let Some(twin) = &twin else { continue };
        let Some(theirs) = twin.get(&handle) else {
            tally.no_twin += 1;
            continue;
        };
        for (sub, nth, code, kind, ours) in &expect {
            match find(theirs, sub, *nth, *code) {
                Some(v) => {
                    tally.compared += 1;
                    if !agrees(*kind, ours, v) {
                        tally.differ += 1;
                        println!(
                            "{name} ({version}) {label} 0x{handle:X} {sub} {code}: dwg {ours:?} dxf {v:?}"
                        );
                    }
                }
                None => tally.omitted += 1,
            }
        }
    }
    let mut failed = false;
    for (label, t) in &tallies {
        let mut line = format!(
            "{name} ({version}) {label}: {} records, {} close",
            t.records, t.closed
        );
        if twin.is_some() {
            line.push_str(&format!(
                "; DXF: {} values compared, {} differ, {} omitted by the DXF, {} records not in it",
                t.compared, t.differ, t.omitted, t.no_twin
            ));
        }
        println!("{line}");
        failed |= t.closed < t.records || t.differ > 0;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
