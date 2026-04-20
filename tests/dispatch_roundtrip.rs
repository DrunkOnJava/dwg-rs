//! Integration tests and property tests for `entities::decode_from_raw`.
//!
//! The existing unit tests in `src/entities/*.rs` verify that each
//! per-entity decoder works against synthetic bit streams. They do
//! NOT verify that the dispatcher wires those decoders to the right
//! type codes. This test module fills that gap with two layers:
//!
//! 1. **Integration** — build a minimal synthetic `RawObject` with a
//!    zeroed payload for each fixed entity type code, dispatch it,
//!    and assert the result variant matches (or fails gracefully,
//!    never panics, never goes to `Unhandled`).
//! 2. **Property** — proptest over arbitrary `(type_code, byte_count,
//!    payload_seed)` and assert no panics across any input.

use dwg::ObjectType;
use dwg::Version;
use dwg::bitcursor::Handle;
use dwg::entities::{DecodedEntity, decode_from_raw};
use dwg::object::RawObject;
use proptest::prelude::*;

fn make_raw(type_code: u16, payload: Vec<u8>) -> RawObject {
    RawObject {
        stream_offset: 0,
        size_bytes: payload.len() as u32,
        type_code,
        kind: ObjectType::from_code(type_code),
        handle: Handle {
            code: 0,
            counter: 0,
            value: 0,
        },
        raw: payload,
    }
}

/// For each fixed entity code, a synthetic RawObject with a 256-byte
/// zeroed payload dispatches to the corresponding typed variant OR
/// returns DecodedEntity::Error (never Unhandled, never panics).
#[test]
fn integration_dispatch_routes_every_entity_code() {
    let entity_codes = [
        0x01u16, // TEXT
        0x02,    // ATTRIB
        0x03,    // ATTDEF
        0x04,    // BLOCK
        0x05,    // ENDBLK
        0x07,    // INSERT
        0x0A,    // VERTEX_2D
        0x0F,    // POLYLINE_2D
        0x11,    // ARC
        0x12,    // CIRCLE
        0x13,    // LINE
        0x14,    // DIMENSION (ORDINATE)
        0x15,    // DIMENSION (LINEAR)
        0x16,    // DIMENSION (ALIGNED)
        0x17,    // DIMENSION (ANG 3-Pt)
        0x18,    // DIMENSION (ANG 2-Ln)
        0x19,    // DIMENSION (RADIUS)
        0x1A,    // DIMENSION (DIAMETER)
        0x1B,    // POINT
        0x1C,    // 3DFACE
        0x1F,    // SOLID
        0x20,    // TRACE
        0x22,    // VIEWPORT
        0x23,    // ELLIPSE
        0x24,    // SPLINE
        0x28,    // RAY
        0x29,    // XLINE
        0x2C,    // MTEXT
        0x2D,    // LEADER
        0x4D,    // LWPOLYLINE
        0x4E,    // HATCH
    ];
    for &code in &entity_codes {
        let raw = make_raw(code, vec![0u8; 256]);
        let decoded = decode_from_raw(&raw, Version::R2018);
        assert!(
            !matches!(decoded, DecodedEntity::Unhandled { .. }),
            "type 0x{code:02X} was routed to Unhandled — the dispatcher \
             does not recognize it as an entity (likely an off-by-one \
             in dispatch.rs; see task #71 for how this class of bug \
             manifested before)"
        );
    }
}

