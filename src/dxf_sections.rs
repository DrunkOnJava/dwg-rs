//! DXF section emitters — HEADER, TABLES, BLOCKS, ENTITIES, OBJECTS.
//!
//! Built on [`crate::dxf::DxfWriter`]. Each emitter takes a writer
//! and appends a full canonical section including its `0 SECTION` /
//! `2 <name>` / … / `0 ENDSEC` boundaries. Emitters never call
//! `finish()` — the caller composes multiple sections and finalizes
//! when the full document is assembled.
//!
//! # Coverage notes
//!
//! Each emitter accepts a neutral data struct (e.g. `HeaderVars`,
//! `[LayerTableEntry]`) rather than reading a live `DwgFile`. This
//! decouples the DXF writer from the decoder pipeline: callers can
//! synthesize DXF from hand-assembled data, from an in-memory model,
//! or from a decoded DWG, without this module needing to know which.
//!
//! # Minimal HEADER
//!
//! ```
//! use dwg::dxf::DxfWriter;
//! use dwg::dxf_sections::{write_header_section, HeaderEntry};
//!
//! let mut w = DxfWriter::new();
//! let vars = [
//!     HeaderEntry::string("$ACADVER", "AC1032"),
//!     HeaderEntry::int("$INSUNITS", 4),
//!     HeaderEntry::double("$LUPREC", 4.0),
//! ];
//! write_header_section(&mut w, &vars);
//! w.finish();
//! let dxf = w.take_output();
//! assert!(dxf.contains("SECTION"));
//! assert!(dxf.contains("HEADER"));
//! assert!(dxf.contains("$ACADVER"));
//! ```

use crate::color::aci_to_rgb;
use crate::curve::{Curve, Path};
use crate::dxf::DxfWriter;
use crate::entities::Point3D;

// ---------------------------------------------------------------------------
// HEADER section
// ---------------------------------------------------------------------------

/// One entry in the HEADER section. Each header variable has a
/// canonical `$NAME` and a typed value.
#[derive(Debug, Clone, PartialEq)]
pub enum HeaderValue {
    String(String),
    Int(i64),
    Double(f64),
    /// 3D point — emits X/Y/Z as group codes 10/20/30.
    Point(Point3D),
}

/// A single HEADER entry pairing name + value.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderEntry {
    pub name: String,
    pub value: HeaderValue,
}

impl HeaderEntry {
    pub fn string(name: impl Into<String>, value: impl Into<String>) -> Self {
        HeaderEntry {
            name: name.into(),
            value: HeaderValue::String(value.into()),
        }
    }
    pub fn int(name: impl Into<String>, value: i64) -> Self {
        HeaderEntry {
            name: name.into(),
            value: HeaderValue::Int(value),
        }
    }
    pub fn double(name: impl Into<String>, value: f64) -> Self {
        HeaderEntry {
            name: name.into(),
            value: HeaderValue::Double(value),
        }
    }
    pub fn point(name: impl Into<String>, value: Point3D) -> Self {
        HeaderEntry {
            name: name.into(),
            value: HeaderValue::Point(value),
        }
    }
}

/// Emit a full HEADER section. Each entry produces `9 $NAME`
/// followed by one or more value pairs depending on the variant.
///
/// `$ACADVER` is emitted automatically based on the writer's
/// [`crate::dxf::DxfWriter::version`] target — the magic comes from
/// [`crate::dxf::DxfVersion::acadver`]. Any `$ACADVER` entry the
/// caller supplies in `vars` is silently skipped so the emitted
/// magic always matches the writer's stored version.
pub fn write_header_section(w: &mut DxfWriter, vars: &[HeaderEntry]) {
    w.begin_section("HEADER");
    // Prepend $ACADVER from the writer's target version so the emitted
    // magic is always consistent with the rest of the file (subclass-
    // marker gating, group-code dialect, etc.).
    let acadver = w.version().acadver();
    w.write_string(9, "$ACADVER");
    w.write_string(1, acadver);
    for var in vars {
        // Skip any caller-supplied $ACADVER — we already emitted it
        // from the writer's version and refuse to duplicate.
        if var.name.eq_ignore_ascii_case("$ACADVER") {
            continue;
        }
        w.write_string(9, &var.name);
        match &var.value {
            HeaderValue::String(s) => w.write_string(1, s),
            HeaderValue::Int(n) => w.write_int(70, *n),
            HeaderValue::Double(f) => w.write_double(40, *f),
            HeaderValue::Point(p) => w.write_point(10, *p),
        }
    }
    w.end_section();
}

// ---------------------------------------------------------------------------
// TABLES section — at minimum, a LAYER table.
// ---------------------------------------------------------------------------

