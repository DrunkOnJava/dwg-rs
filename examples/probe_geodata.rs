//! Compare every decoded GEODATA record (§20.4.78) with AutoCAD's DXF
//! export of the same drawing, matched by handle.
//!
//! Each value is compared with its DXF group: object version `90`,
//! design coordinate type `70`, design point `10`/`20`/`30`, reference
//! point `11`/`21`/`31`, unit scales and units `40`, `91`, `41`, `92`, up
//! direction `210`/`220`/`230`, north direction `12`/`22`, scale
//! estimation `95` and `141`, sea level `294` and `142`, projection
//! radius `143`, the coordinate system definition (`303` chunks, then
//! `301`), the GeoRSS tag `302`, the observation tags `305`-`307`, the
//! mesh points (`93`, then `13`/`23` source and `14`/`24` destination)
//! and the mesh faces (`96`, then `97`/`98`/`99`).
//!
//! The DXF writes a newline in a string as `^J`; the probe decodes that
//! caret escape before comparing. The export prints a real to at most
//! 16 significant digits and 16 decimal places, so two reals agree
//! within 1e-12 relative or 1e-15 absolute.
//!
//! ```sh
//! cargo run --release --example probe_geodata -- <file.dwg> <twin.dxf>
//! ```
//!
//! Exits non-zero if any value differs, a GEODATA record fails to
//! decode, or none is found.

use std::collections::HashMap;
use std::process::ExitCode;

use dwg::entities::DecodedEntity;
use dwg::entities::geodata::GeoData;
use dwg::{DwgFile, ObjectType};

/// The groups of one DXF record, in file order.
type Groups = Vec<(u16, String)>;

fn read_geodata_twins(path: &str) -> HashMap<u64, Groups> {
    let bytes = std::fs::read(path).expect("read the DXF twin");
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let mut out = HashMap::new();
    let mut i = 0;
    let mut current: Option<Groups> = None;
    while i + 1 < lines.len() {
        let code: u16 = lines[i].trim().parse().expect("a DXF group code");
        let value = lines[i + 1].trim_end_matches('\r').to_string();
        i += 2;
        if code == 0 {
            if let Some(groups) = current.take() {
                let handle = groups
                    .iter()
                    .find(|(c, _)| *c == 5)
                    .and_then(|(_, v)| u64::from_str_radix(v.trim(), 16).ok());
                if let Some(h) = handle {
                    out.insert(h, groups);
                }
            }
            if value.trim() == "GEODATA" {
                current = Some(Vec::new());
            }
            continue;
        }
        if let Some(groups) = current.as_mut() {
            groups.push((code, value));
        }
    }
    out
}

