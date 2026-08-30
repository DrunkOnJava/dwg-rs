//! End-to-end entity dispatcher — converts a [`RawObject`] from the
//! object stream into a typed [`DecodedEntity`] by:
//!
//! 1. Positioning a [`BitCursor`] past the object header (the preamble
//!    consumed by [`crate::object::ObjectWalker`] — type code, object-
//!    size-in-bits for R2000, handle).
//! 2. Consuming the common entity preamble (spec §19.4.1).
//! 3. Invoking the type-specific decoder.
//!
//! # What this dispatcher does NOT do
//!
//! - Object types with no decoder in this crate — and the ones whose
//!   decoder reads only a documented prefix of their fields, so it
//!   cannot prove it read them correctly (see [`crate::objects`]) — are
//!   returned as [`DecodedEntity::Unhandled`] with their raw type code.
//!   Downstream callers can run those [`crate::objects`] decoders
//!   directly.
//! - Decoder errors (partial field, truncated stream, version mismatch)
//!   are captured in [`DecodedEntity::Error`] — the dispatcher does not
//!   abort the whole walk on one bad entity.
//!
//! # Honest scope
//!
//! "Decoded" here means the entity's type-specific payload is parsed
//! into a Rust struct with named fields. It does NOT mean 100% of
//! every field is surfaced — HATCH, MLEADER, VIEWPORT, and the
//! DIMENSION family expose the geometric + styling fields a viewer
//! or round-trip tool would need, but skip deeply nested sub-records
//! like HATCH boundary path trees (these remain in the raw bytes).

use crate::bitcursor::BitCursor;
use crate::entities::{
    arc, attdef, attrib, block, camera, circle, dimension, ellipse, endblk, extruded_surface,
    geodata, hatch, helix, image, insert, leader, light, line, lofted_surface, lwpolyline, mesh,
    mleader, mtext, ole2_frame, point, polyface_mesh, polygon_mesh, polyline, ray,
    revolved_surface, solid, spline, sun, swept_surface, text, three_d_face, tolerance, trace,
    underlay, vertex, viewport, wipeout, xline,
};
use crate::error::Result;
use crate::object::RawObject;
use crate::object_type::ObjectType;
use crate::version::Version;

/// A decoded entity — one variant per type this crate knows how to decode.
///
/// Non-entity objects + unknown type codes land in [`DecodedEntity::Unhandled`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum DecodedEntity {
    Line(line::Line),
    Point(point::Point),
    Circle(circle::Circle),
    Arc(arc::Arc),
    Ellipse(ellipse::Ellipse),
    Ray(ray::Ray),
    XLine(xline::XLine),
    Solid(solid::Solid),
    Trace(trace::Trace),
    ThreeDFace(three_d_face::ThreeDFace),
    Spline(spline::Spline),
    Text(text::Text),
    MText(mtext::MText),
    Attrib(attrib::Attrib),
    AttDef(attdef::AttDef),
    Insert(insert::Insert),
    Block(block::Block),
    EndBlk(endblk::EndBlk),
    Vertex(vertex::Vertex),
    Polyline(polyline::Polyline),
    LwPolyline(lwpolyline::LwPolyline),
    Mesh(mesh::Mesh),
    PolyfaceMesh(polyface_mesh::PolyfaceMesh),
    PolygonMesh(polygon_mesh::PolygonMesh),
    Dimension(dimension::Dimension),
    Leader(leader::Leader),
    Image(image::Image),
    Hatch(hatch::Hatch),
    /// Boxed because a MULTILEADER carries its embedded
    /// `MLeaderAnnotContext` inline and is by far the largest variant.
    MLeader(Box<mleader::MLeader>),
    Viewport(viewport::Viewport),
    Camera(camera::Camera),
    Sun(sun::Sun),
    Light(light::Light),
    GeoData(geodata::GeoData),
    ExtrudedSurface(extruded_surface::ExtrudedSurface),
    RevolvedSurface(revolved_surface::RevolvedSurface),
    SweptSurface(swept_surface::SweptSurface),
    LoftedSurface(lofted_surface::LoftedSurface),
    Helix(helix::Helix),
    Tolerance(tolerance::Tolerance),
    Ole2Frame(ole2_frame::Ole2Frame),
    Underlay(underlay::Underlay),
    Wipeout(wipeout::Wipeout),
    // Symbol-table entries — not drawing entities but worth
    // surfacing as typed variants for callers that iterate
    // DecodedEntity over the whole object stream.
    Layer(crate::tables::layer::Layer),
    Ltype(crate::tables::ltype::LtypeEntry),
    Style(crate::tables::style::StyleEntry),
    View(crate::tables::view::ViewEntry),
    Ucs(crate::tables::ucs::UcsEntry),
    VPort(crate::tables::vport::VportEntry),
    AppId(crate::tables::appid::AppId),
    DimStyle(crate::tables::dimstyle::DimStyleEntry),
    BlockRecord(crate::tables::block_record::BlockRecord),
    // Non-entity objects — the structural records that hold the
    // drawing together. Like the symbol-table entries above they are
    // not drawing entities, but a caller iterating DecodedEntity over
    // the whole object stream wants them typed rather than opaque.
    Dictionary(crate::objects::dictionary::Dictionary),
    DictionaryVar(crate::objects::dictionary_var::DictionaryVar),
    XRecord(crate::objects::xrecord::XRecord),
    Placeholder(crate::objects::placeholder::Placeholder),
    Group(crate::objects::acad_group::AcadGroup),
    Scale(crate::objects::acad_scale::AcadScale),
    VisualStyle(crate::objects::acad_visual_style::AcadVisualStyle),
    Layout(Box<crate::objects::acad_layout::AcadLayout>),
    PlotSettings(crate::objects::acad_plot_settings::AcadPlotSettings),
    MLineStyle(crate::objects::acad_mlinestyle::AcadMlinestyle),
    MLeaderStyle(Box<crate::objects::acad_mleader_style::AcadMLeaderStyle>),
    DetailViewStyle(Box<crate::objects::acad_detail_view_style::AcadDetailViewStyle>),
    SectionViewStyle(Box<crate::objects::acad_section_view_style::AcadSectionViewStyle>),
    ImageDef(crate::entities::imagedef::ImageDef),
    /// One of the ten `*_CONTROL` table owners; `kind` says which.
    Control {
        kind: ObjectType,
        control: crate::objects::control::Control,
    },
    /// Object type this dispatcher doesn't decode (unknown custom
    /// classes, objects whose layout this crate has not matched against
    /// real bytes yet). The raw bytes remain accessible on the
    /// originating [`RawObject`].
    Unhandled {
        type_code: u16,
        kind: ObjectType,
    },
    /// Decoder returned an error on this specific object. Walk
    /// continues; the caller decides whether to fail loudly.
    Error {
        type_code: u16,
        kind: ObjectType,
        message: String,
    },
}

