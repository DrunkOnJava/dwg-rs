//! Integration tests that pin the invariants decoded entities must
//! satisfy on **real DWG files**, not synthetic bit streams.
//!
//! # Why this test file exists (task #97)
//!
//! The existing test suites ([`tests/dispatch_roundtrip.rs`],
//! [`tests/samples.rs`]) verify that [`dwg::entities::decode_from_raw`]:
//!
//! - does not panic on arbitrary payloads (property test)
//! - returns the variant expected for a given type code (integration)
//! - doesn't leak `Unhandled` for fixed entity codes (invariant)
//!
//! None of that verifies that the decoded **values** are correct. A
//! LINE whose endpoints decode to `(1e+225, -5e+305, 8e+183)` passes
//! every existing test — it returned `DecodedEntity::Line(...)`, no
//! error was raised, no panic occurred — but the values are garbage,
//! so the decoder is architecturally broken against real files.
//!
//! This file asserts that real R2013 samples produce **plausible**
//! decoded values:
//!
//! 1. The single LINE in `line_2013.dwg` must actually be reached by
//!    the dispatcher and returned as `DecodedEntity::Line`.
//! 2. Similarly for `circle_2013.dwg` (CIRCLE) and `arc_2013.dwg` (ARC).
//! 3. Any decoded LINE/CIRCLE/ARC in any R2013+ sample must have
//!    finite coordinates of magnitude `< 1e12` (a loose sanity band —
//!    AutoCAD's worldspace is `±1e20` but real drawings stay within
//!    millions of millimeters).
//!
//! These tests are active because the common-entity/body boundary is
//! now understood for the R2013/R2018 samples. They guard against
//! regressing the CMC-color, linetype-scale, and shadow-flag reads that
//! previously shifted the typed entity body by tens of bits.

use dwg::entities::DecodedEntity;
use dwg::{DwgFile, Version};
use std::path::PathBuf;

fn samples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p
}

fn open_if_present(name: &str) -> Option<DwgFile> {
    let p = samples_dir().join(name);
    if !p.exists() {
        eprintln!("skipping {name}: sample not present");
        return None;
    }
    Some(DwgFile::open(&p).unwrap_or_else(|e| panic!("{name} open failed: {e}")))
}

fn is_plausible_coord(v: f64) -> bool {
    v.is_finite() && v.abs() < 1e12
}

fn approx_eq(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 1e-9
}

fn plausible_lwpolyline(poly: &dwg::entities::lwpolyline::LwPolyline) -> bool {
    if poly.vertices.len() < 2 {
        return false;
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for vertex in &poly.vertices {
        if !is_plausible_coord(vertex.x) || !is_plausible_coord(vertex.y) {
            return false;
        }
        min_x = min_x.min(vertex.x);
        min_y = min_y.min(vertex.y);
        max_x = max_x.max(vertex.x);
        max_y = max_y.max(vertex.y);
    }
    (max_x - min_x).abs() > 1e-9 || (max_y - min_y).abs() > 1e-9
}

// ================================================================
// R2013 samples should yield the geometry they contain
// ================================================================

#[test]
fn r2013_line_sample_decodes_a_line() {
    let Some(file) = open_if_present("line_2013.dwg") else {
        return;
    };
    assert_eq!(file.version(), Version::R2013);

    let (entities, _summary) = file
        .decoded_entities()
        .expect("R2013 supports handle walk")
        .expect("decode succeeded");

    let lines = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::Line(line) => Some(line),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        1,
        "line_2013.dwg contains exactly one LINE entity authored \
         in AutoCAD (from nextgis/dwg_samples); the handle-driven \
         walk must reach it and the dispatcher must route type 0x13 \
         to Line."
    );
    let line = lines[0];
    assert!(line.is_2d);
    assert!(approx_eq(line.start.x, 50.0));
    assert!(approx_eq(line.start.y, 50.0));
    assert!(approx_eq(line.start.z, 0.0));
    assert!(approx_eq(line.end.x, 100.0));
    assert!(approx_eq(line.end.y, 100.0));
    assert!(approx_eq(line.end.z, 0.0));
}

#[test]
fn r2013_circle_sample_decodes_a_circle() {
    let Some(file) = open_if_present("circle_2013.dwg") else {
        return;
    };
    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let circle_count = entities
        .iter()
        .filter(|e| matches!(e, DecodedEntity::Circle(_)))
        .count();
    assert!(
        circle_count >= 1,
        "circle_2013.dwg contains one CIRCLE; decoded 0"
    );
}

#[test]
fn r2013_arc_sample_decodes_an_arc() {
    let Some(file) = open_if_present("arc_2013.dwg") else {
        return;
    };
    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let arc_count = entities
        .iter()
        .filter(|e| matches!(e, DecodedEntity::Arc(_)))
        .count();
    assert!(arc_count >= 1, "arc_2013.dwg contains one ARC; decoded 0");
}

// ================================================================
// All decoded geometry must have plausible coordinate magnitudes
// ================================================================

