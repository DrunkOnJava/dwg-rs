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
//! # Why every test here is `#[ignore]`
//!
//! **These invariants FAIL on the current codebase.** Two known gaps
//! remain after the 0.1.0-alpha.1 dispatcher fixes:
//!
//! 1. The handle-driven object walk in
//!    [`dwg::reader::DwgFile::decoded_entities`] reaches only control
//!    objects (BlockControl, Dictionary, XRecord) and empty
//!    BLOCK/ENDBLK shells — it never reaches the user-drawn geometry
//!    inside the modelspace block. The geometry is stored at handle
//!    references chained from BLOCK_HEADER → owned entities, which
//!    this reader does not yet traverse.
//! 2. On the one sample that **does** reach typed entity decoders
//!    (`sample_AC1032.dwg`), the decoded field values are implausible
//!    — LINE endpoints with z=1e+225, POINT positions with x=1e+138
//!    — indicating the bit cursor is positioned wrong inside the
//!    object's payload. Likely causes: (a) the object-stream layout
//!    assumptions for R2018 don't match what the file actually uses,
//!    or (b) a bit-counting error earlier in the pipeline shifts
//!    every subsequent read.
//!
//! Each test is `#[ignore]`'d so `cargo test --release -- --ignored`
//! reveals the regression without failing the default `cargo test`
//! run. Remove the `#[ignore]` when the underlying architecture
//! lands and these invariants hold.

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

// ================================================================
// R2013 samples should yield the geometry they contain
// ================================================================

#[test]
#[ignore = "#97: handle walk doesn't reach modelspace geometry — file only \
            produces BLOCK/ENDBLK/control objects"]
fn r2013_line_sample_decodes_a_line() {
    let Some(file) = open_if_present("line_2013.dwg") else {
        return;
    };
    assert_eq!(file.version(), Version::R2013);

    let (entities, _summary) = file
        .decoded_entities()
        .expect("R2013 supports handle walk")
        .expect("decode succeeded");

    let line_count = entities
        .iter()
        .filter(|e| matches!(e, DecodedEntity::Line(_)))
        .count();
    assert!(
        line_count >= 1,
        "line_2013.dwg contains exactly one LINE entity authored \
         in AutoCAD (from nextgis/dwg_samples); the handle-driven \
         walk must reach it and the dispatcher must route type 0x13 \
         to Line. Currently decoded 0 LINE entities."
    );
}

#[test]
#[ignore = "#97: handle walk doesn't reach modelspace geometry"]
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
#[ignore = "#97: handle walk doesn't reach modelspace geometry"]
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
#[ignore = "#97: sample_AC1032 decodes LINE/POINT/CIRCLE but with \
            implausible coordinate magnitudes (1e+138, 1e+225, 1e+305) \
            indicating bit-cursor offset error inside object payloads"]
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