impl DecodedEntity {
    /// The object type code this decoded entity corresponds to.
    pub fn type_code(&self) -> u16 {
        match self {
            Self::Line(_) => OBJECT_TYPE_LINE,
            Self::Point(_) => OBJECT_TYPE_POINT,
            Self::Circle(_) => OBJECT_TYPE_CIRCLE,
            Self::Arc(_) => OBJECT_TYPE_ARC,
            Self::Ellipse(_) => OBJECT_TYPE_ELLIPSE,
            Self::Ray(_) => OBJECT_TYPE_RAY,
            Self::XLine(_) => OBJECT_TYPE_XLINE,
            Self::Solid(_) => OBJECT_TYPE_SOLID,
            Self::Trace(_) => OBJECT_TYPE_TRACE,
            Self::ThreeDFace(_) => OBJECT_TYPE_3DFACE,
            Self::Spline(_) => OBJECT_TYPE_SPLINE,
            Self::Text(_) => OBJECT_TYPE_TEXT,
            Self::MText(_) => OBJECT_TYPE_MTEXT,
            Self::Attrib(_) => OBJECT_TYPE_ATTRIB,
            Self::AttDef(_) => OBJECT_TYPE_ATTDEF,
            Self::Insert(_) => OBJECT_TYPE_INSERT,
            Self::Block(_) => OBJECT_TYPE_BLOCK,
            Self::EndBlk(_) => OBJECT_TYPE_ENDBLK,
            Self::Vertex(_) => OBJECT_TYPE_VERTEX_2D,
            Self::Polyline(_) => OBJECT_TYPE_POLYLINE_2D,
            Self::LwPolyline(_) => OBJECT_TYPE_LWPOLYLINE,
            // MESH (subdivision) is a custom class — code varies per
            // file via AcDb:Classes. Return 0 so callers consult the
            // class map.
            Self::Mesh(_) => 0,
            Self::PolyfaceMesh(_) => OBJECT_TYPE_POLYLINE_PFACE,
            Self::PolygonMesh(_) => OBJECT_TYPE_POLYLINE_MESH,
            Self::Dimension(_) => OBJECT_TYPE_DIMENSION_LINEAR_SENTINEL,
            Self::Leader(_) => OBJECT_TYPE_LEADER,
            // IMAGE is a custom class; there is no fixed code. Return 0
            // so callers can detect this and consult the class map.
            Self::Image(_) => 0,
            Self::Hatch(_) => OBJECT_TYPE_HATCH,
            // MLEADER is a custom class; see Image above.
            Self::MLeader(_) => 0,
            Self::Viewport(_) => OBJECT_TYPE_VIEWPORT,
            Self::Camera(_) => OBJECT_TYPE_CAMERA,
            Self::Sun(_) => OBJECT_TYPE_SUN,
            Self::Light(_) => OBJECT_TYPE_LIGHT,
            Self::GeoData(_) => OBJECT_TYPE_GEODATA,
            // SURFACE family + HELIX are custom classes — their type
            // codes vary per-file via AcDb:Classes. Return 0 so callers
            // know to consult the class map.
            Self::ExtrudedSurface(_) => 0,
            Self::RevolvedSurface(_) => 0,
            Self::SweptSurface(_) => 0,
            Self::LoftedSurface(_) => 0,
            Self::Helix(_) => 0,
            Self::Tolerance(_) => OBJECT_TYPE_TOLERANCE,
            Self::Ole2Frame(_) => OBJECT_TYPE_OLE2FRAME,
            // UNDERLAY family + WIPEOUT are custom classes — codes
            // vary per-file via AcDb:Classes.
            Self::Underlay(_) => 0,
            Self::Wipeout(_) => 0,
            Self::Layer(_) => 0x33,
            Self::Ltype(_) => 0x39,
            Self::Style(_) => 0x35,
            Self::View(_) => 0x3D,
            Self::Ucs(_) => 0x3F,
            Self::VPort(_) => 0x41,
            Self::AppId(_) => 0x43,
            Self::DimStyle(_) => 0x45,
            Self::BlockRecord(_) => 0x31,
            Self::Dictionary(_) => OBJECT_TYPE_DICTIONARY,
            Self::XRecord(_) => OBJECT_TYPE_XRECORD,
            Self::Placeholder(_) => OBJECT_TYPE_ACDB_PLACEHOLDER,
            Self::Group(_) => OBJECT_TYPE_GROUP,
            Self::Layout(_) => OBJECT_TYPE_LAYOUT,
            Self::MLineStyle(_) => OBJECT_TYPE_MLINESTYLE,
            // DICTIONARYVAR and SCALE are custom classes — their codes
            // vary per file via AcDb:Classes. Return 0 so callers know
            // to consult the class map.
            Self::DictionaryVar(_)
            | Self::Scale(_)
            | Self::ImageDef(_)
            | Self::VisualStyle(_)
            | Self::PlotSettings(_)
            | Self::MLeaderStyle(_)
            | Self::DetailViewStyle(_)
            | Self::SectionViewStyle(_) => 0,
            Self::Control { kind, .. } => control_type_code(*kind),
            Self::Unhandled { type_code, .. } | Self::Error { type_code, .. } => *type_code,
        }
    }

    /// Did this variant come back as a fully-typed, successfully
    /// parsed entity?
    pub fn is_decoded(&self) -> bool {
        !matches!(self, Self::Unhandled { .. } | Self::Error { .. })
    }
}

// Object type codes per ODA spec v5.4.1 §5 Table 4 — "Object type codes, BS".
// Cross-checked against object_type.rs ObjectType::from_code; the two
// tables MUST agree (see tests::dispatch_and_object_type_codes_agree).
//
// Fixed codes only (< 500). IMAGE and MLEADER are custom classes whose
// codes are assigned per-file via AcDb:Classes — they are NOT fixed
// codes and do NOT appear here; see task #96 for custom-class dispatch.
const OBJECT_TYPE_TEXT: u16 = 0x01; // 1
const OBJECT_TYPE_ATTRIB: u16 = 0x02; // 2
const OBJECT_TYPE_ATTDEF: u16 = 0x03; // 3
const OBJECT_TYPE_BLOCK: u16 = 0x04; // 4
const OBJECT_TYPE_ENDBLK: u16 = 0x05; // 5
const OBJECT_TYPE_INSERT: u16 = 0x07; // 7
const OBJECT_TYPE_VERTEX_2D: u16 = 0x0A; // 10
const OBJECT_TYPE_POLYLINE_2D: u16 = 0x0F; // 15
// Legacy mesh entities — POLYFACE_MESH (face-list) + POLYGON_MESH (M×N grid).
// Vertex data for both lives in a handle chain of VERTEX_PFACE /
// VERTEX_MESH sub-entities — the headers decoded here carry only the
// counts + handle-chain endpoints.
const OBJECT_TYPE_POLYLINE_PFACE: u16 = 0x1D; // 29 — POLYFACE_MESH
const OBJECT_TYPE_POLYLINE_MESH: u16 = 0x1E; // 30 — POLYGON_MESH
const OBJECT_TYPE_ARC: u16 = 0x11; // 17 (spec says ARC, was incorrectly CIRCLE)
const OBJECT_TYPE_CIRCLE: u16 = 0x12; // 18 (spec says CIRCLE, was incorrectly ARC)
const OBJECT_TYPE_LINE: u16 = 0x13; // 19
// DIMENSION family spans 0x14..=0x1A (20..=26), handled via a range match
// in `dispatch()` + `DimensionKind::from_object_type_code`.
const OBJECT_TYPE_DIMENSION_MIN: u16 = 0x14; // 20
const OBJECT_TYPE_DIMENSION_MAX: u16 = 0x1A; // 26
const OBJECT_TYPE_POINT: u16 = 0x1B; // 27
const OBJECT_TYPE_3DFACE: u16 = 0x1C; // 28 (was incorrectly 32)
const OBJECT_TYPE_SOLID: u16 = 0x1F; // 31
const OBJECT_TYPE_TRACE: u16 = 0x20; // 32 (was incorrectly 30)
const OBJECT_TYPE_SHAPE: u16 = 0x21; // 33
const OBJECT_TYPE_VIEWPORT: u16 = 0x22; // 34
const OBJECT_TYPE_ELLIPSE: u16 = 0x23; // 35
const OBJECT_TYPE_SPLINE: u16 = 0x24; // 36
const OBJECT_TYPE_RAY: u16 = 0x28; // 40
const OBJECT_TYPE_XLINE: u16 = 0x29; // 41
const OBJECT_TYPE_MTEXT: u16 = 0x2C; // 44
const OBJECT_TYPE_LEADER: u16 = 0x2D; // 45
const OBJECT_TYPE_TOLERANCE: u16 = 0x2E; // 46
const OBJECT_TYPE_OLE2FRAME: u16 = 0x4A; // 74
const OBJECT_TYPE_LWPOLYLINE: u16 = 0x4D; // 77
const OBJECT_TYPE_HATCH: u16 = 0x4E; // 78
// CAMERA / SUN / LIGHT / GEODATA are visual/scene entities introduced
// in R2007+ (GEODATA in R2010+). Per the spec they appear in the
// AcDb:Classes table per-file, but every observed modern drawing uses
// the codes below when present outside the dynamic range, so they are
// wired as fixed codes here. If a file assigns a different class
// index, custom-class dispatch via decode_from_raw_with_class_map
// is the correct path.
/// `AcDb:Classes` item class id that marks a class as a non-entity
/// object (§5.7). `0x1F2` is its entity counterpart.
const PROXY_OBJECT_ITEM_CLASS_ID: u16 = 0x1F3;

