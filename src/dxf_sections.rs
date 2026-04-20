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
pub fn write_header_section(w: &mut DxfWriter, vars: &[HeaderEntry]) {
    w.begin_section("HEADER");
    for var in vars {
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
        w.write_int(62, if layer.frozen { -(layer.aci as i64) } else { layer.aci as i64 });
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
pub fn write_entities_section(w: &mut DxfWriter, entities: &[EntityRecord]) {
    w.begin_section("ENTITIES");
    for e in entities {
        match &e.geometry {
            EntityGeometry::Line(Curve::Line { a, b }) => {
                w.write_entity_header("LINE", e.handle);
                common_entity_header(w, &e.layer, e.aci);
                w.write_string(100, "AcDbLine");
                w.write_point(10, *a);
                w.write_point(11, *b);
            }
            EntityGeometry::Circle(Curve::Circle { center, radius, .. }) => {
                w.write_entity_header("CIRCLE", e.handle);
                common_entity_header(w, &e.layer, e.aci);
                w.write_string(100, "AcDbCircle");
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
                common_entity_header(w, &e.layer, e.aci);
                w.write_string(100, "AcDbCircle");
                w.write_point(10, *center);
                w.write_double(40, *radius);
                w.write_string(100, "AcDbArc");
                w.write_double(50, start_angle.to_degrees());
                w.write_double(51, end_angle.to_degrees());
            }
            EntityGeometry::Point(p) => {
                w.write_entity_header("POINT", e.handle);
                common_entity_header(w, &e.layer, e.aci);
                w.write_string(100, "AcDbPoint");
                w.write_point(10, *p);
            }
            EntityGeometry::Polyline(path) => {
                w.write_entity_header("LWPOLYLINE", e.handle);
                common_entity_header(w, &e.layer, e.aci);
                w.write_string(100, "AcDbPolyline");
                // Count line segments as vertices (+1 for start).
                let n = path.segments.len();
                let vertex_count = if n == 0 { 0 } else { n + if path.closed { 0 } else { 1 } };
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
fn common_entity_header(w: &mut DxfWriter, layer: &str, aci: u8) {
    w.write_string(100, "AcDbEntity");
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
pub fn write_blocks_section(w: &mut DxfWriter) {
    w.begin_section("BLOCKS");
    for (name, layout_owner) in [("*Model_Space", "0"), ("*Paper_Space", "0")] {
        w.write_string(0, "BLOCK");
        w.write_string(100, "AcDbEntity");
        w.write_string(8, layout_owner);
        w.write_string(100, "AcDbBlockBegin");
        w.write_string(2, name);
        w.write_int(70, 0);
        w.write_point(10, Point3D::new(0.0, 0.0, 0.0));
        w.write_string(3, name);
        w.write_string(1, "");
        w.write_string(0, "ENDBLK");
        w.write_string(100, "AcDbEntity");
        w.write_string(8, layout_owner);
        w.write_string(100, "AcDbBlockEnd");
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
    fn full_document_has_all_sections() {
        let mut w = DxfWriter::new();
        write_header_section(
            &mut w,
            &[HeaderEntry::string("$ACADVER", "AC1032")],
        );
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
}
