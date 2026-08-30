//! SEQEND entity (§19.4.6) — closes the sub-entity list an INSERT or a
//! POLYLINE opens, exactly as [`super::endblk::EndBlk`] closes a
//! BLOCK's.
//!
//! # Stream shape — measured
//!
//! The record has no type-specific field list at all. All four SEQEND
//! records of `sample_AC1032.dwg` (handles `0x42A`, `0x431`, `0x706`,
//! `0x79E`) have a data-field budget of **zero** bits between the end
//! of the common entity preamble and the record's data-stream boundary
//! — `examples/probe_entity_budgets.rs` prints `budget=0` for every one
//! of them. Two of the four sit immediately after a vertex run
//! (`0x42A` closes the POLYLINE_PFACE at `0x422`, `0x431` closes the
//! POLYLINE_3D at `0x42B`), which is where the spec puts a SEQEND.
//!
//! Before this module the type came back `Unhandled`; it is decoded
//! now because the boundary check can prove the empty field list right
//! rather than merely assume it.

use crate::bitcursor::BitCursor;
use crate::error::Result;

/// SEQEND carries no fields of its own — its owner and its place in the
/// sub-entity chain are handle-stream data, not data-stream data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeqEnd;

/// Decodes a `SeqEnd`; the entity has no payload past the common header.
pub fn decode(_c: &mut BitCursor<'_>) -> Result<SeqEnd> {
    Ok(SeqEnd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seqend_consumes_nothing() {
        let buf = [0xFFu8; 4];
        let mut c = BitCursor::new(&buf);
        let _ = decode(&mut c).unwrap();
        assert_eq!(c.position_bits(), 0);
    }
}