fn uncaret(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '^' {
            match chars.next() {
                Some(' ') => out.push('^'),
                Some(next) => out.push(char::from((next as u8) & 0x1F)),
                None => out.push('^'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

struct Compare {
    compared: usize,
    differ: usize,
}

impl Compare {
    fn real(&mut self, what: &str, dwg: f64, dxf: Option<&str>) {
        self.compared += 1;
        let same = dxf
            .and_then(|v| v.trim().parse::<f64>().ok())
            .is_some_and(|d| {
                (d - dwg).abs() <= 1e-15 || (d - dwg).abs() <= 1e-12 * d.abs().max(dwg.abs())
            });
        if !same {
            self.differ += 1;
            println!("  {what}: decoded {dwg}, DXF {dxf:?}");
        }
    }

    fn int(&mut self, what: &str, dwg: i64, dxf: Option<&str>) {
        self.compared += 1;
        if dxf.and_then(|v| v.trim().parse::<i64>().ok()) != Some(dwg) {
            self.differ += 1;
            println!("  {what}: decoded {dwg}, DXF {dxf:?}");
        }
    }

    fn text(&mut self, what: &str, dwg: &str, dxf: &str) {
        self.compared += 1;
        if dwg != dxf {
            self.differ += 1;
            println!("  {what}: decoded {dwg:?}, DXF {dxf:?}");
        }
    }
}

/// Values of `code` in record order.
fn all(groups: &Groups, code: u16) -> Vec<&str> {
    groups
        .iter()
        .filter(|(c, _)| *c == code)
        .map(|(_, v)| v.as_str())
        .collect()
}

fn first(groups: &Groups, code: u16) -> Option<&str> {
    groups
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, v)| v.as_str())
}

fn compare(g: &GeoData, groups: &Groups) -> Compare {
    let mut cmp = Compare {
        compared: 0,
        differ: 0,
    };
    cmp.int(
        "object version (90)",
        g.object_version.into(),
        first(groups, 90),
    );
    cmp.int(
        "design coordinate type (70)",
        g.design_coordinate_type.into(),
        first(groups, 70),
    );
    for (what, p, codes) in [
        ("design point", g.design_point, [10, 20, 30]),
        ("reference point", g.reference_point, [11, 21, 31]),
        ("up direction", g.up_direction, [210, 220, 230]),
    ] {
        for (value, code) in [p.x, p.y, p.z].into_iter().zip(codes) {
            cmp.real(&format!("{what} ({code})"), value, first(groups, code));
        }
    }
    cmp.real(
        "horizontal unit scale (40)",
        g.horizontal_unit_scale,
        first(groups, 40),
    );
    cmp.int(
        "horizontal units (91)",
        g.horizontal_units.into(),
        first(groups, 91),
    );
    cmp.real(
        "vertical unit scale (41)",
        g.vertical_unit_scale,
        first(groups, 41),
    );
    cmp.int(
        "vertical units (92)",
        g.vertical_units.into(),
        first(groups, 92),
    );
    cmp.real(
        "north direction (12)",
        g.north_direction.x,
        first(groups, 12),
    );
    cmp.real(
        "north direction (22)",
        g.north_direction.y,
        first(groups, 22),
    );
    cmp.int(
        "scale estimation (95)",
        g.scale_estimation_method.into(),
        first(groups, 95),
    );
    cmp.real(
        "user scale factor (141)",
        g.user_scale_factor,
        first(groups, 141),
    );
    cmp.int(
        "sea level correction (294)",
        i64::from(g.sea_level_correction),
        first(groups, 294),
    );
    cmp.real(
        "sea level elevation (142)",
        g.sea_level_elevation,
        first(groups, 142),
    );
    cmp.real(
        "projection radius (143)",
        g.coordinate_projection_radius,
        first(groups, 143),
    );

    let mut definition: String = all(groups, 303).concat();
    definition.push_str(first(groups, 301).unwrap_or(""));
    cmp.text(
        "coordinate system definition (303/301)",
        &g.coordinate_system_definition,
        &uncaret(&definition),
    );
    cmp.text(
        "GeoRSS tag (302)",
        &g.geo_rss_tag,
        &uncaret(first(groups, 302).unwrap_or("")),
    );
    for (what, value, code) in [
        ("observation from", &g.observation_from_tag, 305),
        ("observation to", &g.observation_to_tag, 306),
        ("observation coverage", &g.observation_coverage_tag, 307),
    ] {
        cmp.text(
            &format!("{what} ({code})"),
            value,
            &uncaret(first(groups, code).unwrap_or("")),
        );
    }

    cmp.int(
        "mesh points (93)",
        g.mesh_points.len() as i64,
        first(groups, 93),
    );
    let (sx, sy, dx, dy) = (
        all(groups, 13),
        all(groups, 23),
        all(groups, 14),
        all(groups, 24),
    );
    for (i, (source, destination)) in g.mesh_points.iter().enumerate() {
        cmp.real(
            &format!("mesh point {i} source (13)"),
            source.x,
            sx.get(i).copied(),
        );
        cmp.real(
            &format!("mesh point {i} source (23)"),
            source.y,
            sy.get(i).copied(),
        );
        cmp.real(
            &format!("mesh point {i} destination (14)"),
            destination.x,
            dx.get(i).copied(),
        );
        cmp.real(
            &format!("mesh point {i} destination (24)"),
            destination.y,
            dy.get(i).copied(),
        );
    }
    cmp.int(
        "mesh faces (96)",
        g.mesh_faces.len() as i64,
        first(groups, 96),
    );
    let (f1, f2, f3) = (all(groups, 97), all(groups, 98), all(groups, 99));
    for (i, face) in g.mesh_faces.iter().enumerate() {
        for (j, column) in [&f1, &f2, &f3].into_iter().enumerate() {
            cmp.int(
                &format!("mesh face {i} index {j}"),
                face[j].into(),
                column.get(i).copied(),
            );
        }
    }
    cmp
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dwg_path, dxf_path] = args.as_slice() else {
        eprintln!("usage: probe_geodata <file.dwg> <twin.dxf>");
        return ExitCode::from(2);
    };
    let file = DwgFile::open(dwg_path).expect("open the drawing");
    let version = file.version();
    let objects = file
        .all_objects()
        .expect("walk the object stream")
        .expect("an object stream");
    let class_map = file.class_map().and_then(Result::ok);
    let twins = read_geodata_twins(dxf_path);

    let (mut records, mut failed, mut compared, mut differ) = (0, 0, 0, 0);
    for object in &objects {
        let decoded = match (class_map.as_ref(), object.kind) {
            (Some(cm), ObjectType::Custom(code)) => {
                if cm.by_type_code(code).map(|d| d.dxf_class_name.as_str()) != Some("GEODATA") {
                    continue;
                }
                dwg::entities::decode_from_raw_with_class_map(object, version, cm, code)
            }
            (_, ObjectType::GeoData) => dwg::entities::decode_from_raw(object, version),
            _ => continue,
        };
        records += 1;
        let handle = object.handle.value;
        let DecodedEntity::GeoData(g) = decoded else {
            failed += 1;
            println!("0x{handle:X}: not decoded: {decoded:?}");
            continue;
        };
        let Some(groups) = twins.get(&handle) else {
            println!(
                "0x{handle:X}: object version {}, no twin record",
                g.object_version
            );
            continue;
        };
        let cmp = compare(&g, groups);
        println!(
            "0x{handle:X}: object version {}, {} mesh points, {} faces: {} values compared, {} differ",
            g.object_version,
            g.mesh_points.len(),
            g.mesh_faces.len(),
            cmp.compared,
            cmp.differ
        );
        compared += cmp.compared;
        differ += cmp.differ;
    }

    let name = dwg_path.rsplit('/').next().unwrap_or(dwg_path);
    println!(
        "{name} ({version}): {records} GEODATA, {failed} not decoded, {compared} values compared, {differ} differ"
    );
    if records == 0 || failed > 0 || differ > 0 {
        println!("RESULT: FAIL");
        ExitCode::FAILURE
    } else {
        println!("RESULT: every compared value equals the twin");
        ExitCode::SUCCESS
    }
}