/// Minimal LAYER table entry. Callers build these from decoded
/// symbol-table entries or hand-synthesize for export.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerEntry {
    pub name: String,
    /// AutoCAD Color Index (0–255).
    pub aci: u8,
    /// `true` if the layer is off / frozen (AutoCAD encodes as
    /// negative ACI; we expose as an explicit bool for clarity).
    pub frozen: bool,
    /// Linetype name. "Continuous" is the default.
    pub linetype: String,
}

impl Default for LayerEntry {
    fn default() -> Self {
        LayerEntry {
            name: "0".to_string(),
            aci: 7,
            frozen: false,
            linetype: "Continuous".to_string(),
        }
    }
}

/// Emit a TABLES section containing a LAYER table with the given
/// entries. (Other tables — LTYPE, STYLE, DIMSTYLE, VPORT, VIEW,
/// UCS, APPID, BLOCK_RECORD — can be layered on by extending this
/// emitter; start with LAYER because every DXF reader requires it.)
pub fn write_tables_section(w: &mut DxfWriter, layers: &[LayerEntry]) {
    w.begin_section("TABLES");
    // LAYER table.
    w.write_string(0, "TABLE");
    w.write_string(2, "LAYER");
    w.write_int(70, layers.len() as i64);
    for layer in layers {
        w.write_string(0, "LAYER");
        w.write_string(2, &layer.name);
        // 70 → flags: 1 = frozen, 2 = frozen-by-default, 4 = locked.
        w.write_int(70, if layer.frozen { 1 } else { 0 });
        // 62 → color (negative for off layers per DXF convention).
        w.write_int(
            62,
            if layer.frozen {
                -(layer.aci as i64)
            } else {
                layer.aci as i64
            },
        );
        w.write_string(6, &layer.linetype);
    }
    w.write_string(0, "ENDTAB");
    w.end_section();
}

// ---------------------------------------------------------------------------
// ENTITIES section — emit one entity per Curve or Path.
// ---------------------------------------------------------------------------

/// A drawable entity as it'll appear in an ENTITIES section.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRecord {
    /// Optional handle (emitted as group code 5). When `None`, the
    /// writer emits no handle and the DXF reader assigns one.
    pub handle: Option<u64>,
    /// Layer name.
    pub layer: String,
    /// AutoCAD color index for this entity.
    pub aci: u8,
    /// The geometric content.
    pub geometry: EntityGeometry,
}

/// The geometry variant of an [`EntityRecord`]. Mirrors a subset of
/// [`crate::curve::Curve`] plus a compound `Path` for polyline-like
/// shapes.
#[derive(Debug, Clone, PartialEq)]
pub enum EntityGeometry {
    Line(Curve),
    Circle(Curve),
    Arc(Curve),
    Polyline(Path),
    Point(Point3D),
}

impl EntityRecord {
    /// Build a simple LINE entity record.
    pub fn line(layer: impl Into<String>, aci: u8, a: Point3D, b: Point3D) -> Self {
        EntityRecord {
            handle: None,
            layer: layer.into(),
            aci,
            geometry: EntityGeometry::Line(Curve::Line { a, b }),
        }
    }
}

/// Emit an ENTITIES section containing the given records.
///
/// Subclass markers (group code `100 AcDb*`) are only emitted when the
/// writer targets R13 or newer — `DxfVersion::R12` predates the
/// `AcDbEntity` / `AcDbLine` / `AcDbCircle` markers and rejects files
/// that contain them. See [`crate::dxf::DxfVersion::supports_subclass_markers`].
pub fn write_entities_section(w: &mut DxfWriter, entities: &[EntityRecord]) {
    let subclasses = w.version().supports_subclass_markers();
    w.begin_section("ENTITIES");
    for e in entities {
        match &e.geometry {
            EntityGeometry::Line(Curve::Line { a, b }) => {
                w.write_entity_header("LINE", e.handle);
                common_entity_header(w, &e.layer, e.aci, subclasses);
                if subclasses {
                    w.write_string(100, "AcDbLine");
                }
                w.write_point(10, *a);
                w.write_point(11, *b);
            }
            EntityGeometry::Circle(Curve::Circle { center, radius, .. }) => {
                w.write_entity_header("CIRCLE", e.handle);
                common_entity_header(w, &e.layer, e.aci, subclasses);
                if subclasses {
                    w.write_string(100, "AcDbCircle");
                }
                w.write_point(10, *center);
                w.write_double(40, *radius);
            }
            EntityGeometry::Arc(Curve::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ..
            }) => {
                w.write_entity_header("ARC", e.handle);
                common_entity_header(w, &e.layer, e.aci, subclasses);
                if subclasses {
                    w.write_string(100, "AcDbCircle");
                }
                w.write_point(10, *center);
                w.write_double(40, *radius);
                if subclasses {
                    w.write_string(100, "AcDbArc");
                }
                w.write_double(50, start_angle.to_degrees());
                w.write_double(51, end_angle.to_degrees());
            }
            EntityGeometry::Point(p) => {
                w.write_entity_header("POINT", e.handle);
                common_entity_header(w, &e.layer, e.aci, subclasses);
                if subclasses {
                    w.write_string(100, "AcDbPoint");
                }
                w.write_point(10, *p);
            }
            EntityGeometry::Polyline(path) => {
                w.write_entity_header("LWPOLYLINE", e.handle);
                common_entity_header(w, &e.layer, e.aci, subclasses);
                if subclasses {
                    w.write_string(100, "AcDbPolyline");
                }
                // Count line segments as vertices (+1 for start).
                let n = path.segments.len();
                let vertex_count = if n == 0 {
                    0
                } else {
                    n + if path.closed { 0 } else { 1 }
                };
                w.write_int(90, vertex_count as i64);
                w.write_int(70, if path.closed { 1 } else { 0 });
                // Emit each endpoint. For a polyline built from
                // from_polyline, each Line's endpoints give us the
                // vertex sequence.
                if let Some(Curve::Line { a, .. }) = path.segments.first() {
                    w.write_double(10, a.x);
                    w.write_double(20, a.y);
                }
                for seg in &path.segments {
                    if let Curve::Line { b, .. } = seg {
                        w.write_double(10, b.x);
                        w.write_double(20, b.y);
                    }
                }
            }
            // Variants where the Curve arm doesn't match (defensive —
            // shouldn't happen with EntityRecord's type discipline, but
            // avoids silent data loss).
            _ => {
                w.write_comment("unsupported entity variant skipped");
            }
        }
    }
    w.end_section();
}

