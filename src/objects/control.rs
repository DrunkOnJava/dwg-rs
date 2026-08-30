//! Control objects (§19.5.1..§19.5.10) — table owners.
//!
//! A `*_CONTROL` object owns all entries in its corresponding
//! symbol table:
//!
//! | Control type    | Owned entries                     | Spec |
//! |-----------------|-----------------------------------|------|
//! | BLOCK_CONTROL   | BLOCK_HEADER (block definitions)  | §19.5.1 |
//! | LAYER_CONTROL   | LAYER                             | §19.5.2 |
//! | STYLE_CONTROL   | STYLE                             | §19.5.3 |
//! | LTYPE_CONTROL   | LTYPE                             | §19.5.4 |
//! | VIEW_CONTROL    | VIEW                              | §19.5.5 |
//! | UCS_CONTROL     | UCS                               | §19.5.6 |
//! | VPORT_CONTROL   | VPORT                             | §19.5.7 |
//! | APPID_CONTROL   | APPID                             | §19.5.8 |
//! | DIMSTYLE_CONTROL| DIMSTYLE                          | §19.5.9 |
//! | VP_ENT_HDR_CONTROL | VIEWPORT_ENTITY_HEADER         | §19.5.10 |
//!
//! Nine of the ten have the same one-field body:
//!
//! ```text
//! BL  num_entries
//! ```
//!
//! The handles to the owned entries live *after* the body and are
//! collected by the generic object-handle reader; this decoder only
//! reads the count.
//!
//! # Measured: DIMSTYLE_CONTROL carries one extra `RC`
//!
//! DIMSTYLE_CONTROL — and only DIMSTYLE_CONTROL — spends eight more
//! bits after `num_entries`. Evidence, reading from the end of the
//! common object data to each record's data-stream boundary:
//!
//! | File | Record | Budget | `BL num_entries` | Left over |
//! |---|---|---|---|---|
//! | `arc_2004.dwg` | DIMSTYLE_CONTROL | 18 bits | 10 bits (3) | 8 bits (`RC` = 1) |
//! | `arc_2013.dwg` | DIMSTYLE_CONTROL | 19 bits | 10 bits (3) | 8 bits (`RC` = 1) + trailer |
//! | `sample_AC1032.dwg` | DIMSTYLE_CONTROL | 19 bits | 10 bits (6) | 8 bits (`RC` = 4) + trailer |
//! | same files | the other nine controls | 2-11 bits | 2-10 bits | 0 bits (+ trailer) |
//!
//! What the byte means is not determinable from three observations of
//! two distinct values, so it is surfaced verbatim as
//! [`Control::dimstyle_trailing_rc`] rather than named after a guess.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::object_type::ObjectType;
use crate::objects::modern;
use crate::version::Version;

/// A control object — holds the entry count. The actual entry handles
/// are attached via the object's handle-reference list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Control {
    pub num_entries: u32,
    /// The extra `RC` DIMSTYLE_CONTROL records carry after
    /// `num_entries`; `None` for the other nine control types. See the
    /// module docs for the measurement.
    pub dimstyle_trailing_rc: Option<u8>,
}

/// Decodes the `Control` payload that follows the common object header.
///
/// This reads the nine-of-ten shape; for DIMSTYLE_CONTROL use
/// [`decode_dimstyle`], which also consumes the measured trailing `RC`.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Control> {
    let num_entries = c.read_bl()? as u32;
    Ok(Control {
        num_entries,
        dimstyle_trailing_rc: None,
    })
}

/// Decodes a DIMSTYLE_CONTROL payload — `BL num_entries` plus the
/// measured trailing `RC` (see the module docs).
pub fn decode_dimstyle(c: &mut BitCursor<'_>) -> Result<Control> {
    let num_entries = c.read_bl()? as u32;
    let dimstyle_trailing_rc = Some(c.read_rc()?);
    Ok(Control {
        num_entries,
        dimstyle_trailing_rc,
    })
}

/// Decode a `*_CONTROL` object straight from its raw payload, checking
/// its data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
    kind: ObjectType,
) -> Result<Control> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = if kind == ObjectType::DimStyleControl {
        decode_dimstyle(&mut split.data)?
    } else {
        decode(&mut split.data)?
    };
    split.finish(kind.short_label())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_layer_control() {
        let mut w = BitWriter::new();
        w.write_bl(42);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ctrl = decode(&mut c).unwrap();
        assert_eq!(ctrl.num_entries, 42);
        assert_eq!(ctrl.dimstyle_trailing_rc, None);
    }

    #[test]
    fn roundtrip_empty_control() {
        let mut w = BitWriter::new();
        w.write_bl(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ctrl = decode(&mut c).unwrap();
        assert_eq!(ctrl.num_entries, 0);
    }

    #[test]
    fn dimstyle_control_reads_the_trailing_rc() {
        let mut w = BitWriter::new();
        w.write_bl(6);
        w.write_rc(4);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ctrl = decode_dimstyle(&mut c).unwrap();
        assert_eq!(ctrl.num_entries, 6);
        assert_eq!(ctrl.dimstyle_trailing_rc, Some(4));
    }

    #[test]
    fn r2018_control_closes_one_bit_before_the_handle_stream() {
        let mut body = modern::tests::r2018_object_prefix(0);
        body.write_bl(2);
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = modern::tests::build_payload_without_strings(&bits);
        let ctrl =
            decode_object(&payload, 8, None, Version::R2018, ObjectType::LayerControl).unwrap();
        assert_eq!(ctrl.num_entries, 2);
    }

    #[test]
    fn r2018_dimstyle_control_takes_the_extra_byte() {
        let mut body = modern::tests::r2018_object_prefix(0);
        body.write_bl(6);
        body.write_rc(4);
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = modern::tests::build_payload_without_strings(&bits);
        let ctrl = decode_object(
            &payload,
            8,
            None,
            Version::R2018,
            ObjectType::DimStyleControl,
        )
        .unwrap();
        assert_eq!(ctrl.num_entries, 6);
        assert_eq!(ctrl.dimstyle_trailing_rc, Some(4));
        // The generic shape must reject the same bytes — proof the
        // extra RC is load-bearing, not decoration.
        assert!(
            decode_object(&payload, 8, None, Version::R2018, ObjectType::LayerControl).is_err()
        );
    }

    #[test]
    fn r2004_control_closes_on_the_inline_object_size() {
        let mut w = BitWriter::new();
        w.write_bs_u(0); // EED terminator
        w.write_bl(0); // num_reactors
        w.write_b(true); // no xdictionary
        w.write_bl(3); // num_entries
        let end = w.position_bits();
        let bytes = w.into_bytes();
        let ctrl = decode_object(
            &bytes,
            0,
            Some(end),
            Version::R2004,
            ObjectType::BlockControl,
        )
        .unwrap();
        assert_eq!(ctrl.num_entries, 3);
    }
}