// Non-entity object codes (§5 Table 4). These reach the dispatcher
// through `dispatch_object`, not through the entity path.
const OBJECT_TYPE_DICTIONARY: u16 = 0x2A; // 42
const OBJECT_TYPE_GROUP: u16 = 0x48; // 72
const OBJECT_TYPE_XRECORD: u16 = 0x4F; // 79
const OBJECT_TYPE_ACDB_PLACEHOLDER: u16 = 0x50; // 80
const OBJECT_TYPE_LAYOUT: u16 = 0x52; // 82
const OBJECT_TYPE_MLINESTYLE: u16 = 0x49; // 73

const OBJECT_TYPE_CAMERA: u16 = 0x4F8; // 1272
const OBJECT_TYPE_SUN: u16 = 0x4F9; // 1273
const OBJECT_TYPE_LIGHT: u16 = 0x4FA; // 1274
const OBJECT_TYPE_GEODATA: u16 = 0x4FB; // 1275

/// The used-for-the-back-fix chosen sentinel that `DecodedEntity::type_code()`
/// returns for the `Dimension(...)` variant. Any code in `0x14..=0x1A` would
/// be defensible; the sentinel always points at LINEAR because that is the
/// most common dimension subtype in real drawings.
const OBJECT_TYPE_DIMENSION_LINEAR_SENTINEL: u16 = 0x15; // 21

/// Decode a [`RawObject`] whose type code is a custom class (≥ 500)
/// by looking up the class in [`crate::classes::ClassMap`] and
/// dispatching on the DXF class name.
///
/// Supports IMAGE, MLEADER, and other post-spec entities whose type
/// codes vary per file. Unknown class names fall through to
/// [`DecodedEntity::Unhandled`].
pub fn decode_from_raw_with_class_map(
    raw: &RawObject,
    version: Version,
    class_map: &crate::classes::ClassMap,
    type_code: u16,
) -> DecodedEntity {
    let Some(class_def) = class_map.by_type_code(type_code) else {
        return DecodedEntity::Unhandled {
            type_code,
            kind: raw.kind,
        };
    };
    // Non-entity custom classes first: they carry the common *object*
    // data, so feeding them to the entity preamble reader would
    // mis-align every field after it.
    if let Some(decoded) = dispatch_object_class(
        raw,
        class_def.dxf_class_name.as_str(),
        type_code,
        raw.kind,
        version,
    ) {
        return decoded;
    }
    // A class whose item class id is ACAD_PROXY_OBJECT is a non-entity;
    // running it through the common entity preamble below would
    // mis-read every field and report a decoder error for what is
    // simply a type this crate has no object decoder for.
    if class_def.item_class_id == PROXY_OBJECT_ITEM_CLASS_ID {
        return DecodedEntity::Unhandled {
            type_code,
            kind: raw.kind,
        };
    }
    // Position cursor past header + common preamble, then dispatch by
    // DXF class name.
    let mut cursor = match position_cursor_at_entity_body(raw, version) {
        Ok(c) => c,
        Err(e) => {
            return DecodedEntity::Error {
                type_code,
                kind: raw.kind,
                message: format!("failed to position cursor: {e}"),
            };
        }
    };
    // Custom-class entities whose `TV` fields live in the R2007+ string
    // stream take the payload-and-offset form instead of the cursor
    // form, and read the common preamble themselves.
    if let Some(decoded) = dispatch_split_stream_class(
        raw,
        class_def.dxf_class_name.as_str(),
        cursor.position_bits(),
        type_code,
        raw.kind,
        version,
    ) {
        return decoded;
    }
    if let Err(e) = crate::common_entity::read_common_entity_data(&mut cursor, version) {
        return DecodedEntity::Error {
            type_code,
            kind: raw.kind,
            message: format!("common entity preamble: {e}"),
        };
    }
    let result: std::result::Result<DecodedEntity, String> = match class_def.dxf_class_name.as_str()
    {
        "IMAGE" | "RASTERIMAGE" => image::decode(&mut cursor, version)
            .map(DecodedEntity::Image)
            .map_err(|e| e.to_string()),
        // SURFACE family + HELIX — type codes vary per-file, dispatched
        // on the DXF class name recorded in AcDb:Classes. See spec
        // §19.4.76 (HELIX) and §19.4.78-81 (SURFACE variants).
        "EXTRUDEDSURFACE" | "ACDBEXTRUDEDSURFACE" => extruded_surface::decode(&mut cursor)
            .map(DecodedEntity::ExtrudedSurface)
            .map_err(|e| e.to_string()),
        "REVOLVEDSURFACE" | "ACDBREVOLVEDSURFACE" => revolved_surface::decode(&mut cursor)
            .map(DecodedEntity::RevolvedSurface)
            .map_err(|e| e.to_string()),
        "SWEPTSURFACE" | "ACDBSWEPTSURFACE" => swept_surface::decode(&mut cursor)
            .map(DecodedEntity::SweptSurface)
            .map_err(|e| e.to_string()),
        "LOFTEDSURFACE" | "ACDBLOFTEDSURFACE" => lofted_surface::decode(&mut cursor)
            .map(DecodedEntity::LoftedSurface)
            .map_err(|e| e.to_string()),
        "HELIX" | "ACDBHELIX" => helix::decode(&mut cursor)
            .map(DecodedEntity::Helix)
            .map_err(|e| e.to_string()),
        // UNDERLAY family (§19.4.86) — PDF / DWF / DGN share one payload.
        "WIPEOUT" | "ACDBWIPEOUT" => wipeout::decode(&mut cursor, version)
            .map(DecodedEntity::Wipeout)
            .map_err(|e| e.to_string()),
        // MESH (subdivision surface) — R2010+ custom class §19.4.66.
        // Class name varies slightly across AutoCAD versions; all three
        // observed names dispatch to the same decoder.
        "MESH" | "ACDBSUBDMESH" | "ACDBMESH" => mesh::decode(&mut cursor, version)
            .map(DecodedEntity::Mesh)
            .map_err(|e| e.to_string()),
        _ => {
            return DecodedEntity::Unhandled {
                type_code,
                kind: raw.kind,
            };
        }
    };
    match result {
        Ok(entity) => entity,
        Err(message) => DecodedEntity::Error {
            type_code,
            kind: raw.kind,
            message,
        },
    }
}