/// Emit the shared 330/8/62/370 group codes for a drawable entity.
/// `330` is the block-record owner (skipped here — DXF readers
/// accept entities without an explicit owner), `8` is the layer,
/// `62` is the color, `370` is the lineweight (skipped — defaults
/// to ByLayer).
///
/// `subclasses = true` (R13+) emits the canonical `100 AcDbEntity`
/// marker; under R12 we omit it because that release predates
/// subclass tagging.
fn common_entity_header(w: &mut DxfWriter, layer: &str, aci: u8, subclasses: bool) {
    if subclasses {
        w.write_string(100, "AcDbEntity");
    }
    w.write_string(8, layer);
    w.write_int(62, aci as i64);
    // Emit the RGB triplet as a comment for diagnostic diffing —
    // DXF readers ignore this, but it makes the file easier to
    // eyeball without re-running the ACI table lookup.
    let (r, g, b) = aci_to_rgb(aci);
    w.write_comment(&format!("ACI {aci} ≈ #{r:02X}{g:02X}{b:02X}"));
}

// ---------------------------------------------------------------------------
// BLOCKS section — minimal stub that emits an empty modelspace block.
// ---------------------------------------------------------------------------

/// Emit a BLOCKS section containing the canonical `*Model_Space`
/// and `*Paper_Space` blocks. Real block content is out of scope
/// until the INSERT decoder + block expansion land (L5-05 / L4-15).
///
/// Under R12 the `100 AcDb*` subclass markers are omitted (R12 predates
/// the subclass-tagging convention).
pub fn write_blocks_section(w: &mut DxfWriter) {
    let subclasses = w.version().supports_subclass_markers();
    w.begin_section("BLOCKS");
    for (name, layout_owner) in [("*Model_Space", "0"), ("*Paper_Space", "0")] {
        w.write_string(0, "BLOCK");
        if subclasses {
            w.write_string(100, "AcDbEntity");
        }
        w.write_string(8, layout_owner);
        if subclasses {
            w.write_string(100, "AcDbBlockBegin");
        }
        w.write_string(2, name);
        w.write_int(70, 0);
        w.write_point(10, Point3D::new(0.0, 0.0, 0.0));
        w.write_string(3, name);
        w.write_string(1, "");
        w.write_string(0, "ENDBLK");
        if subclasses {
            w.write_string(100, "AcDbEntity");
        }
        w.write_string(8, layout_owner);
        if subclasses {
            w.write_string(100, "AcDbBlockEnd");
        }
    }
    w.end_section();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_section_emits_sentinels() {
        let mut w = DxfWriter::new();
        let vars = [
            HeaderEntry::string("$ACADVER", "AC1032"),
            HeaderEntry::int("$INSUNITS", 4),
            HeaderEntry::double("$LUPREC", 4.0),
        ];
        write_header_section(&mut w, &vars);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("SECTION"));
        assert!(s.contains("HEADER"));
        assert!(s.contains("$ACADVER"));
        assert!(s.contains("AC1032"));
        assert!(s.contains("$INSUNITS"));
        assert!(s.contains("ENDSEC"));
    }

    #[test]
    fn tables_section_has_layer_table() {
        let mut w = DxfWriter::new();
        let layers = [
            LayerEntry {
                name: "0".to_string(),
                aci: 7,
                frozen: false,
                linetype: "Continuous".to_string(),
            },
            LayerEntry {
                name: "Frozen".to_string(),
                aci: 3,
                frozen: true,
                linetype: "DASHED".to_string(),
            },
        ];
        write_tables_section(&mut w, &layers);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("TABLES"));
        assert!(s.contains("LAYER"));
        assert!(s.contains("Frozen"));
        // Frozen layer's ACI is emitted negative per DXF convention.
        assert!(s.contains("-3"));
        assert!(s.contains("DASHED"));
        assert!(s.contains("ENDTAB"));
    }

    #[test]
    fn entities_section_emits_line() {
        let mut w = DxfWriter::new();
        let entities = [EntityRecord::line(
            "0",
            7,
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(100.0, 50.0, 0.0),
        )];
        write_entities_section(&mut w, &entities);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("ENTITIES"));
        assert!(s.contains("LINE"));
        assert!(s.contains("AcDbLine"));
        assert!(s.contains("AcDbEntity"));
    }

    #[test]
    fn entities_section_emits_circle() {
        let mut w = DxfWriter::new();
        let entities = [EntityRecord {
            handle: None,
            layer: "0".to_string(),
            aci: 1,
            geometry: EntityGeometry::Circle(Curve::Circle {
                center: Point3D::new(10.0, 20.0, 0.0),
                radius: 5.0,
                normal: crate::entities::Vec3D {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
            }),
        }];
        write_entities_section(&mut w, &entities);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("CIRCLE"));
        assert!(s.contains("AcDbCircle"));
    }

    #[test]
    fn entities_section_emits_point_with_aci_comment() {
        let mut w = DxfWriter::new();
        let entities = [EntityRecord {
            handle: Some(0x83),
            layer: "0".to_string(),
            aci: 1,
            geometry: EntityGeometry::Point(Point3D::new(1.0, 2.0, 3.0)),
        }];
        write_entities_section(&mut w, &entities);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("POINT"));
        assert!(s.contains("AcDbPoint"));
        assert!(s.contains("#FF0000")); // ACI 1 = red, emitted as diag comment
    }

    #[test]
    fn entities_section_emits_polyline() {
        let mut w = DxfWriter::new();
        let pts = [
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(10.0, 0.0, 0.0),
            Point3D::new(10.0, 10.0, 0.0),
        ];
        let path = Path::from_polyline(&pts, true);
        let entities = [EntityRecord {
            handle: None,
            layer: "0".to_string(),
            aci: 7,
            geometry: EntityGeometry::Polyline(path),
        }];
        write_entities_section(&mut w, &entities);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("LWPOLYLINE"));
        assert!(s.contains("AcDbPolyline"));
    }

    #[test]
    fn blocks_section_emits_modelspace_and_paperspace() {
        let mut w = DxfWriter::new();
        write_blocks_section(&mut w);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("BLOCKS"));
        assert!(s.contains("*Model_Space"));
        assert!(s.contains("*Paper_Space"));
        assert!(s.contains("ENDBLK"));
    }

    #[test]
    fn header_section_emits_acadver_from_writer_version() {
        // Default writer (R2018) → AC1032.
        let mut w = DxfWriter::new();
        write_header_section(&mut w, &[]);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("$ACADVER"));
        assert!(s.contains("AC1032"));

        // R12 writer → AC1009.
        let mut w = crate::dxf::DxfWriter::with_version(crate::dxf::DxfVersion::R12);
        write_header_section(&mut w, &[]);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("$ACADVER"));
        assert!(s.contains("AC1009"));
        assert!(!s.contains("AC1032"));
    }

    #[test]
    fn header_section_skips_caller_supplied_acadver() {
        // Caller-supplied $ACADVER gets dropped in favor of the writer's
        // version (prevents mismatched magic).
        let mut w = crate::dxf::DxfWriter::with_version(crate::dxf::DxfVersion::R2000);
        write_header_section(&mut w, &[HeaderEntry::string("$ACADVER", "AC9999")]);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("AC1015")); // R2000 magic
        assert!(!s.contains("AC9999")); // caller's bogus value dropped
    }

    #[test]
    fn r12_entities_section_omits_subclass_markers() {
        let mut w = crate::dxf::DxfWriter::with_version(crate::dxf::DxfVersion::R12);
        write_entities_section(
            &mut w,
            &[EntityRecord::line(
                "0",
                7,
                Point3D::new(0.0, 0.0, 0.0),
                Point3D::new(1.0, 1.0, 0.0),
            )],
        );
        w.finish();
        let s = w.take_output();
        assert!(s.contains("LINE"));
        // R12 predates the subclass-marker convention — not a single
        // 100 AcDb* tag should appear.
        assert!(!s.contains("AcDbLine"));
        assert!(!s.contains("AcDbEntity"));
    }

    #[test]
    fn r2018_entities_section_emits_subclass_markers() {
        let mut w = crate::dxf::DxfWriter::with_version(crate::dxf::DxfVersion::R2018);
        write_entities_section(
            &mut w,
            &[EntityRecord::line(
                "0",
                7,
                Point3D::new(0.0, 0.0, 0.0),
                Point3D::new(1.0, 1.0, 0.0),
            )],
        );
        w.finish();
        let s = w.take_output();
        assert!(s.contains("AcDbEntity"));
        assert!(s.contains("AcDbLine"));
    }

    #[test]
    fn r12_blocks_section_omits_subclass_markers() {
        let mut w = crate::dxf::DxfWriter::with_version(crate::dxf::DxfVersion::R12);
        write_blocks_section(&mut w);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("*Model_Space"));
        assert!(!s.contains("AcDbBlockBegin"));
        assert!(!s.contains("AcDbBlockEnd"));
    }

    #[test]
    fn full_document_has_all_sections() {
        let mut w = DxfWriter::new();
        write_header_section(&mut w, &[HeaderEntry::string("$ACADVER", "AC1032")]);
        write_tables_section(&mut w, &[LayerEntry::default()]);
        write_blocks_section(&mut w);
        write_entities_section(
            &mut w,
            &[EntityRecord::line(
                "0",
                7,
                Point3D::new(0.0, 0.0, 0.0),
                Point3D::new(1.0, 1.0, 0.0),
            )],
        );
        w.finish();
        let s = w.take_output();
        // Section names present, EOF terminator present.
        for sec in ["HEADER", "TABLES", "BLOCKS", "ENTITIES", "ENDSEC", "EOF"] {
            assert!(s.contains(sec), "output missing section {sec}");
        }
    }

    // -------------------------------------------------------------------
    // OBJECTS section tests
    // -------------------------------------------------------------------

    #[test]
    fn objects_section_emits_dictionary_with_entries() {
        let mut w = DxfWriter::new();
        let objects = [DecodedObject::Dictionary {
            handle: 0x10,
            owner_handle: 0,
            hard_owner: true,
            entries: vec![
                ("ACAD_LAYOUT".to_string(), 0x1A),
                ("ACAD_MATERIAL".to_string(), 0x2B),
            ],
        }];
        write_objects_section(&mut w, objects);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("OBJECTS"));
        assert!(s.contains("DICTIONARY"));
        assert!(s.contains("AcDbDictionary"));
        assert!(s.contains("ACAD_LAYOUT"));
        assert!(s.contains("1A"));
    }

    #[test]
    fn objects_section_emits_xrecord_with_raw_bytes_marker() {
        let mut w = DxfWriter::new();
        let objects = [DecodedObject::XRecord {
            handle: 0x20,
            owner_handle: 0x10,
            cloning_flags: 1,
            raw_bytes_len: 128,
        }];
        write_objects_section(&mut w, objects);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("XRECORD"));
        assert!(s.contains("AcDbXrecord"));
    }

    #[test]
    fn objects_section_proxy_object_emits_suppression_comment() {
        let mut w = DxfWriter::new();
        let objects = [DecodedObject::ProxyObject {
            handle: 0x30,
            owner_handle: 0x10,
            class_id: 501,
        }];
        write_objects_section(&mut w, objects);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("ACAD_PROXY_OBJECT"));
        assert!(s.contains("opaque proxy data suppressed"));
    }

    #[test]
    fn objects_section_proxy_entity_emits_suppression_comment() {
        let mut w = DxfWriter::new();
        let objects = [DecodedObject::ProxyEntity {
            handle: 0x31,
            owner_handle: 0x10,
            class_id: 500,
        }];
        write_objects_section(&mut w, objects);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("ACAD_PROXY_ENTITY"));
        assert!(s.contains("opaque proxy data suppressed"));
    }

    #[test]
    fn objects_section_acad_group_emits_typed_fields() {
        let mut w = DxfWriter::new();
        let objects = [DecodedObject::AcadGroup {
            handle: 0x40,
            owner_handle: 0x10,
            name: "MyGroup".to_string(),
            selectable: true,
            member_handles: vec![0x50, 0x51],
        }];
        write_objects_section(&mut w, objects);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("ACAD_GROUP"));
        assert!(s.contains("AcDbGroup"));
        assert!(s.contains("MyGroup"));
        assert!(s.contains("\n50\n"));
    }

    #[test]
    fn objects_section_acad_scale_emits_ratio_fields() {
        let mut w = DxfWriter::new();
        let objects = [DecodedObject::AcadScale {
            handle: 0x60,
            owner_handle: 0x10,
            name: "1/4\" = 1'-0\"".to_string(),
            paper_units: 0.25,
            drawing_units: 12.0,
            is_unit_scale: false,
        }];
        write_objects_section(&mut w, objects);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("ACDBSCALE"));
        assert!(s.contains("AcDbScale"));
        assert!(s.contains("1/4"));
    }

    #[test]
    fn objects_section_pass_through_emits_stub_comment() {
        let mut w = DxfWriter::new();
        let objects = [DecodedObject::PassThrough {
            type_name: "ACAD_PROPERTYSET_DATA".to_string(),
            handle: 0x70,
            owner_handle: 0x10,
            subclass: None,
        }];
        write_objects_section(&mut w, objects);
        w.finish();
        let s = w.take_output();
        assert!(s.contains("ACAD_PROPERTYSET_DATA"));
        assert!(s.contains("pass-through"));
    }

    #[test]
    fn r12_objects_section_omits_subclass_markers() {
        let mut w = crate::dxf::DxfWriter::with_version(crate::dxf::DxfVersion::R12);
        let objects = [DecodedObject::Dictionary {
            handle: 0x10,
            owner_handle: 0,
            hard_owner: true,
            entries: vec![("ACAD_LAYOUT".to_string(), 0x1A)],
        }];
        write_objects_section(&mut w, objects);
        w.finish();
        let s = w.take_output();
        // The whole section is skipped under R12 — OBJECTS is an
        // R2000+ section per the DXF reference.
        assert!(!s.contains("\nOBJECTS\n"));
        assert!(!s.contains("AcDbDictionary"));
    }

    #[test]
    fn objects_section_empty_emits_section_boundaries() {
        let mut w = DxfWriter::new();
        let objects: [DecodedObject; 0] = [];
        write_objects_section(&mut w, objects);
        w.finish();
        let s = w.take_output();
        // An R2000+ writer with no objects still emits the section
        // boundaries so downstream consumers see a well-formed document.
        assert!(s.contains("\nOBJECTS\n"));
        assert!(s.contains("\nENDSEC\n"));
    }
}

