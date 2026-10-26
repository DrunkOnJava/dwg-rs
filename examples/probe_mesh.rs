//! Does the MESH (subdivision mesh) field list close on every release,
//! and do its values equal AutoCAD's DXF export, all the way to the
//! last field?
//!
//! # What this proves
//!
//! For every MESH record (the `MESH` custom class) of a drawing this
//! prints whether the crate's decoder closed exactly on the record's
//! data-stream boundary (ARCHITECTURE.md §7i) and the shape it read. With
//! a second argument — AutoCAD's DXF export of the same drawing — it
//! walks the `AcDbSubDMesh` group of the DXF entity whose handle (group
//! 5) matches and compares, in order:
//!
//! ```text
//! 71 version  72 blend crease  91 subdivision level
//! 92 vertex count, then 10/20/30 per vertex
//! 93 face-list size, then 90 per entry (a vertex count, then its indices)
//! 94 edge count, then 90 per endpoint
//! 95 crease count, then 140 per crease
//! 90 the trailing count after the creases
//! ```
//!
//! Every value is compared, so a record that agrees has been read in
//! full: 926 values for a 62-vertex mesh.
//!
//! ```sh
//! cargo run --release --example probe_mesh -- sample_AC1015.dwg sample_AC1015_ascii.dxf
//! ```
//!
//! Exit status is non-zero when a record errors or a value differs.

use dwg::ObjectType;
use dwg::entities::DecodedEntity;
use dwg::entities::mesh::Mesh;
use dwg::{DwgFile, entities};
use std::collections::HashMap;
use std::process::ExitCode;

/// The record's values as `(group code, value)` pairs, in the order the
/// DXF writes the `AcDbSubDMesh` group.
fn sequence(m: &Mesh) -> Vec<(u16, f64)> {
    let mut out = vec![
        (71, f64::from(m.version)),
        (72, f64::from(u8::from(m.blend_crease))),
        (91, f64::from(m.subdivision_level)),
        (92, m.vertices.len() as f64),
    ];
    for v in &m.vertices {
        out.extend([(10, v.x), (20, v.y), (30, v.z)]);
    }
    let face_list: usize = m.faces.iter().map(|f| 1 + f.len()).sum();
    out.push((93, face_list as f64));
    for f in &m.faces {
        out.push((90, f.len() as f64));
        out.extend(f.iter().map(|&i| (90, f64::from(i))));
    }
    out.push((94, m.edges.len() as f64));
    for &(a, b) in &m.edges {
        out.extend([(90, f64::from(a)), (90, f64::from(b))]);
    }
    out.push((95, m.creases.len() as f64));
    out.extend(m.creases.iter().map(|&c| (140, c)));
    // The decoder accepts only a trailing count of 0 (anything else is
    // an error), so a decoded record's count is 0.
    out.push((90, 0.0));
    out
}

/// Every `MESH` entity of an ASCII DXF, keyed by its handle, as the
/// `(code, value)` pairs after its `AcDbSubDMesh` subclass marker.
fn dxf_meshes(path: &str) -> std::io::Result<HashMap<u64, Vec<(u16, String)>>> {
    let bytes = std::fs::read(path)?;
    let text: String = bytes.iter().map(|&b| b as char).collect();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = HashMap::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i].trim() == "0" && lines[i + 1].trim() == "MESH" {
            let (mut handle, mut body, mut inside) = (None, Vec::new(), false);
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j].trim() != "0" {
                let code: u16 = lines[j].trim().parse().unwrap_or(u16::MAX);
                let value = lines[j + 1].trim().to_string();
                if code == 5 {
                    handle = u64::from_str_radix(&value, 16).ok();
                } else if code == 100 {
                    inside = value == "AcDbSubDMesh";
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

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_mesh <file.dwg> [twin.dxf]");
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
    let Some(Ok(class_map)) = file.class_map() else {
        eprintln!("{path}: no class map");
        return ExitCode::FAILURE;
    };
    let twin = match dxf.as_deref().map(dxf_meshes).transpose() {
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

    let (mut records, mut failures) = (0usize, 0usize);
    for object in &objects {
        let ObjectType::Custom(code) = object.kind else {
            continue;
        };
        let is_mesh = class_map
            .by_type_code(code)
            .is_some_and(|d| d.dxf_class_name == "MESH");
        if !is_mesh {
            continue;
        }
        records += 1;
        let handle = object.handle.value;
        match entities::decode_from_raw_with_class_map(object, version, &class_map, code) {
            DecodedEntity::Mesh(m) => {
                println!(
                    "{name} ({version}) 0x{handle:X} closes  version {} blend {} level {} \
                     vertices {} faces {} edges {} creases {}",
                    m.version,
                    m.blend_crease,
                    m.subdivision_level,
                    m.vertices.len(),
                    m.faces.len(),
                    m.edges.len(),
                    m.creases.len()
                );
                let Some(twin) = &twin else { continue };
                let Some(theirs) = twin.get(&handle) else {
                    println!("  DXF: no MESH with this handle");
                    failures += 1;
                    continue;
                };
                let ours = sequence(&m);
                let mut differ = 0usize;
                if ours.len() != theirs.len() {
                    differ += 1;
                    println!("  group count: dwg {} dxf {}", ours.len(), theirs.len());
                }
                for (k, ((oc, ov), (tc, tv))) in ours.iter().zip(theirs).enumerate() {
                    if oc != tc || !same(*ov, tv) {
                        differ += 1;
                        if differ <= 10 {
                            println!("  [{k}] dwg {oc}={ov} dxf {tc}={tv}");
                        }
                    }
                }
                println!(
                    "  DXF: {} values compared, {differ} differ",
                    ours.len().min(theirs.len())
                );
                failures += differ;
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
    println!("{name}: {records} MESH");
    if failures > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