/// LINE is the most common real entity. Build a plausible-looking
/// LINE payload that should survive the common-entity preamble + the
/// LINE type-specific decoder without erroring (approximately).
///
/// This asserts only that the dispatcher returns a `Line` variant —
/// the actual coordinate values on synthetic zeros are not meaningful.
/// What we're pinning is that `dispatch_from_raw` for type=0x13 calls
/// `line::decode`, not (say) `circle::decode`.
#[test]
fn integration_line_dispatch_returns_line_variant() {
    let code = 0x13u16;
    let raw = make_raw(code, vec![0u8; 512]);
    let decoded = decode_from_raw(&raw, Version::R2018);
    match decoded {
        DecodedEntity::Line(_) => { /* correct */ }
        DecodedEntity::Error { .. } => { /* acceptable — synthetic bytes don't decode cleanly */ }
        other => panic!(
            "type 0x13 (LINE) dispatched to wrong variant: {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// CIRCLE dispatch lands on `Circle`, not `Arc`. Regression test for
/// the pre-fix bug where OBJECT_TYPE_CIRCLE = 17 / OBJECT_TYPE_ARC = 18
/// were swapped.
#[test]
fn integration_circle_dispatch_is_not_arc() {
    let raw = make_raw(0x12, vec![0u8; 512]);
    let decoded = decode_from_raw(&raw, Version::R2018);
    match decoded {
        DecodedEntity::Circle(_) | DecodedEntity::Error { .. } => { /* correct */ }
        DecodedEntity::Arc(_) => panic!(
            "type 0x12 (CIRCLE) dispatched to ARC variant — \
             OBJECT_TYPE_CIRCLE and OBJECT_TYPE_ARC are swapped again"
        ),
        other => panic!(
            "type 0x12 (CIRCLE) dispatched to unexpected variant: {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// ARC dispatch lands on `Arc`, not `Circle`. See above.
#[test]
fn integration_arc_dispatch_is_not_circle() {
    let raw = make_raw(0x11, vec![0u8; 512]);
    let decoded = decode_from_raw(&raw, Version::R2018);
    match decoded {
        DecodedEntity::Arc(_) | DecodedEntity::Error { .. } => { /* correct */ }
        DecodedEntity::Circle(_) => {
            panic!("type 0x11 (ARC) dispatched to CIRCLE variant — swap regression")
        }
        other => panic!(
            "type 0x11 (ARC) dispatched to unexpected variant: {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// DIMENSION (LINEAR) at code 0x15 dispatches to `Dimension` with the
/// `Linear` subtype. Regression against the pre-fix off-by-one where
/// 0x15 went to `Ordinate`.
#[test]
fn integration_linear_dimension_subtype() {
    use dwg::entities::dimension::{Dimension, DimensionKind};
    let raw = make_raw(0x15, vec![0u8; 512]);
    let decoded = decode_from_raw(&raw, Version::R2018);
    match decoded {
        DecodedEntity::Dimension(Dimension::Linear(_)) => { /* correct */ }
        DecodedEntity::Dimension(Dimension::Ordinate(_)) => panic!(
            "type 0x15 (LINEAR) dispatched to Ordinate subtype — \
             DimensionKind off-by-one regressed; see task #71"
        ),
        DecodedEntity::Error { .. } => { /* acceptable */ }
        other => panic!(
            "type 0x15 (LINEAR DIMENSION) dispatched to unexpected variant: {:?}",
            std::mem::discriminant(&other)
        ),
    }
    // Also exercise the kind resolver directly.
    assert_eq!(
        DimensionKind::from_object_type_code(0x15),
        Some(DimensionKind::Linear)
    );
}

/// POINT at code 0x1B dispatches to `Point`. Previously, when the
/// dimension range was mistyped as 21..=27, code 27 (POINT) fell into
/// the dimension match arm and was (mis-)dispatched as
/// `Dimension::Diameter`. Regression test.
#[test]
fn integration_point_is_not_dimension_diameter() {
    let raw = make_raw(0x1B, vec![0u8; 512]);
    let decoded = decode_from_raw(&raw, Version::R2018);
    match decoded {
        DecodedEntity::Point(_) | DecodedEntity::Error { .. } => { /* correct */ }
        DecodedEntity::Dimension(_) => panic!(
            "type 0x1B (POINT) dispatched to a Dimension variant — \
             the dimension range in dispatch.rs overlaps POINT again"
        ),
        other => panic!(
            "type 0x1B (POINT) dispatched to unexpected variant: {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

// ================================================================
// Property tests — fuzzing the dispatcher
// ================================================================

proptest! {
    /// decode_from_raw must never panic on any combination of
    /// (type_code, payload) — it must return some variant, even if
    /// the decoder errored or the code is unknown.
    #[test]
    fn property_dispatch_never_panics(
        type_code in 0u16..0x200,
        payload in prop::collection::vec(any::<u8>(), 0..=1024)
    ) {
        let raw = make_raw(type_code, payload);
        let _ = decode_from_raw(&raw, Version::R2018);
        // If we got here without panicking, success.
    }

    /// For every code the dispatcher claims to handle as an entity,
    /// random bytes never turn the result into `Unhandled`. It should
    /// either decode or Error — but never silently skip.
    #[test]
    fn property_entity_codes_never_return_unhandled(
        payload in prop::collection::vec(any::<u8>(), 64..=512)
    ) {
        // Fixed entity codes from the dispatcher's match arms.
        for &code in &[
            0x01u16, 0x02, 0x03, 0x04, 0x05, 0x07, 0x0A, 0x0F, 0x11, 0x12, 0x13,
            0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1F, 0x20,
            0x22, 0x23, 0x24, 0x28, 0x29, 0x2C, 0x2D, 0x4D, 0x4E,
        ] {
            let raw = make_raw(code, payload.clone());
            let decoded = decode_from_raw(&raw, Version::R2018);
            prop_assert!(
                !matches!(decoded, DecodedEntity::Unhandled { .. }),
                "type 0x{:02X} routed to Unhandled on random input", code
            );
        }
    }

    /// Non-entity codes (DICTIONARY, XRECORD, LAYER, etc.) must
    /// always return Unhandled — never an Error (which would mean the
    /// dispatcher ran a per-entity decoder on non-entity bytes).
    #[test]
    fn property_non_entity_codes_always_unhandled(
        payload in prop::collection::vec(any::<u8>(), 0..=256)
    ) {
        for &code in &[
            0x2Au16, // DICTIONARY
            0x32,    // LAYER_CONTROL
            0x33,    // LAYER
            0x38,    // LTYPE_CONTROL
            0x4F,    // XRECORD
            0x52,    // LAYOUT
        ] {
            let raw = make_raw(code, payload.clone());
            let decoded = decode_from_raw(&raw, Version::R2018);
            prop_assert!(
                matches!(decoded, DecodedEntity::Unhandled { .. }),
                "type 0x{:02X} (non-entity) routed to a typed variant", code
            );
        }
    }
}
