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

/// R2007+ STYLE records must recover their name and font file names
/// from the object's string stream (ODA v5.4.1 §19.1).
#[test]
fn sample_ac1032_recovers_modern_style_fonts() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    assert_eq!(file.version(), Version::R2018);

    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let styles = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::Style(style) => Some(style),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        styles.len() >= 5,
        "expected the whole STYLE table to decode; got {}",
        styles.len()
    );

    let standard = styles
        .iter()
        .find(|s| s.header.name == "Standard")
        .expect("Standard STYLE should decode");
    assert_eq!(standard.font_filename, "arial.ttf");
    assert!(standard.bigfont_filename.is_empty());
    assert_eq!(standard.width_factor, 1.0);
    assert_eq!(standard.oblique_angle, 0.0);
    assert!(!standard.is_shape_file());
    assert!(!standard.is_vertical());

    // A shape-file STYLE has no name; only its .shx font identifies it.
    assert!(
        styles
            .iter()
            .any(|s| s.header.name.is_empty() && s.font_filename == "ltypeshp.shx"),
        "expected the ltypeshp.shx shape-file STYLE; got {:?}",
        styles
            .iter()
            .map(|s| (&s.header.name, &s.font_filename))
            .collect::<Vec<_>>()
    );
}

/// The R2018 sample names its layers after the state they encode, so
/// the decoded `values` word can be checked against the name.
#[test]
fn sample_ac1032_recovers_modern_layer_state() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let layers = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::Layer(layer) => Some(layer),
            _ => None,
        })
        .collect::<Vec<_>>();

    let by_name = |name: &str| {
        layers
            .iter()
            .find(|l| l.header.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "missing LAYER {name:?}; decoded: {:?}",
                    layers.iter().map(|l| &l.header.name).collect::<Vec<_>>()
                )
            })
    };

    assert!(by_name("0").is_plottable());
    assert!(by_name("Layer_Freeze").is_frozen());
    assert!(!by_name("Layer1").is_frozen());
    assert!(!by_name("Layer_NoPlot").is_plottable());
    assert!(!by_name("Defpoints").is_plottable());
    assert_eq!(by_name("Layer_color_80").color_index, 80);
    // Lineweight index 9 == 0.35 mm, 11 == 0.50 mm.
    assert_eq!(by_name("Layer_lw_035").lineweight, 9);
    assert_eq!(by_name("Layer_lw_050").lineweight, 11);
    // ByLayer/default sentinel.
    assert_eq!(by_name("Layer1").lineweight, 31);
}

/// R2007+ DIMSTYLE must decode its whole field body, which is how the
/// string-stream invariant verifies it.
#[test]
fn sample_ac1032_recovers_modern_dimstyle_variables() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let styles = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::DimStyle(d) => Some(d),
            _ => None,
        })
        .collect::<Vec<_>>();

    let iso = styles
        .iter()
        .find(|d| d.header.name == "ISO-25")
        .expect("ISO-25 DIMSTYLE should decode");
    assert!(approx_eq(iso.dimscale, 1.0));
    assert!(approx_eq(iso.dimasz, 2.5));
    assert!(approx_eq(iso.dimexo, 0.625));
    assert!(approx_eq(iso.dimexe, 1.25));
    assert!(approx_eq(iso.dimtxt, 2.5));
    assert!(approx_eq(iso.dimcen, 2.5));
    assert!(approx_eq(iso.dimlfac, 1.0));
    assert_eq!(iso.dimtad, 1);

    assert!(
        styles.iter().any(|d| d.header.name == "custom_dim_style"),
        "the AcDs-binary-data DIMSTYLE should decode too"
    );
}

/// R2007+ VPORT must decode the whole viewport body, including the
/// screen rectangle and the grid/snap block.
#[test]
fn sample_ac1032_recovers_modern_active_vport() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let vport = entities
        .iter()
        .find_map(|e| match e {
            DecodedEntity::VPort(v) if v.header.name == "*Active" => Some(v),
            _ => None,
        })
        .expect("*Active VPORT should decode");
    assert!(approx_eq(vport.lower_left.x, 0.0));
    assert!(approx_eq(vport.lower_left.y, 0.0));
    assert!(approx_eq(vport.upper_right.x, 1.0));
    assert!(approx_eq(vport.upper_right.y, 1.0));
    assert!(approx_eq(vport.lens_length, 50.0));
    assert!(approx_eq(vport.view_direction.z, 1.0));
    assert!(approx_eq(vport.grid_spacing.x, vport.snap_spacing.x));
    assert!(vport.grid_spacing.x > 0.0);
}

