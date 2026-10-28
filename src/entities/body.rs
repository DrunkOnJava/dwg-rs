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