// ---------------------------------------------------------------------------
// OBJECTS section — non-entity, non-table objects per DXF reference.
// ---------------------------------------------------------------------------

/// A non-entity, non-symbol-table object destined for the OBJECTS
/// section of a DXF document (DICTIONARY, XRECORD, proxy, LAYOUT,
/// ACAD_GROUP, ACAD_MLINESTYLE, …).
///
/// # Variants
///
/// The enum carries either the typed fields we can fill in from the
/// corresponding `src/objects/*.rs` decoder, or — for object types
/// we can classify via [`crate::ObjectType`] but whose body we do
/// not currently typed-decode through the public walker pipeline —
/// a [`DecodedObject::PassThrough`] stub that emits the canonical
/// `0 <TYPE>` + handle + subclass marker shape with a `999`
/// diagnostic comment.
///
/// Proxy objects (entity or object variant) always emit a
/// `999 opaque proxy data suppressed` comment: we don't have the
/// originating ARX class's schema so we can't faithfully round-trip
/// the serialised body. This mirrors the common LibreCAD / LibreDWG
/// fallback where a proxy cannot be re-synthesised without the
/// plug-in installed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DecodedObject {
    /// §19.5.19 — string-keyed handle map.
    Dictionary {
        handle: u64,
        owner_handle: u64,
        hard_owner: bool,
        entries: Vec<(String, u64)>,
    },
    /// §19.6.5 — opaque key/value storage. We preserve only the
    /// declared body length here; the raw bytes are not round-tripped
    /// because the group-code-to-value decomposition requires per-
    /// consumer schema knowledge.
    XRecord {
        handle: u64,
        owner_handle: u64,
        cloning_flags: i16,
        raw_bytes_len: usize,
    },
    /// §19.4.91 (object variant) — serialized body from an unknown
    /// ARX class. Emitted with a suppression comment.
    ProxyObject {
        handle: u64,
        owner_handle: u64,
        class_id: u32,
    },
    /// §19.4.91 (entity variant) — same shape, exposed separately so
    /// consumers can route to ACAD_PROXY_ENTITY vs ACAD_PROXY_OBJECT.
    ProxyEntity {
        handle: u64,
        owner_handle: u64,
        class_id: u32,
    },
    /// §19.6.7 — named, ordered set of entity handles.
    AcadGroup {
        handle: u64,
        owner_handle: u64,
        name: String,
        selectable: bool,
        member_handles: Vec<u64>,
    },
    /// §19.6.4 — multiline style. Emitted as the name + subclass
    /// marker + a 999 note; full line-element round-trip is deferred.
    AcadMlinestyle {
        handle: u64,
        owner_handle: u64,
        name: String,
    },
    /// §19.6.6 — per-layout plot/page configuration.
    AcadPlotSettings {
        handle: u64,
        owner_handle: u64,
        page_setup_name: String,
    },
    /// §19.6.8 — single scale-list entry (1:1, 1/4" = 1'-0", …).
    AcadScale {
        handle: u64,
        owner_handle: u64,
        name: String,
        paper_units: f64,
        drawing_units: f64,
        is_unit_scale: bool,
    },
    /// §19.6.9 — rendering material. Emitted with name + subclass
    /// marker only; full texture-sub-record round-trip is deferred.
    AcadMaterial {
        handle: u64,
        owner_handle: u64,
        name: String,
    },
    /// §19.6.10 — named display style.
    AcadVisualStyle {
        handle: u64,
        owner_handle: u64,
        description: String,
    },
    /// Pass-through stub for an object whose type we can classify
    /// (ACDBPLACEHOLDER, LAYOUT, ACAD_PROPERTYSET_DATA, etc.) but
    /// whose body we do NOT currently typed-decode through the public
    /// walker pipeline. Emits `0 <type_name>`, `5 <handle>`, `330`,
    /// optional `100 <subclass>`, and a `999 pass-through` comment.
    PassThrough {
        type_name: String,
        handle: u64,
        owner_handle: u64,
        /// Optional `AcDb<Class>` subclass marker. `None` for object
        /// types with no canonical subclass tag (e.g. DUMMY).
        subclass: Option<String>,
    },
}

