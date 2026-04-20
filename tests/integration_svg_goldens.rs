//! SVG golden-file regression tests (L9-15).
//!
//! Two harnesses:
//!
//! 1. `synthetic_svg_goldens_roundtrip` — constructs a small document
//!    via the public [`SvgDoc`] API and compares against a committed
//!    golden at `tests/golden/svg/synthetic_basics.svg`. Runs everywhere
//!    the crate compiles; does not need the sample DWG corpus.
//!
//! 2. `line_2013_dwg_to_svg_golden` — opens `samples/line_2013.dwg`,
//!    renders a synthetic SVG from it, and compares against
//!    `tests/golden/svg/line_2013.svg`. Skips gracefully if either the
//!    sample or the golden is missing (so vendored builds and
//!    sample-less CI jobs still pass).
//!
//! # Updating goldens
//!
//! When the output format changes intentionally, regenerate with:
//!
//! ```text
//! UPDATE_GOLDENS=1 cargo test --test integration_svg_goldens
//! ```
//!
//! Inspect the resulting diff under `tests/golden/svg/` and commit it
//! alongside the source change. The test harness writes the new file
//! verbatim (no normalization) so the committed golden exactly matches
//! the live output.

use dwg::curve::{Curve, Path};
use dwg::entities::Point3D;
use dwg::svg::{Style, SvgDoc, SvgSpace};
use std::path::PathBuf;

/// Root of the golden-file tree. Tests read/write files under this dir.
fn goldens_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("golden");
    p.push("svg");
    p
}

/// Directory that holds the DWG sample corpus (one level up from the
/// crate root, alongside the other dwg-recon tools).
fn samples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("..");
    p.push("samples");
    p
}

/// Compare `actual` bytes against the golden file at `path`.
///
/// When `UPDATE_GOLDENS=1` is set in the environment the golden is
/// overwritten with `actual` (parent dirs are created on demand) and the
/// assertion is skipped so a `cargo test` run under that env var always
/// succeeds. Otherwise: if the golden is missing the test logs and exits
/// early (returning `false`); if present the bytes must match exactly.
fn assert_golden_eq(path: &PathBuf, actual: &str) -> bool {
    let update = std::env::var("UPDATE_GOLDENS").ok().as_deref() == Some("1");
    if update {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                panic!("failed to create golden dir {}: {e}", parent.display())
            });
        }
        std::fs::write(path, actual.as_bytes())
            .unwrap_or_else(|e| panic!("failed to write golden {}: {e}", path.display()));
        eprintln!(
            "UPDATE_GOLDENS=1 → wrote {} ({} bytes)",
            path.display(),
            actual.len()
        );
        return true;
    }
    if !path.exists() {
        eprintln!(
            "skipping golden check: {} not present (set UPDATE_GOLDENS=1 to create)",
            path.display()
        );
        return false;
    }
    let expected = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read golden {}: {e}", path.display()));
    assert_eq!(
        actual,
        expected,
        "SVG golden mismatch at {}; re-run with UPDATE_GOLDENS=1 if the \
         change is intentional.",
        path.display()
    );
    true
}

/// Build a minimal but feature-covering SVG document using only the
/// public API — no DWG file needed. Exercises model-space rendering,
/// layers, curves, a title block, and a viewport clip so the golden
/// meaningfully covers the emission surface added by L9-10..13.
fn build_synthetic_doc() -> SvgDoc {
    let mut doc = SvgDoc::new(200.0, 150.0);
    doc.begin_layer("outline");
    let style = Style {
        stroke: "#000000".to_string(),
        stroke_width: 0.5,
        fill: None,
        dashes: None,
    };
    doc.push_curve(
        &Curve::Line {
            a: Point3D::new(10.0, 10.0, 0.0),
            b: Point3D::new(100.0, 10.0, 0.0),
        },
        &style,
        Some("0x10"),
    );
    let box_pts = [
        Point3D::new(10.0, 10.0, 0.0),
        Point3D::new(100.0, 10.0, 0.0),
        Point3D::new(100.0, 60.0, 0.0),
        Point3D::new(10.0, 60.0, 0.0),
    ];
    let box_path = Path::from_polyline(&box_pts, true);
    doc.push_path(&box_path, &style, Some("0x11"));
    doc.end_layer();
    doc.push_title_block(
        Point3D::new(110.0, 10.0, 0.0),
        80.0,
        50.0,
        &[
            ("Drawing".to_string(), "TEST-001".to_string()),
            ("Rev".to_string(), "A".to_string()),
        ],
    );
    doc
}

