//! BODY entity (§20.4.41) — generic ACIS body (non-solid, non-region).
//!
//! BODY shares one record shape with 3DSOLID and REGION: an ACIS
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

/// BODY entity — §20.4.41.
///
/// See [`crate::entities::three_d_solid::ThreeDSolid`] for the meaning
/// of each field; the encoding is identical.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
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

/// Decode a BODY **envelope** per ODA spec v5.4.1 §20.4.41.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Body> {
    let (acis_empty, version, sat_blob) = read_sat_blob(c)?;
    Ok(Body {
        acis_empty,
        version,
        sat_blob,
        in_data_store: false,
        tail: None,
    })
}

/// Decode a whole BODY record — envelope plus the §20.4.41 fields that
/// follow it.
pub fn decode_record(c: &mut BitCursor<'_>, version: Version, in_data_store: bool) -> Result<Body> {
    let record = modeler::decode_record(c, version, in_data_store)?;
    Ok(Body {
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
        w.write_b(false); // §20.4.41 unknown bit
        w.write_bs_u(1);
        w.write_bl(payload.len() as i32);
        for x in payload {
            w.write_rc(*x);
        }
        w.write_bl(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode(&mut c).unwrap();
        assert!(!b.acis_empty);
        assert_eq!(b.version, Some(1));
        assert_eq!(b.sat_blob.as_deref(), Some(&payload[..]));
    }

    /// The R2013+ data-store shape: no inline envelope, a wireframe
    /// point, `num isolines = 4` and the 16-byte revision GUID.
    #[test]
    fn roundtrip_body_data_store_record() {
        let guid = [0xCDu8; 16];
        let mut w = BitWriter::new();
        w.write_b(true); // wireframe data present
        w.write_b(false); // point present
        w.write_bl(4); // num isolines
        w.write_b(false); // isolines present
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
        let b = decode_record(&mut c, Version::R2018, true).unwrap();
        assert!(b.in_data_store);
        let fields = b.tail.unwrap();
        assert_eq!(fields.num_isolines, 4);
        assert!(!fields.isolines_present);
        assert_eq!(fields.revision_guid, Some(guid));
        assert_eq!(c.position_bits(), end);
    }
}
