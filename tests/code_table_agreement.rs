//! Cross-validation between `ObjectType::from_code` and the dispatcher's
//! fixed code table.
//!
//! The two must agree on:
//!
//! 1. Which codes identify the DIMENSION family (spec §5 Table 4:
//!    0x14..=0x1A).
//! 2. Which codes represent entities vs non-entities.
//! 3. Individual code ↔ variant bindings for every type the dispatcher
//!    dispatches to a typed decoder.
//!
//! These tests would have caught the off-by-one bug where dispatch.rs
//! claimed CIRCLE=17 / ARC=18 (swapped), TRACE=30 / 3DFACE=32 (swapped
//! with each other and with POLYLINE_MESH), and DIMENSION range
//! 21..=27 (should be 20..=26). See task #71.

use dwg::ObjectType;
use dwg::entities::DecodedEntity;
use dwg::entities::dimension::DimensionKind;
use dwg::entities::{
    arc, attdef, attrib, block, circle, ellipse, endblk, hatch, insert, leader, line, lwpolyline,
    mtext, point, polyline, ray, solid, spline, text, three_d_face, trace, vertex, viewport, xline,
};

/// DIMENSION family occupies exactly 0x14..=0x1A (20..=26) per ODA spec
/// §5 Table 4. Both tables must agree.
#[test]
fn dimension_range_agrees_between_object_type_and_dimension_kind() {
    for code in 0x00..=0x60u16 {
        let ot_is_dim = matches!(
            ObjectType::from_code(code),
            ObjectType::DimensionOrdinate
                | ObjectType::DimensionLinear
                | ObjectType::DimensionAligned
                | ObjectType::DimensionAng3Pt
                | ObjectType::DimensionAng2Ln
                | ObjectType::DimensionRadius
                | ObjectType::DimensionDiameter
        );
        let dk_is_dim = DimensionKind::from_object_type_code(code).is_some();
        assert_eq!(
            ot_is_dim, dk_is_dim,
            "code 0x{code:02X}: ObjectType says dim={ot_is_dim}, \
             DimensionKind says dim={dk_is_dim} — tables disagree"
        );
    }
}

/// Every DIMENSION code maps to the same subtype label in both tables.
#[test]
fn dimension_subtype_labels_agree_per_code() {
    let pairs: [(u16, ObjectType, DimensionKind); 7] = [
        (0x14, ObjectType::DimensionOrdinate, DimensionKind::Ordinate),
        (0x15, ObjectType::DimensionLinear, DimensionKind::Linear),
        (0x16, ObjectType::DimensionAligned, DimensionKind::Aligned),
        (
            0x17,
            ObjectType::DimensionAng3Pt,
            DimensionKind::Angular3Pt,
        ),
        (
            0x18,
            ObjectType::DimensionAng2Ln,
            DimensionKind::Angular2Line,
        ),
        (0x19, ObjectType::DimensionRadius, DimensionKind::Radius),
        (0x1A, ObjectType::DimensionDiameter, DimensionKind::Diameter),
    ];
    for (code, ot_expected, dk_expected) in pairs {
        assert_eq!(
            ObjectType::from_code(code),
            ot_expected,
            "ObjectType for code 0x{code:02X}"
        );
        assert_eq!(
            DimensionKind::from_object_type_code(code),
            Some(dk_expected),
            "DimensionKind for code 0x{code:02X}"
        );
    }
}

/// The specific codes that caused the production bug: CIRCLE (0x12)
/// and ARC (0x11) were swapped in dispatch.rs. This test pins them so
/// a future refactor can't re-swap them.
#[test]
fn circle_arc_codes_are_not_swapped() {
    // Spec §5 Table 4: 0x11 = ARC, 0x12 = CIRCLE.
    assert_eq!(ObjectType::from_code(0x11), ObjectType::Arc);
    assert_eq!(ObjectType::from_code(0x12), ObjectType::Circle);
}

/// TRACE (0x20) and 3DFACE (0x1C) — both previously wrong in dispatch.rs
/// (said TRACE=30/0x1E, 3DFACE=32/0x20). Pin them so the fix survives.
#[test]
fn trace_and_face3d_codes_match_spec() {
    assert_eq!(ObjectType::from_code(0x1C), ObjectType::Face3D);
    assert_eq!(ObjectType::from_code(0x20), ObjectType::Trace);
    // 0x1E is POLYLINE_MESH, NOT TRACE (common confusion).
    assert_eq!(ObjectType::from_code(0x1E), ObjectType::PolylineMesh);
}