/// Same structure as the synthetic doc but wrapped in a paper-space
/// layout, plus a viewport clip. Separate golden file so both the model
/// and paper-space emission paths are covered.
fn build_synthetic_paper_doc() -> SvgDoc {
    let mut doc = SvgDoc::new(297.0, 210.0).with_space(SvgSpace::Paper("Layout1".into()));
    doc.push_title_block(
        Point3D::new(0.0, 0.0, 0.0),
        297.0,
        210.0,
        &[
            ("Sheet".to_string(), "1 of 1".to_string()),
            ("Scale".to_string(), "1:50".to_string()),
        ],
    );
    doc.push_viewport(Point3D::new(10.0, 10.0, 0.0), 200.0, 150.0, "main");
    let style = Style::default();
    doc.push_curve(
        &Curve::Line {
            a: Point3D::new(20.0, 20.0, 0.0),
            b: Point3D::new(180.0, 140.0, 0.0),
        },
        &style,
        None,
    );
    doc.pop_clip();
    doc
}

#[test]
fn synthetic_svg_goldens_roundtrip() {
    // The synthetic harness must work without the sample corpus so
    // CI/vendored builds can still enforce the goldens contract. No
    // external inputs — only public API + deterministic coordinates.
    let doc = build_synthetic_doc();
    let svg = doc.finish();
    let mut golden = goldens_dir();
    golden.push("synthetic_basics.svg");
    let ran = assert_golden_eq(&golden, &svg);
    // Even if the golden is absent the SVG itself must be well-formed —
    // this catches the case where a contributor adds the test without
    // committing the golden file and runs `cargo test` without the
    // UPDATE_GOLDENS flag. At minimum: XML prolog, root element, and
    // closing tag must all be present.
    if !ran {
        assert!(svg.contains("<?xml"));
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }
}

#[test]
fn synthetic_paper_space_golden_roundtrip() {
    let doc = build_synthetic_paper_doc();
    let svg = doc.finish();
    let mut golden = goldens_dir();
    golden.push("synthetic_paper_space.svg");
    let ran = assert_golden_eq(&golden, &svg);
    if !ran {
        // Fallback sanity check when the golden is absent.
        assert!(svg.contains("data-layout=\"Layout1\""));
        assert!(svg.contains("<clipPath"));
        assert!(svg.contains("data-role=\"title-block-frame\""));
    }
}

#[test]
fn line_2013_dwg_to_svg_golden() {
    // This test needs both the DWG sample and the committed golden.
    // Either being absent is a skip (and *not* a failure), so builds
    // that vendor the crate without the corpus can still pass.
    let sample_path = {
        let mut p = samples_dir();
        p.push("line_2013.dwg");
        p
    };
    if !sample_path.exists() {
        eprintln!(
            "skipping line_2013 golden: {} not present",
            sample_path.display()
        );
        return;
    }
    // Open the sample so we exercise the reader path; we don't yet have
    // a full DWG→SVG pipeline wired into this crate, so render a
    // representative synthetic SVG tagged with the sample's version so
    // future per-entity renderers can substitute their output into the
    // same harness without rewriting the golden mechanics.
    let f = dwg::DwgFile::open(&sample_path).unwrap_or_else(|e| panic!("open line_2013.dwg: {e}"));
    let mut doc = SvgDoc::new(100.0, 100.0);
    doc.begin_layer(&format!("line_2013::{:?}", f.version()));
    let style = Style::default();
    doc.push_curve(
        &Curve::Line {
            a: Point3D::new(0.0, 0.0, 0.0),
            b: Point3D::new(100.0, 100.0, 0.0),
        },
        &style,
        None,
    );
    doc.end_layer();
    let svg = doc.finish();

    let mut golden = goldens_dir();
    golden.push("line_2013.svg");
    let ran = assert_golden_eq(&golden, &svg);
    if !ran {
        // Sample present + golden absent → useful diagnostic output so
        // the developer knows exactly which command regenerates it.
        eprintln!(
            "line_2013.dwg was read successfully but no golden exists; \
             run UPDATE_GOLDENS=1 cargo test --test integration_svg_goldens \
             to create {}",
            golden.display()
        );
    }
}
