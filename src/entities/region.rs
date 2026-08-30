//! REGION entity (§20.4.41) — 2D ACIS region (planar bounded face).
//!
//! REGION shares one record shape with 3DSOLID and BODY: an ACIS
//! envelope, the wireframe / isoline / silhouette block, a second
//! ACIS-empty bit, the R2007+ trailing `BL` and — on an R2013+ record
//! whose payload lives in the data-storage section — the measured
//! revision-GUID block. See [`crate::entities::three_d_solid`] for the
//! envelope and [`crate::entities::modeler`] for the rest of the
//! record, its evidence and the defensive caps.
//!
//! The same [`crate::entities::three_d_solid::MAX_SAT_BLOB_BYTES`]
//! 32 MiB ceiling applies here.

use crate::bitcursor::BitCursor;
use crate::entities::modeler::{self, ModelerTail};
use crate::entities::three_d_solid::read_sat_blob;
use crate::error::Result;
use crate::version::Version;

/// REGION entity — §20.4.41.
///
/// See [`crate::entities::three_d_solid::ThreeDSolid`] for the meaning
/// of each field; the encoding is identical.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// `true` when the entity was written with no inline body
    /// (`ACIS empty` bit set).
    pub acis_empty: bool,
    /// ACIS format version reported before the block loop. `None`
    /// when `acis_empty` is `true`.
    pub version: Option<u16>,
    /// Concatenated SAT payload bytes, or `None` when `acis_empty`.
    pub sat_blob: Option<Vec<u8>>,
    /// R2013+: the SAB stream is a data record in the data-storage
    /// section (§24) rather than inline.
    pub in_data_store: bool,
    /// The §20.4.41 fields that follow the envelope. `None` from the
    /// envelope-only [`decode`] entry point.
    pub tail: Option<ModelerTail>,
}

/// Decode a REGION **envelope** per ODA spec v5.4.1 §20.4.41.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Region> {
    let (acis_empty, version, sat_blob) = read_sat_blob(c)?;
    Ok(Region {
        acis_empty,
        version,
        sat_blob,
        in_data_store: false,
        tail: None,
    })
}

/// Decode a whole REGION record — envelope plus the §20.4.41 fields
/// that follow it.
pub fn decode_record(
    c: &mut BitCursor<'_>,
    version: Version,
    in_data_store: bool,
) -> Result<Region> {
    let record = modeler::decode_record(c, version, in_data_store)?;
    Ok(Region {
        acis_empty: record.blob.empty,
        version: (!record.blob.empty).then_some(record.blob.version),
        sat_blob: (!record.blob.empty).then_some(record.blob.bytes),
        in_data_store: record.in_data_store,
        tail: Some(record.tail),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_empty_region() {
        let mut w = BitWriter::new();
        w.write_b(true); // ACIS empty
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let r = decode(&mut c).unwrap();
        assert!(r.acis_empty);
        assert_eq!(r.version, None);
        assert_eq!(r.sat_blob, None);
    }

    #[test]
    fn roundtrip_region_with_blob() {
        let payload = b"400 0 1 0";
        let mut w = BitWriter::new();
        w.write_b(false);
        w.write_b(false); // §20.4.41 unknown bit
        w.write_bs_u(1);
        w.write_bl(payload.len() as i32);
        for b in payload {
            w.write_rc(*b);
        }
        w.write_bl(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let r = decode(&mut c).unwrap();
        assert!(!r.acis_empty);
        assert_eq!(r.version, Some(1));
        assert_eq!(r.sat_blob.as_deref(), Some(&payload[..]));
    }

    /// The R2013+ data-store shape: no inline envelope, a wireframe
    /// point, `num isolines = 4` and the 16-byte revision GUID.
    #[test]
    fn roundtrip_region_data_store_record() {
        let guid = [0xABu8; 16];
        let mut w = BitWriter::new();
        w.write_b(true); // wireframe data present
        w.write_b(true); // point present
        w.write_bd(1.0);
        w.write_bd(2.0);
        w.write_bd(0.0);
        w.write_bl(4); // num isolines
        w.write_b(true); // isolines present
        w.write_bl(0); // num wires
        w.write_bl(0); // num silhouettes
        w.write_b(true); // ACIS empty (2)
        w.write_bl(0); // R2007+ unknown
        w.write_b(true);
        w.write_bb(0);
        w.write_bb(0);
        w.write_bb(0b10);
        for b in guid {
            w.write_rc(b);
        }
        w.write_bl(0);
        let end = w.position_bits();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let r = decode_record(&mut c, Version::R2018, true).unwrap();
        assert!(r.in_data_store);
        assert!(r.acis_empty);
        let fields = r.tail.unwrap();
        assert_eq!(fields.num_isolines, 4);
        assert_eq!(fields.revision_guid, Some(guid));
        assert_eq!(c.position_bits(), end);
    }
}
