//! Pins the exact decode behaviour of the committed canonical corpus
//! (`tests/fixtures/canonical/*.dwg`, task #94).
//!
//! The CI `coverage-smoke` job asserts a coarse aggregate decode ratio
//! against the same files. This test is the tight companion gate: it
//! pins the per-file entity counts AND the decoded field values, so a
//! bit-alignment regression that still "decodes" into garbage numbers
//! fails here even though the ratio would stay at 100 %.
//!
//! Regenerate the corpus with:
//!
//! ```bash
//! cargo run --release --example build_fixtures
//! ```

use dwg::DwgFile;
use dwg::Version;
use dwg::entities::DecodedEntity;
use std::path::{Path, PathBuf};

/// Every fixture in the corpus, paired with the version it encodes.
const FIXTURES: &[(&str, Version)] = &[
    ("synthetic_2004.dwg", Version::R2004),
    ("synthetic_2010.dwg", Version::R2010),
    ("synthetic_2013.dwg", Version::R2013),
    ("synthetic_2018.dwg", Version::R2018),
];

/// Number of entities `examples/build_fixtures.rs` writes into each file:
/// LINE (2D), LINE (3D), CIRCLE, ARC, POINT.
const ENTITIES_PER_FIXTURE: usize = 5;

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/canonical")
}

/// The corpus must not silently disappear — an empty directory used to
/// make the CI coverage gate vacuous (it skipped with a warning).
#[test]
fn canonical_corpus_is_not_empty() {
    let dir = corpus_dir();
    let dwgs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/fixtures/canonical must exist")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dwg"))
        .collect();
    assert_eq!(
        dwgs.len(),
        FIXTURES.len(),
        "canonical corpus should hold exactly {} .dwg fixtures, found {:?}",
        FIXTURES.len(),
        dwgs
    );
}

/// Each fixture opens, reports the version it was generated for, and
/// yields exactly `ENTITIES_PER_FIXTURE` cleanly decoded entities with
/// zero unhandled and zero errored records.
#[test]
fn every_fixture_decodes_all_entities() {
    for (name, version) in FIXTURES {
        let path = corpus_dir().join(name);
        let file = DwgFile::open(&path).unwrap_or_else(|e| panic!("{name}: open failed: {e}"));
        assert_eq!(file.version(), *version, "{name}: wrong version byte");

        let (entities, summary) = file
            .decoded_entities()
            .unwrap_or_else(|| panic!("{name}: decoded_entities returned None (no handle map)"))
            .unwrap_or_else(|e| panic!("{name}: decoded_entities failed: {e}"));

        assert_eq!(entities.len(), ENTITIES_PER_FIXTURE, "{name}: entity count");
        assert_eq!(summary.decoded, ENTITIES_PER_FIXTURE, "{name}: decoded");
        assert_eq!(summary.unhandled, 0, "{name}: unhandled");
        assert_eq!(summary.errored, 0, "{name}: errored — {:?}", summary.errors);
        assert_eq!(summary.decoded_ratio(), 1.0, "{name}: decoded_ratio");
    }
}

/// Field-level pin: the decoded geometry must match what
/// `examples/build_fixtures.rs` encoded. Catches a preamble
/// bit-alignment regression that still "succeeds" but shifts every
/// coordinate.
#[test]
fn fixture_entity_values_match_the_generator() {
    for (name, _version) in FIXTURES {
        let path = corpus_dir().join(name);
        let file = DwgFile::open(&path).unwrap_or_else(|e| panic!("{name}: open failed: {e}"));
        let (entities, _) = file
            .decoded_entities()
            .expect("handle map present")
            .expect("dispatch succeeds");

        match &entities[0] {
            DecodedEntity::Line(l) => {
                assert!(l.is_2d, "{name}: entity 0 should be the 2D LINE");
                assert_eq!((l.start.x, l.start.y, l.start.z), (0.0, 0.0, 0.0));
                assert_eq!((l.end.x, l.end.y, l.end.z), (100.0, 50.0, 0.0));
                assert_eq!(l.thickness, 0.0);
            }
            other => panic!("{name}: entity 0 is {other:?}, expected Line"),
        }
        match &entities[1] {
            DecodedEntity::Line(l) => {
                assert!(!l.is_2d, "{name}: entity 1 should be the 3D LINE");
                assert_eq!((l.start.x, l.start.y, l.start.z), (1.0, 3.0, 5.0));
                assert_eq!((l.end.x, l.end.y, l.end.z), (3.0, 7.0, 11.0));
                assert_eq!(l.thickness, 2.5);
                assert_eq!(
                    (l.extrusion.x, l.extrusion.y, l.extrusion.z),
                    (0.5, 0.25, 0.75)
                );
            }
            other => panic!("{name}: entity 1 is {other:?}, expected Line"),
        }
        match &entities[2] {
            DecodedEntity::Circle(c) => {
                assert_eq!((c.center.x, c.center.y, c.center.z), (10.0, 20.0, 0.0));
                assert_eq!(c.radius, 5.0);
            }
            other => panic!("{name}: entity 2 is {other:?}, expected Circle"),
        }
        match &entities[3] {
            DecodedEntity::Arc(a) => {
                assert_eq!((a.center.x, a.center.y, a.center.z), (-4.0, 8.0, 0.0));
                assert_eq!(a.radius, 12.5);
                assert_eq!(a.start_angle, 0.0);
                assert!((a.end_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
            }
            other => panic!("{name}: entity 3 is {other:?}, expected Arc"),
        }
        match &entities[4] {
            DecodedEntity::Point(p) => {
                assert_eq!(
                    (p.position.x, p.position.y, p.position.z),
                    (1.25, 2.5, 3.75)
                );
                assert_eq!(p.x_axis_angle, 0.0);
            }
            other => panic!("{name}: entity 4 is {other:?}, expected Point"),
        }
    }
}

/// The `AcDb:Header`, `AcDb:Handles`, and `AcDb:AcDbObjects` sections
/// must all be locatable through the assembled section map — the
/// container-layer half of the gate.
#[test]
fn fixture_sections_are_readable() {
    for (name, _version) in FIXTURES {
        let path = corpus_dir().join(name);
        let file = DwgFile::open(&path).unwrap_or_else(|e| panic!("{name}: open failed: {e}"));
        for section in ["AcDb:Header", "AcDb:Handles", "AcDb:AcDbObjects"] {
            let bytes = file
                .read_section(section)
                .unwrap_or_else(|| panic!("{name}: {section} missing"))
                .unwrap_or_else(|e| panic!("{name}: {section} unreadable: {e}"));
            assert!(!bytes.is_empty(), "{name}: {section} decoded to 0 bytes");
        }
        let map = file
            .handle_map()
            .expect("AcDb:Handles present")
            .expect("handle map parses");
        assert_eq!(map.len(), ENTITIES_PER_FIXTURE, "{name}: handle map size");
    }
}
