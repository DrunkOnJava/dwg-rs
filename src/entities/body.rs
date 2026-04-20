//! BODY entity (§19.4.44) — generic ACIS body (non-solid, non-region).
//!
//! BODY shares an identical on-wire encoding with 3DSOLID (§19.4.42)
//! and REGION (§19.4.43): an `acis_empty` flag, a version field, and a
//! loop of length-prefixed payload chunks that concatenate to an
//! opaque Standard ACIS Text (SAT) blob. See
//! [`crate::entities::three_d_solid`] for the full stream layout,
//! defensive caps, and shared helper documentation.
//!
//! The same [`crate::entities::three_d_solid::MAX_SAT_BLOB_BYTES`]
//! 32 MiB ceiling applies here.

use crate::bitcursor::BitCursor;
use crate::entities::three_d_solid::read_sat_blob;
use crate::error::Result;

/// BODY entity — §19.4.44.
///
/// See [`crate::entities::three_d_solid::ThreeDSolid`] for the meaning
/// of each field; the encoding is identical.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// `true` when the entity was written with no body attached
    /// (`acis_empty` bit set).
    pub acis_empty: bool,
    /// ACIS format version reported before the chunk loop. `None`
    /// when `acis_empty` is `true`.
    pub version: Option<u16>,
    /// Concatenated SAT payload bytes, or `None` when `acis_empty`.
    pub sat_blob: Option<Vec<u8>>,
}

/// Decode a BODY entity per ODA spec v5.4.1 §19.4.44.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Body> {
    let (acis_empty, version, sat_blob) = read_sat_blob(c)?;
    Ok(Body {
        acis_empty,
        version,
        sat_blob,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_empty_body_entity() {
        let mut w = BitWriter::new();
        w.write_b(true);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode(&mut c).unwrap();
        assert!(b.acis_empty);
        assert_eq!(b.version, None);
        assert_eq!(b.sat_blob, None);
    }

    #[test]
    fn roundtrip_body_with_blob() {
        let payload = b"BODY_SAT_PAYLOAD_BYTES";
        let mut w = BitWriter::new();
        w.write_b(false);
        w.write_bs_u(70);
        w.write_b(true);
        w.write_bl(payload.len() as i32);
        for x in payload {
            w.write_rc(*x);
        }
        w.write_b(false);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode(&mut c).unwrap();
        assert!(!b.acis_empty);
        assert_eq!(b.version, Some(70));
        assert_eq!(b.sat_blob.as_deref(), Some(&payload[..]));
    }
}
