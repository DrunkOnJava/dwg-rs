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
//! - Non-entity objects (DICTIONARY, XRECORD, `*_CONTROL`, symbol-table
//!   entries, or any `Custom(N)` type resolved through the class map)
//!   are returned as [`DecodedEntity::Unhandled`] with their raw type
//!   code. Downstream callers can run [`crate::tables`] or
//!   [`crate::objects`] decoders on them as needed.
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
    geodata, hatch, helix, image, insert, leader, light, line, lofted_surface, lwpolyline, mleader,
    mtext, point, polyline, ray, revolved_surface, solid, spline, sun, swept_surface, text,
    three_d_face, trace, vertex, viewport, xline,
};
use crate::error::{Error, Result};
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
    Dimension(dimension::Dimension),
    Leader(leader::Leader),
    Image(image::Image),
    Hatch(hatch::Hatch),
    MLeader(mleader::MLeader),
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
    // Symbol-table entries — not drawing entities but worth
    // surfacing as typed variants for callers that iterate
    // DecodedEntity over the whole object stream.
    Layer(crate::tables::layer::Layer),
    Ltype(crate::tables::ltype::LType),
    Style(crate::tables::style::Style),
    View(crate::tables::view::View),
    Ucs(crate::tables::ucs::Ucs),
    VPort(crate::tables::vport::VPort),
    AppId(crate::tables::appid::AppId),
    DimStyle(crate::tables::dimstyle::DimStyle),
    BlockRecord(crate::tables::block_record::BlockRecord),
    /// Object type this dispatcher doesn't decode (control objects,
    /// dictionaries, unknown custom classes). The raw bytes remain
    /// accessible on the originating [`RawObject`].
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
            Self::Layer(_) => 0x33,
            Self::Ltype(_) => 0x39,
            Self::Style(_) => 0x35,
            Self::View(_) => 0x3D,
            Self::Ucs(_) => 0x3F,
            Self::VPort(_) => 0x41,
            Self::AppId(_) => 0x43,
            Self::DimStyle(_) => 0x45,
            Self::BlockRecord(_) => 0x31,
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
const OBJECT_TYPE_LWPOLYLINE: u16 = 0x4D; // 77
const OBJECT_TYPE_HATCH: u16 = 0x4E; // 78
// CAMERA / SUN / LIGHT / GEODATA are visual/scene entities introduced
// in R2007+ (GEODATA in R2010+). Per the spec they appear in the
// AcDb:Classes table per-file, but every observed modern drawing uses
// the codes below when present outside the dynamic range, so they are
// wired as fixed codes here. If a file assigns a different class
// index, custom-class dispatch via decode_from_raw_with_class_map
// is the correct path.
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
        "MULTILEADER" | "MLEADER" => mleader::decode(&mut cursor)
            .map(DecodedEntity::MLeader)
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

    // Short-circuit objects that are neither drawing entities NOR
    // symbol-table entries (DICTIONARY, XRECORD, control objects,
    // unknown custom classes).
    if !raw.is_entity() && !kind.is_table_entry() {
        return DecodedEntity::Unhandled { type_code, kind };
    }

    match position_cursor_at_entity_body(raw, version) {
        Ok(mut cursor) => {
            if kind.is_table_entry() {
                dispatch_table_entry(&mut cursor, type_code, kind, version)
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

/// Dispatch a symbol-table entry to its per-type decoder.
///
/// Each table decoder internally calls
/// [`crate::tables::read_table_entry_header`] for the shared
/// table-entry preamble, so the cursor positioning here is the same
/// as for drawing entities.
fn dispatch_table_entry(
    c: &mut BitCursor<'_>,
    type_code: u16,
    kind: ObjectType,
    version: Version,
) -> DecodedEntity {
    let result: core::result::Result<DecodedEntity, String> = match kind {
        ObjectType::Layer => crate::tables::layer::decode(c, version)
            .map(DecodedEntity::Layer)
            .map_err(|e| e.to_string()),
        ObjectType::Ltype => crate::tables::ltype::decode(c, version)
            .map(DecodedEntity::Ltype)
            .map_err(|e| e.to_string()),
        ObjectType::Style => crate::tables::style::decode(c, version)
            .map(DecodedEntity::Style)
            .map_err(|e| e.to_string()),
        ObjectType::View => crate::tables::view::decode(c, version)
            .map(DecodedEntity::View)
            .map_err(|e| e.to_string()),
        ObjectType::Ucs => crate::tables::ucs::decode(c, version)
            .map(DecodedEntity::Ucs)
            .map_err(|e| e.to_string()),
        ObjectType::Vport => crate::tables::vport::decode(c, version)
            .map(DecodedEntity::VPort)
            .map_err(|e| e.to_string()),
        ObjectType::AppId => crate::tables::appid::decode(c, version)
            .map(DecodedEntity::AppId)
            .map_err(|e| e.to_string()),
        ObjectType::DimStyle => crate::tables::dimstyle::decode_partial(c, version)
            .map(DecodedEntity::DimStyle)
            .map_err(|e| e.to_string()),
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
    let mut cursor = BitCursor::new(&raw.raw);

    if version.is_r2010_plus() {
        // MC — handle-stream-size-in-bits; byte-aligned, throwaway.
        read_mc_unsigned(&mut cursor)?;
    }

    // Type code encoding depends on version. The walker already parsed
    // this; we need to re-consume exactly the same number of bits.
    crate::object::read_object_type(&mut cursor, version)?;

    if matches!(version, Version::R2000) {
        // R2000 only — 32-bit object-size-in-bits field.
        cursor.read_rl()?;
    }

    // Handle (4 bits code + 4 bits counter + counter bytes).
    let _ = cursor.read_handle()?;

    Ok(cursor)
}

/// Exact inverse of `object::read_mc_unsigned` — duplicated rather
/// than imported because the walker's version is private to that
/// module. Consumes a byte-aligned modular char as unsigned.
fn read_mc_unsigned(cursor: &mut BitCursor<'_>) -> Result<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for _ in 0..10 {
        let b = cursor.read_rc()? as u64;
        value |= (b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift >= 64 {
            break;
        }
    }
    Err(Error::SectionMap("MC length exceeded 10 bytes".into()))
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
        OBJECT_TYPE_INSERT => insert::decode(cursor)
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
        OBJECT_TYPE_LWPOLYLINE => lwpolyline::decode(cursor)
            .map(DecodedEntity::LwPolyline)
            .map_err(|e| e.to_string()),
        OBJECT_TYPE_LEADER => leader::decode(cursor)
            .map(DecodedEntity::Leader)
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

    pub fn total(&self) -> usize {
        self.decoded + self.unhandled + self.errored
    }

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

    #[test]
    fn unhandled_for_non_entity_type() {
        let raw = RawObject {
            stream_offset: 0,
            size_bytes: 0,
            type_code: 42,
            kind: ObjectType::Dictionary, // non-entity
            handle: crate::bitcursor::Handle {
                code: 0,
                counter: 0,
                value: 0,
            },
            raw: Vec::new(),
        };
        let decoded = decode_from_raw(&raw, Version::R2018);
        assert!(matches!(decoded, DecodedEntity::Unhandled { .. }));
        assert!(!decoded.is_decoded());
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