/// Decode a [`RawObject`] to a typed [`DecodedEntity`].
///
/// This positions a fresh [`BitCursor`] on the raw payload bytes,
/// skips the object header (type code, R2000 size-in-bits, handle),
/// consumes the common entity preamble, then dispatches on type code
/// to the matching per-entity decoder.
///
/// On decoder error, returns [`DecodedEntity::Error`] rather than
/// propagating — the dispatcher intentionally does not abort a walk
/// on a single bad entity.
///
/// For custom-class entities (type codes ≥ 500 like IMAGE, MLEADER,
/// TABLE), see [`decode_from_raw_with_class_map`] which resolves the
/// code via [`crate::classes::ClassMap`] before dispatching.
pub fn decode_from_raw(raw: &RawObject, version: Version) -> DecodedEntity {
    let type_code = raw.type_code;
    let kind = raw.kind;

    // Objects that are neither drawing entities NOR symbol-table
    // entries (DICTIONARY, XRECORD, control objects, ...) take the
    // non-entity path: a shorter common prefix, and no common entity
    // preamble at all.
    if !raw.is_entity() && !kind.is_table_entry() {
        return dispatch_object(raw, type_code, kind, version);
    }

    match position_cursor_at_entity_body(raw, version) {
        Ok(mut cursor) => {
            if kind.is_table_entry() {
                dispatch_table_entry(raw, &mut cursor, type_code, kind, version)
            } else if let Some(decoded) =
                dispatch_split_stream_entity(raw, cursor.position_bits(), type_code, kind, version)
            {
                decoded
            } else {
                dispatch(&mut cursor, type_code, kind, version)
            }
        }
        Err(e) => DecodedEntity::Error {
            type_code,
            kind,
            message: format!("failed to position cursor: {e}"),
        },
    }
}

/// Object type code of a `*_CONTROL` record (§5 Table 4). Returns 0 for
/// anything that is not a control object.
fn control_type_code(kind: ObjectType) -> u16 {
    match kind {
        ObjectType::BlockControl => 0x30,
        ObjectType::LayerControl => 0x32,
        ObjectType::StyleControl => 0x34,
        ObjectType::LtypeControl => 0x38,
        ObjectType::ViewControl => 0x3C,
        ObjectType::UcsControl => 0x3E,
        ObjectType::VportControl => 0x40,
        ObjectType::AppIdControl => 0x42,
        ObjectType::DimStyleControl => 0x44,
        ObjectType::VpEntHdrCtrl => 0x46,
        _ => 0,
    }
}

/// Dispatch a non-entity object — DICTIONARY, XRECORD, the
/// `*_CONTROL` owners, ACDB_PLACEHOLDER, ACAD_GROUP.
///
/// These carry the common *object* data of §19.4.2 rather than the
/// common entity preamble, and from R2007 their `TV` fields live in the
/// object's string stream, so they route through
/// [`crate::objects::modern`] instead of the entity cursor path. Every
/// decoder reached from here checks that its data fields end exactly on
/// the record's data-stream boundary, so a wrong field list produces
/// [`DecodedEntity::Error`], never a plausible-looking struct.
fn dispatch_object(
    raw: &RawObject,
    type_code: u16,
    kind: ObjectType,
    version: Version,
) -> DecodedEntity {
    // Decide whether this crate has a decoder *before* touching the
    // bytes, so a type with no decoder reports Unhandled rather than a
    // cursor error from the header skip.
    let has_decoder = matches!(
        kind,
        ObjectType::Dictionary
            | ObjectType::XRecord
            | ObjectType::AcDbPlaceholder
            | ObjectType::Group
            | ObjectType::Layout
            | ObjectType::MLineStyle
    ) || kind.is_control();
    if !has_decoder {
        return DecodedEntity::Unhandled { type_code, kind };
    }
    let body_start = match crate::object::body_cursor(raw, version) {
        Ok(c) => c.position_bits(),
        Err(e) => {
            return DecodedEntity::Error {
                type_code,
                kind,
                message: format!("failed to position cursor: {e}"),
            };
        }
    };
    let inline_end = raw.obj_size_bits.map(|b| b as usize);
    let payload = raw.raw.as_slice();
    use crate::objects::{control, dictionary, placeholder, xrecord};
    let result: core::result::Result<DecodedEntity, String> = match kind {
        ObjectType::Dictionary => {
            dictionary::decode_object(payload, body_start, inline_end, version)
                .map(DecodedEntity::Dictionary)
                .map_err(|e| e.to_string())
        }
        ObjectType::XRecord => xrecord::decode_object(payload, body_start, inline_end, version)
            .map(DecodedEntity::XRecord)
            .map_err(|e| e.to_string()),
        ObjectType::AcDbPlaceholder => {
            placeholder::decode_object(payload, body_start, inline_end, version)
                .map(DecodedEntity::Placeholder)
                .map_err(|e| e.to_string())
        }
        ObjectType::Group => {
            crate::objects::acad_group::decode_object(payload, body_start, inline_end, version)
                .map(DecodedEntity::Group)
                .map_err(|e| e.to_string())
        }
        ObjectType::Layout => {
            match crate::objects::acad_layout::decode_object(
                payload, body_start, inline_end, version,
            ) {
                Ok(layout) => Ok(DecodedEntity::Layout(Box::new(layout))),
                // "this release's layout is not determined" is not a
                // decode failure — see `dispatch_object_class`.
                Err(crate::error::Error::Unsupported { .. }) => {
                    return DecodedEntity::Unhandled { type_code, kind };
                }
                Err(e) => Err(e.to_string()),
            }
        }
        ObjectType::MLineStyle => {
            crate::objects::acad_mlinestyle::decode_object(payload, body_start, inline_end, version)
                .map(DecodedEntity::MLineStyle)
                .map_err(|e| e.to_string())
        }
        k if k.is_control() => control::decode_object(payload, body_start, inline_end, version, k)
            .map(|control| DecodedEntity::Control { kind: k, control })
            .map_err(|e| e.to_string()),
        _ => return DecodedEntity::Unhandled { type_code, kind },
    };
    match result {
        Ok(decoded) => decoded,
        Err(message) => DecodedEntity::Error {
            type_code,
            kind,
            message,
        },
    }
}

