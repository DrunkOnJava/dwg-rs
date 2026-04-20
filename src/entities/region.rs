//! REGION entity (§19.4.43) — 2D ACIS region (planar bounded face).
//!
//! REGION shares an identical on-wire encoding with 3DSOLID (§19.4.42)
//! and BODY (§19.4.44): an `acis_empty` flag, a version field, and a
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

/// REGION entity — §19.4.43.
///
/// See [`crate::entities::three_d_solid::ThreeDSolid`] for the meaning
/// of each field; the encoding is identical.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// `true` when the entity was written with no body attached
    /// (`acis_empty` bit set).
    pub acis_empty: bool,
    /// ACIS format version reported before the chunk loop. `None`
    /// when `acis_empty` is `true`.
    pub version: Option<u16>,
    /// Concatenated SAT payload bytes, or `None` when `acis_empty`.
    pub sat_blob: Option<Vec<u8>>,
}

/// Decode a REGION entity per ODA spec v5.4.1 §19.4.43.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Region> {
    let (acis_empty, version, sat_blob) = read_sat_blob(c)?;
    Ok(Region {
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
    fn roundtrip_empty_region() {
        let mut w = BitWriter::new();
        w.write_b(true); // acis_empty
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let r = decode(&mut c).unwrap();
        assert!(r.acis_empty);
        assert_eq!(r.version, None);
        assert_eq!(r.sat_blob, None);
    }

    #[test]
    fn roundtrip_region_with_blob() {
        let payload = b"REGION_SAT_DATA";
        let mut w = BitWriter::new();
        w.write_b(false);
        w.write_bs_u(700);
        w.write_b(true);
        w.write_bl(payload.len() as i32);
        for b in payload {
            w.write_rc(*b);
        }
        w.write_b(false);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let r = decode(&mut c).unwrap();
        assert!(!r.acis_empty);
        assert_eq!(r.version, Some(700));
        assert_eq!(r.sat_blob.as_deref(), Some(&payload[..]));
    }
}
