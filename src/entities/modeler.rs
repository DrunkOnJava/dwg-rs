//! Shared helpers for ACIS-backed modeler entities (3DSOLID, BODY,
//! REGION, and the SURFACE family) — ODA Open Design Specification
//! v5.4.1 §19.4.78 and siblings.
//!
//! Autodesk stores parametric solids and surfaces as an opaque ACIS
//! SAT (Standard ACIS Text) byte stream wrapped in a thin DWG-level
//! envelope. The low-level wire decoder lives in
//! `crate::entities::three_d_solid::read_sat_blob`; this module
//! offers a small typed wrapper that the four SURFACE variants use
//! to keep their own structs uncluttered.
//!
//! The re-export keeps the SAT wire shape defined in exactly one
//! place — if the shape ever needs to evolve (e.g., an R2013 variant
//! with an extra flag), only `three_d_solid` needs to change and
//! every SURFACE picks up the new behaviour automatically.

use crate::bitcursor::BitCursor;
use crate::entities::three_d_solid;
use crate::error::Result;

/// Re-export of [`three_d_solid::MAX_SAT_BLOB_BYTES`] so SURFACE
/// decoders can reference a stable symbol without reaching into the
/// 3DSOLID module.
pub const MAX_SAT_BYTES: usize = three_d_solid::MAX_SAT_BLOB_BYTES;

/// ACIS envelope data decoded from the stream.
///
/// `bytes` is the concatenation of every non-terminator chunk. If
/// `empty` is true, the drawing stored no SAT payload for this
/// entity (a legitimate state — some procedural surfaces retain only
/// their parametric definition, not a cached ACIS body).
#[derive(Debug, Clone, PartialEq)]
pub struct SatBlob {
    /// Was the `acis_empty` flag set? If so, no payload followed.
    pub empty: bool,
    /// ACIS format version (e.g. `70` for ACIS 7.0). Undefined when
    /// `empty == true`.
    pub version: u16,
    /// Raw concatenated SAT bytes. May be XOR-masked per the ACIS
    /// format rules; this helper does not demask.
    pub bytes: Vec<u8>,
}

/// Decode one ACIS envelope by delegating to
/// `three_d_solid::read_sat_blob` and adapting the tuple result
/// into the [`SatBlob`] struct.
pub fn decode_sat_blob(c: &mut BitCursor<'_>) -> Result<SatBlob> {
    let (empty, version, bytes) = three_d_solid::read_sat_blob(c)?;
    Ok(SatBlob {
        empty,
        version: version.unwrap_or(0),
        bytes: bytes.unwrap_or_default(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Encode a SAT blob in the same wire shape the underlying
    /// `three_d_solid` decoder expects. Keeps the per-entity tests
    /// focused on their own fields without re-stating the chunk loop
    /// each time.
    pub(crate) fn write_sat_blob(w: &mut BitWriter, blob: &SatBlob) {
        if blob.empty {
            w.write_b(true);
            return;
        }
        w.write_b(false);
        w.write_bs_u(blob.version);
        if !blob.bytes.is_empty() {
            w.write_b(true); // has_more_blocks
            w.write_bl(blob.bytes.len() as i32);
            for b in &blob.bytes {
                w.write_rc(*b);
            }
        }
        w.write_b(false); // no more blocks
    }

    #[test]
    fn roundtrip_empty_blob() {
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: true,
                version: 0,
                bytes: Vec::new(),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode_sat_blob(&mut c).unwrap();
        assert!(b.empty);
        assert!(b.bytes.is_empty());
    }

    #[test]
    fn roundtrip_one_chunk_blob() {
        let payload = b"ACIS DUMMY BODY DATA".to_vec();
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: false,
                version: 70,
                bytes: payload.clone(),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode_sat_blob(&mut c).unwrap();
        assert!(!b.empty);
        assert_eq!(b.version, 70);
        assert_eq!(b.bytes, payload);
    }
}