/// Dispatch a non-entity object whose type code came from the class
/// map — the `AcDbScale` / `AcDbDictionaryVar` / `AcDbDictionaryWithDefault`
/// family. Returns `None` when the class name has no self-validating
/// object decoder, so the caller can fall through to the entity path.
fn dispatch_object_class(
    raw: &RawObject,
    dxf_class_name: &str,
    type_code: u16,
    kind: ObjectType,
    version: Version,
) -> Option<DecodedEntity> {
    let body_start = crate::object::body_cursor(raw, version)
        .map(|c| c.position_bits())
        .ok()?;
    let inline_end = raw.obj_size_bits.map(|b| b as usize);
    let payload = raw.raw.as_slice();
    let result = match dxf_class_name {
        "SCALE" | "ACDBSCALE" => {
            crate::objects::acad_scale::decode_object(payload, body_start, inline_end, version)
                .map(DecodedEntity::Scale)
        }
        "VISUALSTYLE" | "ACDBVISUALSTYLE" => crate::objects::acad_visual_style::decode_object(
            payload, body_start, inline_end, version,
        )
        .map(DecodedEntity::VisualStyle),
        // A standalone PLOTSETTINGS record carries exactly the block
        // §20.4.84 embeds in LAYOUT; the two share one field list.
        "PLOTSETTINGS" | "ACDBPLOTSETTINGS" => crate::objects::acad_plot_settings::decode_object(
            payload, body_start, inline_end, version,
        )
        .map(DecodedEntity::PlotSettings),
        "MLEADERSTYLE" | "ACDBMLEADERSTYLE" => crate::objects::acad_mleader_style::decode_object(
            payload, body_start, inline_end, version,
        )
        .map(|style| DecodedEntity::MLeaderStyle(Box::new(style))),
        "ACDBDETAILVIEWSTYLE" | "DETAILVIEWSTYLE" => {
            crate::objects::acad_detail_view_style::decode_object(
                payload, body_start, inline_end, version,
            )
            .map(|style| DecodedEntity::DetailViewStyle(Box::new(style)))
        }
        "ACDBSECTIONVIEWSTYLE" | "SECTIONVIEWSTYLE" => {
            crate::objects::acad_section_view_style::decode_object(
                payload, body_start, inline_end, version,
            )
            .map(|style| DecodedEntity::SectionViewStyle(Box::new(style)))
        }
        "DICTIONARYVAR" | "ACDBDICTIONARYVAR" => {
            crate::objects::dictionary_var::decode_object(payload, body_start, inline_end, version)
                .map(DecodedEntity::DictionaryVar)
        }
        // A dictionary-with-default is a DICTIONARY plus one extra
        // handle, and handles are not data-stream fields, so the two
        // share a field list exactly. Measured on `arc_2004.dwg`
        // handle 14 and `arc_2013.dwg` handle 14, both of which close
        // on their boundary with the DICTIONARY body.
        "ACDBDICTIONARYWDFLT" | "ACDBDICTIONARYWITHDEFAULT" => {
            crate::objects::dictionary::decode_object(payload, body_start, inline_end, version)
                .map(DecodedEntity::Dictionary)
        }
        "IMAGEDEF" | "ACDBRASTERIMAGEDEF" => {
            crate::entities::imagedef::decode_object(payload, body_start, inline_end, version)
                .map(DecodedEntity::ImageDef)
        }
        _ => return None,
    };
    Some(match result {
        Ok(decoded) => decoded,
        // "this release's layout is not determined" is not a decode
        // failure — the record is simply one this crate does not handle
        // yet, so it belongs in the Unhandled bucket rather than being
        // reported as a broken record.
        Err(crate::error::Error::Unsupported { .. }) => {
            DecodedEntity::Unhandled { type_code, kind }
        }
        Err(e) => DecodedEntity::Error {
            type_code,
            kind,
            message: e.to_string(),
        },
    })
}

/// Dispatch a **custom-class** entity whose `TV` fields live in the
/// object's string stream and whose `H` fields live in the handle
/// stream (spec §19.1 / §20.4.48).
///
/// These decoders take the record payload plus the bit offset of the
/// common entity preamble rather than a positioned cursor, because they
/// have to locate the string stream inside the same payload. Returns
/// `None` when the class or version has no split-stream decoder, so the
/// caller falls through to the cursor path.
fn dispatch_split_stream_class(
    raw: &RawObject,
    dxf_class_name: &str,
    object_body_start: usize,
    type_code: u16,
    kind: ObjectType,
    version: Version,
) -> Option<DecodedEntity> {
    if !version.is_r2010_plus() {
        return None;
    }
    let result = match dxf_class_name {
        "MULTILEADER" | "MLEADER" | "ACDBMULTILEADER" => {
            mleader::decode_modern_split_stream(&raw.raw, object_body_start, version)
                .map(|m| DecodedEntity::MLeader(Box::new(m)))
        }
        // UNDERLAY family (§ not in v5.4 — see `entities::underlay`).
        "PDFUNDERLAY" | "ACDBPDFUNDERLAY" | "DWFUNDERLAY" | "ACDBDWFUNDERLAY" | "DGNUNDERLAY"
        | "ACDBDGNUNDERLAY" => {
            let kind = match dxf_class_name {
                "DWFUNDERLAY" | "ACDBDWFUNDERLAY" => underlay::UnderlayKind::Dwf,
                "DGNUNDERLAY" | "ACDBDGNUNDERLAY" => underlay::UnderlayKind::Dgn,
                _ => underlay::UnderlayKind::Pdf,
            };
            checked_inline(raw, object_body_start, version, "UNDERLAY", move |c, v| {
                underlay::decode(c, kind, v)
            })
            .map(DecodedEntity::Underlay)
        }
        _ => return None,
    };
    Some(match result {
        Ok(decoded) => decoded,
        Err(e) => DecodedEntity::Error {
            type_code,
            kind,
            message: e.to_string(),
        },
    })
}