/// R2007+ TEXT must read its string from the string stream, and its
/// height as a raw `RD`.
#[test]
fn sample_ac1032_recovers_modern_text_strings() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let texts = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::Text(t) => Some(t),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        texts.len() >= 20,
        "expected the TEXT population to decode; got {}",
        texts.len()
    );
    assert!(
        texts
            .iter()
            .any(|t| t.text == "Hello this is a single line text"),
        "expected the sample's authored TEXT string; got {:?}",
        texts.iter().map(|t| &t.text).take(5).collect::<Vec<_>>()
    );
    for t in &texts {
        assert!(is_plausible_coord(t.insertion_point.x));
        assert!(is_plausible_coord(t.insertion_point.y));
        assert!(
            t.height.is_finite() && t.height > 0.0,
            "height {}",
            t.height
        );
    }
}

/// Single-line ATTRIB / ATTDEF must recover value, tag and prompt from
/// the string stream, in that order.
#[test]
fn sample_ac1032_recovers_modern_attribute_tags() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();

    let attribs = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::Attrib(a) => Some(a),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        attribs
            .iter()
            .any(|a| a.tag == "ATTINFO" && a.text.text == "17"),
        "expected ATTINFO=17; got {:?}",
        attribs
            .iter()
            .map(|a| (&a.tag, &a.text.text))
            .collect::<Vec<_>>()
    );

    let attdefs = entities
        .iter()
        .filter_map(|e| match e {
            DecodedEntity::AttDef(a) => Some(a),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        attdefs
            .iter()
            .any(|a| a.tag == "ATTINFO" && a.prompt == "Enter number:"),
        "expected the ATTINFO prompt; got {:?}",
        attdefs
            .iter()
            .map(|a| (&a.tag, &a.prompt))
            .collect::<Vec<_>>()
    );
}

/// The three §20.4.41 ACIS records of `sample_AC1032.dwg` — the only
/// entity records in the corpus that set the R2013+ `has AcDs binary
/// data` bit, and therefore the ones that arbitrate its width (#54).
///
/// Each must decode with:
///
/// - `in_data_store` set (the SAB stream is a data-storage record, §24);
/// - a wireframe point that is real drawing geometry — the three solids
///   sit in a row at `y ≈ -220`;
/// - `num_isolines == 4`, AutoCAD's default `ISOLINES` value;
/// - a 16-byte revision GUID that is a valid RFC-4122 version-4 UUID.
///
/// Reaching a `DecodedEntity` variant at all is itself the boundary
/// assertion: the dispatcher runs these through `checked_inline`, which
/// errors unless the field list ends exactly on the record's
/// data-stream boundary. A 16-bit AcDs marker would leave every one of
/// them 16 bits short.
#[test]
fn sample_ac1032_acis_records_close_on_their_boundary() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    assert_eq!(file.version(), Version::R2018);

    let (entities, _summary) = file.decoded_entities().unwrap().unwrap();
    let mut seen = Vec::new();
    for entity in &entities {
        let (label, in_data_store, fields) = match entity {
            DecodedEntity::ThreeDSolid(solid) => {
                ("3DSOLID", solid.in_data_store, solid.tail.clone())
            }
            DecodedEntity::Region(region) => ("REGION", region.in_data_store, region.tail.clone()),
            DecodedEntity::Body(body) => ("BODY", body.in_data_store, body.tail.clone()),
            _ => continue,
        };
        let fields = fields.expect("a dispatched ACIS record carries its §20.4.41 fields");
        assert!(
            in_data_store,
            "{label}: every ACIS record of this file sets the AcDs bit"
        );
        assert_eq!(
            fields.num_isolines, 4,
            "{label}: AutoCAD writes ISOLINES = 4 by default"
        );
        assert!(fields.wireframe_present, "{label}: wireframe block present");
        assert!(fields.acis_empty_2, "{label}: second ACIS-empty bit is 1");
        let point = fields.point.expect("{label}: point present bit is set");
        assert!(
            is_plausible_coord(point.x) && is_plausible_coord(point.y),
            "{label}: wireframe point {point:?} is not plausible geometry"
        );
        assert!(
            (point.y - -220.0).abs() < 1.0,
            "{label}: the three ACIS bodies sit in a row at y ≈ -220; got {}",
            point.y
        );
        let guid = fields
            .revision_guid
            .expect("{label}: data-store records carry a revision GUID");
        assert_eq!(
            guid[6] >> 4,
            4,
            "{label}: revision GUID {guid:02X?} is not RFC-4122 version 4"
        );
        assert_eq!(
            guid[8] >> 6,
            0b10,
            "{label}: revision GUID {guid:02X?} has the wrong variant bits"
        );
        seen.push(label);
    }
    seen.sort_unstable();
    assert_eq!(
        seen,
        ["3DSOLID", "3DSOLID", "REGION"],
        "sample_AC1032.dwg holds exactly two 3DSOLIDs and one REGION"
    );
}