/// Emit an OBJECTS section containing `objects`.
///
/// The OBJECTS section is an R2000+ concept per the AutoCAD DXF
/// reference: R12 drawings keep their dictionaries and layouts inside
/// the TABLES section or not at all. Under R12 this function emits
/// nothing — callers get a DXF document with HEADER / TABLES /
/// BLOCKS / ENTITIES only, which R12 readers expect.
///
/// Under R2000+ every object gets the canonical `0 <TYPE>` /
/// `5 <handle>` / `330 <owner>` shape. Subclass markers (`100 AcDb*`)
/// are gated on [`crate::dxf::DxfVersion::supports_subclass_markers`]
/// — consistent with the ENTITIES and BLOCKS emitters.
///
/// Proxy objects are emitted with a `999 opaque proxy data
/// suppressed` diagnostic: we don't have the originating ARX class's
/// schema so faithful round-trip is impossible without that plug-in
/// installed, and writing fake data would corrupt the drawing.
pub fn write_objects_section<I>(w: &mut DxfWriter, objects: I)
where
    I: IntoIterator<Item = DecodedObject>,
{
    // OBJECTS is an R2000+ section per the DXF reference; R12 files
    // have no equivalent. Emit nothing under R12 so the resulting
    // document stays spec-compliant for that target.
    if !w.version().supports_subclass_markers() {
        return;
    }

    let subclasses = w.version().supports_subclass_markers();
    w.begin_section("OBJECTS");
    for obj in objects {
        write_one_object(w, subclasses, &obj);
    }
    w.end_section();
}