/// Run an inline entity decoder against a record's payload and require
/// it to end **exactly** on the record's data-stream boundary — the
/// first bit of its string stream, or the start of its handle stream
/// when it holds no strings (§19.1).
///
/// Entities with no `TV` field do not need the split streams to read
/// their values, but they still get the boundary for free, and the
/// boundary is what turns a wrong field list into an error instead of
/// plausible-looking geometry. Types listed here have been measured
/// against every record of that type in the corpus.
fn checked_inline<T>(
    raw: &RawObject,
    object_body_start: usize,
    version: Version,
    what: &'static str,
    decode_body: impl FnOnce(&mut BitCursor<'_>, Version) -> Result<T>,
) -> Result<T> {
    let (_strings, string_start) = crate::tables::modern::open_entity(&raw.raw, version)?;
    let mut c = BitCursor::new(&raw.raw);
    crate::string_stream::seek(&mut c, object_body_start)?;
    crate::common_entity::read_common_entity_data(&mut c, version)?;
    let value = decode_body(&mut c, version)?;
    let at = c.position_bits();
    if at != string_start {
        return Err(crate::tables::modern::misaligned(what, at, string_start));
    }
    Ok(value)
}

/// Dispatch the R2007+ entities whose `TV` fields live in the object's
/// string stream (spec §19.1). Returns `None` when the type or version
/// is not handled by a split-stream decoder, so the caller falls back
/// to the inline path.
fn dispatch_split_stream_entity(
    raw: &RawObject,
    object_body_start: usize,
    type_code: u16,
    kind: ObjectType,
    version: Version,
) -> Option<DecodedEntity> {
    if !version.is_r2007_plus() {
        return None;
    }
    let result = match type_code {
        OBJECT_TYPE_TEXT => text::decode_modern_split_stream(&raw.raw, object_body_start, version)
            .map(DecodedEntity::Text),
        OBJECT_TYPE_ATTRIB => {
            attrib::decode_modern_split_stream(&raw.raw, object_body_start, version)
                .map(DecodedEntity::Attrib)
        }
        OBJECT_TYPE_ATTDEF => {
            attdef::decode_modern_split_stream(&raw.raw, object_body_start, version)
                .map(DecodedEntity::AttDef)
        }
        OBJECT_TYPE_MTEXT => {
            mtext::decode_modern_split_stream(&raw.raw, object_body_start, version)
                .map(DecodedEntity::MText)
        }
        OBJECT_TYPE_TOLERANCE => {
            tolerance::decode_modern_split_stream(&raw.raw, object_body_start, version)
                .map(DecodedEntity::Tolerance)
        }
        OBJECT_TYPE_HATCH => {
            hatch::decode_modern_split_stream(&raw.raw, object_body_start, version)
                .map(DecodedEntity::Hatch)
        }
        OBJECT_TYPE_SPLINE if version.is_r2010_plus() => {
            spline::decode_modern_split_stream(&raw.raw, object_body_start, version)
                .map(DecodedEntity::Spline)
        }
        OBJECT_TYPE_INSERT if version.is_r2010_plus() => {
            checked_inline(raw, object_body_start, version, "INSERT", insert::decode)
                .map(DecodedEntity::Insert)
        }
        OBJECT_TYPE_3DFACE if version.is_r2010_plus() => {
            checked_inline(raw, object_body_start, version, "3DFACE", |c, _v| {
                three_d_face::decode(c)
            })
            .map(DecodedEntity::ThreeDFace)
        }
        OBJECT_TYPE_LWPOLYLINE if version.is_r2010_plus() => checked_inline(
            raw,
            object_body_start,
            version,
            "LWPOLYLINE",
            lwpolyline::decode,
        )
        .map(DecodedEntity::LwPolyline),
        OBJECT_TYPE_LIGHT => {
            light::decode_modern_split_stream(&raw.raw, object_body_start, version)
                .map(DecodedEntity::Light)
        }
        OBJECT_TYPE_GEODATA => {
            geodata::decode_modern_split_stream(&raw.raw, object_body_start, version)
                .map(DecodedEntity::GeoData)
        }
        OBJECT_TYPE_DIMENSION_MIN..=OBJECT_TYPE_DIMENSION_MAX => {
            let kind = dimension::DimensionKind::from_object_type_code(type_code)?;
            dimension::decode_modern_split_stream(&raw.raw, object_body_start, version, kind)
                .map(DecodedEntity::Dimension)
        }
        _ => return None,
    };
    Some(match result {
        Ok(decoded) => decoded,
        Err(e) => DecodedEntity::Error {
            type_code,
            kind,
            message: e.to_string(),
        },
    })
}

/// Dispatch a symbol-table entry to its per-type decoder.
///
/// Each table decoder internally calls
/// [`crate::tables::read_table_entry_header`] for the shared
/// table-entry preamble, so the cursor positioning here is the same
/// as for drawing entities.
///
/// The pre-R2007 decoders read inline from `c`, so the common object
/// data (§19.4.2 — EED chain, reactor count, xdictionary flag) is
/// consumed here first. The R2007+ split-stream decoders take the
/// bit offset instead and skip that prefix themselves
/// ([`crate::tables::modern::open_table_entry`]), so `c` must stay
/// parked immediately after the object handle for them.
fn dispatch_table_entry(
    raw: &RawObject,
    c: &mut BitCursor<'_>,
    type_code: u16,
    kind: ObjectType,
    version: Version,
) -> DecodedEntity {
    if !version.is_r2007_plus() {
        if let Err(e) = crate::common_entity::read_common_object_data(c, version) {
            return DecodedEntity::Error {
                type_code,
                kind,
                message: format!("common object data: {e}"),
            };
        }
    }
    let result: core::result::Result<DecodedEntity, String> = match kind {
        ObjectType::Layer if version.is_r2007_plus() => {
            crate::tables::layer::decode_modern_split_stream(&raw.raw, c.position_bits(), version)
                .map(DecodedEntity::Layer)
                .map_err(|e| e.to_string())
        }
        ObjectType::Layer => crate::tables::layer::decode(c, version)
            .map(DecodedEntity::Layer)
            .map_err(|e| e.to_string()),
        ObjectType::Ltype if version.is_r2007_plus() => {
            match crate::tables::ltype::decode_modern_split_stream(
                &raw.raw,
                c.position_bits(),
                version,
            ) {
                Ok(ltype) => Ok(DecodedEntity::Ltype(ltype)),
                Err(_) => crate::tables::ltype::decode(c, version)
                    .map(DecodedEntity::Ltype)
                    .map_err(|e| e.to_string()),
            }
        }
        ObjectType::Ltype => crate::tables::ltype::decode(c, version)
            .map(DecodedEntity::Ltype)
            .map_err(|e| e.to_string()),
        ObjectType::Style if version.is_r2007_plus() => {
            crate::tables::style::decode_modern_split_stream(&raw.raw, c.position_bits(), version)
                .map(DecodedEntity::Style)
                .map_err(|e| e.to_string())
        }
        ObjectType::Style => crate::tables::style::decode(c, version)
            .map(DecodedEntity::Style)
            .map_err(|e| e.to_string()),
        ObjectType::View if version.is_r2007_plus() => {
            crate::tables::view::decode_modern_split_stream(&raw.raw, c.position_bits(), version)
                .map(DecodedEntity::View)
                .map_err(|e| e.to_string())
        }
        ObjectType::View => crate::tables::view::decode(c, version)
            .map(DecodedEntity::View)
            .map_err(|e| e.to_string()),
        ObjectType::Ucs if version.is_r2007_plus() => {
            crate::tables::ucs::decode_modern_split_stream(&raw.raw, c.position_bits(), version)
                .map(DecodedEntity::Ucs)
                .map_err(|e| e.to_string())
        }
        ObjectType::Ucs => crate::tables::ucs::decode(c, version)
            .map(DecodedEntity::Ucs)
            .map_err(|e| e.to_string()),
        ObjectType::Vport if version.is_r2007_plus() => {
            crate::tables::vport::decode_modern_split_stream(&raw.raw, c.position_bits(), version)
                .map(DecodedEntity::VPort)
                .map_err(|e| e.to_string())
        }
        ObjectType::Vport => crate::tables::vport::decode(c, version)
            .map(DecodedEntity::VPort)
            .map_err(|e| e.to_string()),
        ObjectType::AppId if version.is_r2007_plus() => {
            crate::tables::appid::decode_modern_split_stream(&raw.raw, c.position_bits(), version)
                .map(DecodedEntity::AppId)
                .map_err(|e| e.to_string())
        }
        ObjectType::AppId => crate::tables::appid::decode(c, version)
            .map(DecodedEntity::AppId)
            .map_err(|e| e.to_string()),
        ObjectType::DimStyle if version.is_r2007_plus() => {
            crate::tables::dimstyle::decode_modern_split_stream(
                &raw.raw,
                c.position_bits(),
                version,
            )
            .map(DecodedEntity::DimStyle)
            .map_err(|e| e.to_string())
        }
        ObjectType::DimStyle => crate::tables::dimstyle::decode_partial(c, version)
            .map(DecodedEntity::DimStyle)
            .map_err(|e| e.to_string()),
        ObjectType::BlockHeader if version.is_r2007_plus() => {
            crate::tables::block_record::decode_modern_split_stream(
                &raw.raw,
                c.position_bits(),
                version,
            )
            .map(DecodedEntity::BlockRecord)
            .map_err(|e| e.to_string())
        }
        ObjectType::BlockHeader => crate::tables::block_record::decode(c, version)
            .map(DecodedEntity::BlockRecord)
            .map_err(|e| e.to_string()),
        _ => return DecodedEntity::Unhandled { type_code, kind },
    };
    match result {
        Ok(decoded) => decoded,
        Err(message) => DecodedEntity::Error {
            type_code,
            kind,
            message,
        },
    }
}

/// Replay the object-header reads so the cursor lands just past the
/// handle, at the start of the common entity preamble. Mirrors the
/// logic in [`crate::object::ObjectWalker::read_one_at_pos`] for the
/// payload-level fields only (the MS header is already stripped by
/// the walker).
fn position_cursor_at_entity_body<'a>(
    raw: &'a RawObject,
    version: Version,
) -> Result<BitCursor<'a>> {
    crate::object::body_cursor(raw, version)
}