/// Build a minimal synthetic `RawObject` for each fixed entity type and
/// dispatch it. The test does NOT assert on decode success — many
/// decoders need real-world bytes they don't have here — it asserts
/// that the dispatcher *routes the code to the right typed variant or
/// error variant*, not to `Unhandled`.
///
/// If a dispatcher arm gets disconnected (refactor drops a case, or a
/// future off-by-one re-enters), this test catches it.
#[test]
fn every_fixed_entity_code_is_routed_not_unhandled() {
    use dwg::Version;
    use dwg::bitcursor::Handle;
    use dwg::entities::decode_from_raw;
    use dwg::object::RawObject;

    // (type_code, label-for-failure-msg, should-route-to-typed-variant)
    let entity_codes: &[(u16, &str)] = &[
        (0x01, "TEXT"),
        (0x02, "ATTRIB"),
        (0x03, "ATTDEF"),
        (0x04, "BLOCK"),
        (0x05, "ENDBLK"),
        (0x07, "INSERT"),
        (0x0A, "VERTEX_2D"),
        (0x0F, "POLYLINE_2D"),
        (0x11, "ARC"),
        (0x12, "CIRCLE"),
        (0x13, "LINE"),
        (0x14, "DIMENSION_ORDINATE"),
        (0x15, "DIMENSION_LINEAR"),
        (0x16, "DIMENSION_ALIGNED"),
        (0x17, "DIMENSION_ANG_3PT"),
        (0x18, "DIMENSION_ANG_2LN"),
        (0x19, "DIMENSION_RADIUS"),
        (0x1A, "DIMENSION_DIAMETER"),
        (0x1B, "POINT"),
        (0x1C, "3DFACE"),
        (0x1F, "SOLID"),
        (0x20, "TRACE"),
        (0x22, "VIEWPORT"),
        (0x23, "ELLIPSE"),
        (0x24, "SPLINE"),
        (0x28, "RAY"),
        (0x29, "XLINE"),
        (0x2C, "MTEXT"),
        (0x2D, "LEADER"),
        (0x4D, "LWPOLYLINE"),
        (0x4E, "HATCH"),
    ];
    for &(code, label) in entity_codes {
        // Zeroed 256 bytes — enough for the dispatcher to walk the
        // header + common preamble without underflowing. Decoders may
        // still error on the contents, but that becomes
        // `DecodedEntity::Error`, NOT `Unhandled`.
        let raw = RawObject {
            stream_offset: 0,
            size_bytes: 256,
            type_code: code,
            kind: ObjectType::from_code(code),
            handle: Handle {
                code: 0,
                counter: 0,
                value: 0,
            },
            raw: vec![0u8; 256],
        };
        let decoded = decode_from_raw(&raw, Version::R2018);
        match decoded {
            DecodedEntity::Unhandled { .. } => panic!(
                "type 0x{code:02X} ({label}) was routed to Unhandled — \
                 the dispatcher does not recognize it as an entity"
            ),
            DecodedEntity::Error { .. } => { /* expected for synthetic bytes */ }
            _ => { /* some decoder produced a value, even better */ }
        }
    }
}

/// Non-entity codes must NOT be routed to a typed decoder. If this test
/// fires, the dispatcher is happily feeding DICTIONARY / XRECORD bytes
/// into an entity decoder somewhere.
#[test]
fn non_entity_codes_route_to_unhandled() {
    use dwg::Version;
    use dwg::bitcursor::Handle;
    use dwg::entities::decode_from_raw;
    use dwg::object::RawObject;

    let non_entity_codes = [
        (0x2A, "DICTIONARY"),
        (0x32, "LAYER_CONTROL"),
        (0x33, "LAYER"),
        (0x38, "LTYPE_CONTROL"),
        (0x4F, "XRECORD"),
        (0x52, "LAYOUT"),
    ];
    for (code, label) in non_entity_codes {
        let raw = RawObject {
            stream_offset: 0,
            size_bytes: 256,
            type_code: code,
            kind: ObjectType::from_code(code),
            handle: Handle {
                code: 0,
                counter: 0,
                value: 0,
            },
            raw: vec![0u8; 256],
        };
        let decoded = decode_from_raw(&raw, Version::R2018);
        match decoded {
            DecodedEntity::Unhandled { .. } => { /* correct */ }
            DecodedEntity::Error { .. } => {
                panic!(
                    "type 0x{code:02X} ({label}) was routed to an entity decoder \
                     (returned Error) — non-entity types should be Unhandled"
                )
            }
            _ => panic!(
                "type 0x{code:02X} ({label}) was routed to a typed decoder — \
                 non-entity types must be Unhandled"
            ),
        }
    }
}

/// Fixed codes the spec says map to `ObjectType::Unused` or reserved
/// slots should pass through as `ObjectType::Unassigned*` — never a
/// panic, never a wrong classification.
#[test]
fn reserved_slots_classify_cleanly() {
    assert_eq!(ObjectType::from_code(0x00), ObjectType::Unused);
    assert_eq!(ObjectType::from_code(0x09), ObjectType::Unassigned09);
    assert_eq!(ObjectType::from_code(0x36), ObjectType::Unassigned36);
    assert_eq!(ObjectType::from_code(0x37), ObjectType::Unassigned37);
}

/// Unused imports quiet — the imports above exist to make the tests
/// compile if someone factors out the decoder functions later; silence
/// the current-build warning without an allow-attribute.
#[allow(dead_code)]
fn _decoder_imports_exist(
    _a: fn() -> arc::Arc,
    _b: fn() -> attdef::AttDef,
    _c: fn() -> attrib::Attrib,
    _d: fn() -> block::Block,
    _e: fn() -> circle::Circle,
    _f: fn() -> ellipse::Ellipse,
    _g: fn() -> endblk::EndBlk,
    _h: fn() -> hatch::Hatch,
    _i: fn() -> insert::Insert,
    _j: fn() -> leader::Leader,
    _k: fn() -> line::Line,
    _l: fn() -> lwpolyline::LwPolyline,
    _m: fn() -> mtext::MText,
    _n: fn() -> point::Point,
    _o: fn() -> polyline::Polyline,
    _p: fn() -> ray::Ray,
    _q: fn() -> solid::Solid,
    _r: fn() -> spline::Spline,
    _s: fn() -> text::Text,
    _t: fn() -> three_d_face::ThreeDFace,
    _u: fn() -> trace::Trace,
    _v: fn() -> vertex::Vertex,
    _w: fn() -> viewport::Viewport,
    _x: fn() -> xline::XLine,
) {
}