/// Emit the canonical `0 <type>` / `5 <handle>` / `330 <owner>` /
/// optional `100 <subclass>` preamble shared by every OBJECTS entry.
fn write_object_preamble(
    w: &mut DxfWriter,
    subclasses: bool,
    object_type: &str,
    handle: u64,
    owner_handle: u64,
    subclass: Option<&str>,
) {
    w.write_string(0, object_type);
    w.write_handle(handle);
    // 330 is the owner handle (DICTIONARY, BLOCK_RECORD, etc). We
    // emit it even when the owner is 0 so the DXF reader sees a
    // well-formed reference; an explicit "0" owner is the canonical
    // encoding for "owned by the drawing root".
    w.write_string(330, &format!("{owner_handle:X}"));
    if subclasses {
        if let Some(tag) = subclass {
            w.write_string(100, tag);
        }
    }
}

fn write_one_object(w: &mut DxfWriter, subclasses: bool, obj: &DecodedObject) {
    match obj {
        DecodedObject::Dictionary {
            handle,
            owner_handle,
            hard_owner,
            entries,
        } => {
            write_object_preamble(
                w,
                subclasses,
                "DICTIONARY",
                *handle,
                *owner_handle,
                Some("AcDbDictionary"),
            );
            // 281 — hard-owner flag (1 = entries hard-owned, 0 = soft).
            w.write_int(281, if *hard_owner { 1 } else { 0 });
            for (name, value_handle) in entries {
                w.write_string(3, name);
                w.write_string(350, &format!("{value_handle:X}"));
            }
        }
        DecodedObject::XRecord {
            handle,
            owner_handle,
            cloning_flags,
            raw_bytes_len,
        } => {
            write_object_preamble(
                w,
                subclasses,
                "XRECORD",
                *handle,
                *owner_handle,
                Some("AcDbXrecord"),
            );
            // 280 — duplicate-record cloning flag (DWG's cloning_flags
            // field maps directly to DXF group 280).
            w.write_int(280, *cloning_flags as i64);
            // We do not surface the raw group-code-to-value pairs
            // here — decomposition requires the consuming application's
            // schema (see src/objects/xrecord.rs doc comment). Emit a
            // diagnostic so the suppression is visible.
            w.write_comment(&format!(
                "XRECORD body {raw_bytes_len} bytes; group-code pairs not surfaced"
            ));
        }
        DecodedObject::ProxyObject {
            handle,
            owner_handle,
            class_id,
        } => {
            write_object_preamble(
                w,
                subclasses,
                "ACAD_PROXY_OBJECT",
                *handle,
                *owner_handle,
                Some("AcDbProxyObject"),
            );
            // 91 — proxy class id (DXF reference's "Proxy entity class id").
            w.write_int(91, *class_id as i64);
            w.write_comment("opaque proxy data suppressed — originating ARX class unavailable");
        }
        DecodedObject::ProxyEntity {
            handle,
            owner_handle,
            class_id,
        } => {
            write_object_preamble(
                w,
                subclasses,
                "ACAD_PROXY_ENTITY",
                *handle,
                *owner_handle,
                Some("AcDbProxyEntity"),
            );
            w.write_int(91, *class_id as i64);
            w.write_comment("opaque proxy data suppressed — originating ARX class unavailable");
        }
        DecodedObject::AcadGroup {
            handle,
            owner_handle,
            name,
            selectable,
            member_handles,
        } => {
            write_object_preamble(
                w,
                subclasses,
                "ACAD_GROUP",
                *handle,
                *owner_handle,
                Some("AcDbGroup"),
            );
            // 300 — group description (empty by default).
            w.write_string(300, "");
            // 70 — unnamed flag (0 = named, 1 = AutoCAD-generated *A).
            w.write_int(70, 0);
            // 71 — selectable flag.
            w.write_int(71, if *selectable { 1 } else { 0 });
            // 340 — handle of each entity member.
            w.write_comment(&format!("group name: {name}"));
            for h in member_handles {
                w.write_string(340, &format!("{h:X}"));
            }
        }
        DecodedObject::AcadMlinestyle {
            handle,
            owner_handle,
            name,
        } => {
            write_object_preamble(
                w,
                subclasses,
                "ACAD_MLINESTYLE",
                *handle,
                *owner_handle,
                Some("AcDbMlineStyle"),
            );
            w.write_string(2, name);
            w.write_comment("MLINESTYLE line-element array not surfaced (pass-through)");
        }
        DecodedObject::AcadPlotSettings {
            handle,
            owner_handle,
            page_setup_name,
        } => {
            write_object_preamble(
                w,
                subclasses,
                "ACDBPLOTSETTINGS",
                *handle,
                *owner_handle,
                Some("AcDbPlotSettings"),
            );
            w.write_string(1, page_setup_name);
            w.write_comment("plot settings printer/paper/stylesheet fields not surfaced");
        }
        DecodedObject::AcadScale {
            handle,
            owner_handle,
            name,
            paper_units,
            drawing_units,
            is_unit_scale,
        } => {
            write_object_preamble(
                w,
                subclasses,
                "ACDBSCALE",
                *handle,
                *owner_handle,
                Some("AcDbScale"),
            );
            // 70 — flag (bit 0 = unit-scale).
            w.write_int(70, if *is_unit_scale { 1 } else { 0 });
            // 300 — scale name (user-facing label).
            w.write_string(300, name);
            // 140 — paper units.
            w.write_double(140, *paper_units);
            // 141 — drawing units.
            w.write_double(141, *drawing_units);
        }
        DecodedObject::AcadMaterial {
            handle,
            owner_handle,
            name,
        } => {
            write_object_preamble(
                w,
                subclasses,
                "MATERIAL",
                *handle,
                *owner_handle,
                Some("AcDbMaterial"),
            );
            w.write_string(1, name);
            w.write_comment("MATERIAL ambient/diffuse/specular/textures not surfaced");
        }
        DecodedObject::AcadVisualStyle {
            handle,
            owner_handle,
            description,
        } => {
            write_object_preamble(
                w,
                subclasses,
                "VISUALSTYLE",
                *handle,
                *owner_handle,
                Some("AcDbVisualStyle"),
            );
            w.write_string(2, description);
            w.write_comment("VISUALSTYLE face/edge/lighting fields not surfaced");
        }
        DecodedObject::PassThrough {
            type_name,
            handle,
            owner_handle,
            subclass,
        } => {
            write_object_preamble(
                w,
                subclasses,
                type_name,
                *handle,
                *owner_handle,
                subclass.as_deref(),
            );
            w.write_comment(&format!(
                "{type_name} body pass-through — typed decoder not wired through convert pipeline"
            ));
        }
    }
}