fn dispatch(
    cursor: &mut BitCursor<'_>,
    type_code: u16,
    kind: ObjectType,
    version: Version,
) -> DecodedEntity {
    // Step through the common entity preamble first (§19.4.1).
    if let Err(e) = crate::common_entity::read_common_entity_data(cursor, version) {
        return DecodedEntity::Error {
            type_code,
            kind,
            message: format!("common entity preamble: {e}"),
        };
    }

    // Dispatch on fixed type code.
    let result: std::result::Result<DecodedEntity, String> = match type_code {
        OBJECT_TYPE_LINE => line::decode(cursor)
            .map(DecodedEntity::Line)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_POINT => point::decode(cursor)
            .map(DecodedEntity::Point)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_CIRCLE => circle::decode(cursor)
            .map(DecodedEntity::Circle)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_ARC => arc::decode(cursor)
            .map(DecodedEntity::Arc)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_ELLIPSE => ellipse::decode(cursor)
            .map(DecodedEntity::Ellipse)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_RAY => ray::decode(cursor)
            .map(DecodedEntity::Ray)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_XLINE => xline::decode(cursor)
            .map(DecodedEntity::XLine)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_SOLID => solid::decode(cursor)
            .map(DecodedEntity::Solid)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_TRACE => trace::decode(cursor)
            .map(DecodedEntity::Trace)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_3DFACE => three_d_face::decode(cursor)
            .map(DecodedEntity::ThreeDFace)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_SPLINE => spline::decode(cursor, version)
            .map(DecodedEntity::Spline)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_TEXT => text::decode(cursor, version)
            .map(DecodedEntity::Text)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_MTEXT => mtext::decode(cursor, version)
            .map(DecodedEntity::MText)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_ATTRIB => attrib::decode(cursor, version)
            .map(DecodedEntity::Attrib)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_ATTDEF => attdef::decode(cursor, version)
            .map(DecodedEntity::AttDef)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_INSERT => insert::decode(cursor, version)
            .map(DecodedEntity::Insert)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_BLOCK => block::decode(cursor, version)
            .map(DecodedEntity::Block)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_ENDBLK => endblk::decode(cursor)
            .map(DecodedEntity::EndBlk)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_VERTEX_2D => vertex::decode(cursor, version)
            .map(DecodedEntity::Vertex)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_POLYLINE_2D => polyline::decode(cursor)
            .map(DecodedEntity::Polyline)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_LWPOLYLINE => lwpolyline::decode(cursor, version)
            .map(DecodedEntity::LwPolyline)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_POLYLINE_PFACE => polyface_mesh::decode(cursor)
            .map(DecodedEntity::PolyfaceMesh)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_POLYLINE_MESH => polygon_mesh::decode(cursor)
            .map(DecodedEntity::PolygonMesh)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_LEADER => leader::decode(cursor)
            .map(DecodedEntity::Leader)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_TOLERANCE => tolerance::decode(cursor, version)
            .map(DecodedEntity::Tolerance)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_OLE2FRAME => ole2_frame::decode(cursor)
            .map(DecodedEntity::Ole2Frame)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_HATCH => hatch::decode(cursor, version)
            .map(DecodedEntity::Hatch)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_VIEWPORT => viewport::decode(cursor)
            .map(DecodedEntity::Viewport)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_CAMERA => camera::decode(cursor, version)
            .map(DecodedEntity::Camera)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_SUN => sun::decode(cursor, version)
            .map(DecodedEntity::Sun)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_LIGHT => light::decode(cursor, version)
            .map(DecodedEntity::Light)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_GEODATA => geodata::decode(cursor, version)
            .map(DecodedEntity::GeoData)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_SHAPE => return DecodedEntity::Unhandled { type_code, kind },
        // DIMENSION family per ODA §5 Table 4:
        //   0x14 ORDINATE, 0x15 LINEAR, 0x16 ALIGNED, 0x17 ANG_3PT,
        //   0x18 ANG_2LN, 0x19 RADIUS, 0x1A DIAMETER.
        OBJECT_TYPE_DIMENSION_MIN..=OBJECT_TYPE_DIMENSION_MAX => {
            match dimension::DimensionKind::from_object_type_code(type_code) {
                Some(dk) => dimension::decode(cursor, version, dk)
                    .map(DecodedEntity::Dimension)
                    .map_err(|e| e.to_string()),
                None => return DecodedEntity::Unhandled { type_code, kind },
            }
        }
        // IMAGE and MLEADER are custom classes (AcDb:Classes lookup) —
        // their codes vary per-file, so they're handled in the custom-
        // class dispatch pass (see task #96) not here.
        _ => return DecodedEntity::Unhandled { type_code, kind },
    };

    match result {
        Ok(entity) => entity,
        Err(message) => DecodedEntity::Error {
            type_code,
            kind,
            message,
        },
    }
}

/// Summary of a dispatch run — honest bookkeeping for the README +
/// CLI tools, so callers can report "decoded N / skipped M / errored K"
/// instead of pretending every object succeeded.
/// Cap on the number of error messages retained in a
/// [`DispatchSummary`]. Beyond this point, the count is still tracked
/// via [`DispatchSummary::errored`] but the message strings are
/// discarded and [`DispatchSummary::errors_suppressed`] increments.
/// This prevents unbounded `String` allocation on adversarial files.
pub const MAX_RETAINED_ERRORS: usize = 1_000;

#[derive(Debug, Default, Clone)]
pub struct DispatchSummary {
    pub decoded: usize,
    pub unhandled: usize,
    pub errored: usize,
    /// First [`MAX_RETAINED_ERRORS`] error (type_code, message) pairs.
    /// Past that, only the count is kept.
    pub errors: Vec<(u16, String)>,
    /// Count of error messages dropped after the retention cap.
    pub errors_suppressed: usize,
}

impl DispatchSummary {
    /// Tallies one dispatch outcome, retaining up to `MAX_RETAINED_ERRORS` error messages.
    pub fn record(&mut self, decoded: &DecodedEntity) {
        match decoded {
            DecodedEntity::Unhandled { .. } => self.unhandled += 1,
            DecodedEntity::Error {
                type_code, message, ..
            } => {
                self.errored += 1;
                if self.errors.len() < MAX_RETAINED_ERRORS {
                    self.errors.push((*type_code, message.clone()));
                } else {
                    self.errors_suppressed += 1;
                }
            }
            _ => self.decoded += 1,
        }
    }

    /// Total entities seen: decoded + unhandled + errored.
    pub fn total(&self) -> usize {
        self.decoded + self.unhandled + self.errored
    }