#[test]
fn all_decoded_geometry_has_plausible_coordinates() {
    // Pick every version that supports handle walking.
    let samples = [
        "line_2013.dwg",
        "circle_2013.dwg",
        "arc_2013.dwg",
        "sample_AC1032.dwg",
    ];

    let mut checked = 0usize;
    let mut offending: Vec<String> = Vec::new();

    for name in &samples {
        let Some(file) = open_if_present(name) else {
            continue;
        };
        let Some(Ok((entities, _))) = file.decoded_entities() else {
            continue;
        };

        for (i, e) in entities.iter().enumerate() {
            match e {
                DecodedEntity::Line(l) => {
                    checked += 1;
                    if !is_plausible_coord(l.start.x)
                        || !is_plausible_coord(l.start.y)
                        || !is_plausible_coord(l.start.z)
                        || !is_plausible_coord(l.end.x)
                        || !is_plausible_coord(l.end.y)
                        || !is_plausible_coord(l.end.z)
                    {
                        offending.push(format!(
                            "{name}[{i}] LINE start=({:.3e},{:.3e},{:.3e}) end=({:.3e},{:.3e},{:.3e})",
                            l.start.x, l.start.y, l.start.z, l.end.x, l.end.y, l.end.z
                        ));
                    }
                }
                DecodedEntity::Circle(c) => {
                    checked += 1;
                    if !is_plausible_coord(c.center.x)
                        || !is_plausible_coord(c.center.y)
                        || !is_plausible_coord(c.center.z)
                        || !is_plausible_coord(c.radius)
                        || c.radius < 0.0
                    {
                        offending.push(format!(
                            "{name}[{i}] CIRCLE center=({:.3e},{:.3e},{:.3e}) radius={:.3e}",
                            c.center.x, c.center.y, c.center.z, c.radius
                        ));
                    }
                }
                DecodedEntity::Arc(a) => {
                    checked += 1;
                    if !is_plausible_coord(a.center.x)
                        || !is_plausible_coord(a.center.y)
                        || !is_plausible_coord(a.center.z)
                        || !is_plausible_coord(a.radius)
                        || a.radius < 0.0
                    {
                        offending.push(format!(
                            "{name}[{i}] ARC center=({:.3e},{:.3e},{:.3e}) radius={:.3e}",
                            a.center.x, a.center.y, a.center.z, a.radius
                        ));
                    }
                }
                DecodedEntity::Point(p) => {
                    checked += 1;
                    if !is_plausible_coord(p.position.x)
                        || !is_plausible_coord(p.position.y)
                        || !is_plausible_coord(p.position.z)
                    {
                        offending.push(format!(
                            "{name}[{i}] POINT position=({:.3e},{:.3e},{:.3e})",
                            p.position.x, p.position.y, p.position.z
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    assert!(
        offending.is_empty(),
        "{} of {checked} decoded geometry entities have implausible \
         coordinates (|v| >= 1e12 or non-finite). This indicates a \
         bit-cursor alignment bug, not synthetic-test coverage gaps. \
         Offenders:\n{}",
        offending.len(),
        offending.join("\n")
    );
}

#[test]
fn sample_ac1032_decodes_most_line_bodies_plausibly() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    assert_eq!(file.version(), Version::R2018);

    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let lines = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::Line(line) => Some(line),
            _ => None,
        })
        .collect::<Vec<_>>();
    let nondegenerate = lines
        .iter()
        .filter(|line| {
            let dx = line.end.x - line.start.x;
            let dy = line.end.y - line.start.y;
            let dz = line.end.z - line.start.z;
            let len = (dx * dx + dy * dy + dz * dz).sqrt();
            len.is_finite() && len > 1e-9 && len < 1e9
        })
        .count();

    assert!(
        lines.len() >= 80,
        "sample_AC1032.dwg should decode the modelspace LINE population; got {}",
        lines.len()
    );
    assert!(
        nondegenerate >= 80,
        "sample_AC1032.dwg should decode at least 80 nondegenerate LINE bodies; got {nondegenerate}"
    );
}

#[test]
fn sample_ac1032_decodes_common_lwpolyline_bodies_plausibly() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    assert_eq!(file.version(), Version::R2018);

    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let polylines = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::LwPolyline(polyline) => Some(polyline),
            _ => None,
        })
        .collect::<Vec<_>>();
    let plausible = polylines
        .iter()
        .filter(|polyline| plausible_lwpolyline(polyline))
        .count();

    assert!(
        plausible >= 10,
        "sample_AC1032.dwg should decode at least 10 finite, nondegenerate \
         LWPOLYLINE bodies after the first-RD/subsequent-DD fix; decoded {} \
         LWPOLYLINEs, plausible {plausible}",
        polylines.len()
    );
}

#[test]
fn sample_ac1032_recovers_modern_block_record_names() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    assert_eq!(file.version(), Version::R2018);

    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let names = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::BlockRecord(block) => Some(block.header.name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();

    for expected in [
        "*Model_Space",
        "*Paper_Space",
        "_ArchTick",
        "MyBlock",
        "my_block",
        "my_block_v2",
        "my-dynamic-block",
    ] {
        assert!(
            names.contains(&expected),
            "missing BLOCK_HEADER name {expected:?}; decoded names: {names:?}"
        );
    }
    assert!(
        names.len() >= 20,
        "sample_AC1032.dwg should recover the modern BLOCK_HEADER string stream; got {} names: {names:?}",
        names.len()
    );
}

#[test]
fn sample_ac1032_recovers_simple_modern_ltype_names() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    assert_eq!(file.version(), Version::R2018);

    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let ltypes = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::Ltype(ltype) => Some(ltype),
            _ => None,
        })
        .collect::<Vec<_>>();
    let names = ltypes
        .iter()
        .map(|ltype| ltype.header.name.as_str())
        .collect::<Vec<_>>();

    for expected in ["ByBlock", "ByLayer", "Continuous"] {
        assert!(
            names.contains(&expected),
            "missing LTYPE name {expected:?}; decoded names: {names:?}"
        );
    }
    let continuous = ltypes
        .iter()
        .find(|ltype| ltype.header.name == "Continuous")
        .expect("Continuous LTYPE should be decoded");
    assert_eq!(continuous.description, "Solid line");
    assert_eq!(continuous.alignment, b'A');
    assert!(continuous.dashes.is_empty());
}