    /// Fraction of seen entities that decoded, or 0.0 when nothing was seen.
    pub fn decoded_ratio(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            0.0
        } else {
            self.decoded as f64 / total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A non-entity type this crate has no self-validating decoder for
    /// stays `Unhandled` — it must never be fed to an entity decoder,
    /// and must never be counted as decoded.
    #[test]
    fn unhandled_for_non_entity_type_without_a_decoder() {
        let raw = RawObject {
            stream_offset: 0,
            size_bytes: 0,
            type_code: 0x51,
            kind: ObjectType::VbaProject, // non-entity, no decoder
            handle: crate::bitcursor::Handle {
                code: 0,
                counter: 0,
                value: 0,
            },
            raw: Vec::new(),
            obj_size_bits: None,
        };
        let decoded = decode_from_raw(&raw, Version::R2018);
        assert!(matches!(decoded, DecodedEntity::Unhandled { .. }));
        assert!(!decoded.is_decoded());
    }

    /// LAYOUT now has a self-validating decoder, so an empty payload
    /// reaches it and fails there rather than being waved through as
    /// `Unhandled`.
    #[test]
    fn empty_layout_payload_errors_rather_than_decoding() {
        let raw = RawObject {
            stream_offset: 0,
            size_bytes: 0,
            type_code: 0x52,
            kind: ObjectType::Layout,
            handle: crate::bitcursor::Handle {
                code: 0,
                counter: 0,
                value: 0,
            },
            raw: Vec::new(),
            obj_size_bits: None,
        };
        let decoded = decode_from_raw(&raw, Version::R2018);
        assert!(matches!(decoded, DecodedEntity::Error { .. }));
        assert!(!decoded.is_decoded());
    }

    /// A release band whose LAYOUT layout is not determined comes back
    /// `Unhandled`, never `Error` — "not determined" is not a broken
    /// record.
    #[test]
    fn undetermined_layout_release_is_unhandled_not_error() {
        let raw = RawObject {
            stream_offset: 0,
            size_bytes: 0,
            type_code: 0x52,
            kind: ObjectType::Layout,
            handle: crate::bitcursor::Handle {
                code: 0,
                counter: 0,
                value: 0,
            },
            raw: vec![0u8; 32],
            obj_size_bits: None,
        };
        let decoded = decode_from_raw(&raw, Version::R2000);
        assert!(matches!(decoded, DecodedEntity::Unhandled { .. }));
    }

    /// A DICTIONARY with no bytes reaches the object decoder and fails
    /// there — an honest `Error`, not a silent `Unhandled` and not a
    /// fabricated empty dictionary.
    #[test]
    fn empty_dictionary_payload_errors_rather_than_decoding() {
        let raw = RawObject {
            stream_offset: 0,
            size_bytes: 0,
            type_code: OBJECT_TYPE_DICTIONARY,
            kind: ObjectType::Dictionary,
            handle: crate::bitcursor::Handle {
                code: 0,
                counter: 0,
                value: 0,
            },
            raw: Vec::new(),
            obj_size_bits: None,
        };
        let decoded = decode_from_raw(&raw, Version::R2018);
        assert!(matches!(decoded, DecodedEntity::Error { .. }));
        assert!(!decoded.is_decoded());
    }

    /// Build an R2004 (`AC1018`) object record from bits and walk it
    /// through the public dispatcher.
    ///
    /// §19.1 puts an `RL` "object data size in bits" between the object
    /// type and the object handle for the whole R2000..R2007 band.
    /// Skipping it — as the reader did for every version but R2000 —
    /// left the handle and everything after it 32 bits out of phase,
    /// which is what produced the `Bit cursor exhausted` failures on
    /// every AC1018 sample.
    fn build_r2004_line_record(obj_size_bits: u32) -> Vec<u8> {
        let mut w = crate::bitwriter::BitWriter::new();
        w.write_bs_u(OBJECT_TYPE_LINE); // OT object type
        w.write_rl(obj_size_bits); // RL object data size in bits
        w.write_handle(0, 0x83); // H object handle
        // -- common entity preamble (§19.4.1), R2004 shape --
        w.write_bs_u(0); // EED terminator
        w.write_b(false); // no graphics
        w.write_bb(0b10); // entmode = InBlock
        w.write_bl(0); // num_reactors
        w.write_b(true); // xdictionary missing (R2004+)
        w.write_bs(0); // CMC colour — BYLAYER
        w.write_bd(1.0); // linetype scale
        w.write_bb(0b00); // ltype flags
        w.write_bb(0b00); // plotstyle flags
        w.write_bs(0); // invisibility
        w.write_rc(0x1D); // lineweight
        // -- LINE body (§19.4.20) --
        w.write_b(true); // 2D
        w.write_rd(50.0);
        w.write_dd(50.0, 100.0);
        w.write_rd(50.0);
        w.write_dd(50.0, 100.0);
        w.write_b(true); // thickness default
        w.write_b(true); // extrusion default
        w.into_bytes()
    }

    #[test]
    fn r2004_line_record_decodes_and_ends_on_obj_size() {
        // First pass with a placeholder to learn the true data-stream
        // length, then rebuild with it so the record is self-consistent.
        let probe = build_r2004_line_record(0);
        let mut c = BitCursor::new(&probe);
        c.read_bs_u().unwrap();
        c.read_rl().unwrap();
        c.read_handle().unwrap();
        crate::common_entity::read_common_entity_data(&mut c, Version::R2004).unwrap();
        line::decode(&mut c).unwrap();
        let obj_size_bits = c.position_bits() as u32;

        let bytes = build_r2004_line_record(obj_size_bits);
        let raw = RawObject {
            stream_offset: 0,
            size_bytes: bytes.len() as u32,
            type_code: OBJECT_TYPE_LINE,
            kind: ObjectType::Line,
            handle: crate::bitcursor::Handle {
                code: 0,
                counter: 1,
                value: 0x83,
            },
            raw: bytes,
            obj_size_bits: Some(obj_size_bits),
        };

        match decode_from_raw(&raw, Version::R2004) {
            DecodedEntity::Line(l) => {
                assert!(l.is_2d);
                assert_eq!(l.start.x, 50.0);
                assert_eq!(l.start.y, 50.0);
                assert_eq!(l.end.x, 100.0);
                assert_eq!(l.end.y, 100.0);
            }
            other => panic!("expected a LINE, got {other:?}"),
        }

        // Without the RL the handle is misread and the walk overruns.
        // The counter-nibble gate (#51) now rejects the misread handle
        // one field earlier than the entity body used to; either way
        // the misaligned read must not yield a LINE.
        let mut misaligned = BitCursor::new(&raw.raw);
        misaligned.read_bs_u().unwrap();
        let misaligned_result = misaligned.read_handle().and_then(|_| {
            crate::common_entity::read_common_entity_data(&mut misaligned, Version::R2004)
                .and_then(|_| line::decode(&mut misaligned))
        });
        assert!(
            misaligned_result.is_err(),
            "skipping the RL obj_size must not decode as a valid LINE"
        );
    }

    /// The non-entity common object data (§19.4.2) is `EED + BL
    /// num_reactors + B xdic-missing` on R2004 — three fields, not two.
    #[test]
    fn r2004_common_object_data_consumes_reactor_count() {
        let mut w = crate::bitwriter::BitWriter::new();
        w.write_bs_u(0); // EED terminator
        w.write_bl(2); // num_reactors
        w.write_b(true); // xdictionary missing (R2004+)
        w.write_bs_u(4); // TV name length
        for b in b"ACAD" {
            w.write_rc(*b);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let reactors =
            crate::common_entity::read_common_object_data(&mut c, Version::R2004).expect("prefix");
        assert_eq!(reactors, 2);
        assert_eq!(
            crate::tables::read_tv(&mut c, Version::R2004).unwrap(),
            "ACAD"
        );
    }

    #[test]
    fn summary_ratio_zero_on_empty() {
        let s = DispatchSummary::default();
        assert_eq!(s.decoded_ratio(), 0.0);
        assert_eq!(s.total(), 0);
    }

    #[test]
    fn summary_tracks_counts() {
        let mut s = DispatchSummary::default();
        s.record(&DecodedEntity::Unhandled {
            type_code: 100,
            kind: ObjectType::Dictionary,
        });
        s.record(&DecodedEntity::Error {
            type_code: 19,
            kind: ObjectType::Line,
            message: "test".into(),
        });
        assert_eq!(s.decoded, 0);
        assert_eq!(s.unhandled, 1);
        assert_eq!(s.errored, 1);
        assert_eq!(s.total(), 2);
    }
}
