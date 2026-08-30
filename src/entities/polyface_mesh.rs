//! POLYLINE_PFACE entity (§19.4.29) — legacy face-list 3D mesh header.
//!
//! Predates both ACIS solids and the R2010 subdivision [`super::mesh::Mesh`].
//! A POLYLINE_PFACE header holds only counts — the actual vertex and
//! face records live in separate `VERTEX_PFACE` /
//! `VERTEX_PFACE_FACE` sub-entities the object stream owns (see
//! [`crate::entities::vertex`]), reached through the record's handle
//! stream rather than through any field decoded here.
//!
//! # Stream shape — measured
//!
//! ```text
//! BS   vertex_count           -- VERTEX_PFACE sub-entities
//! BS   face_count             -- VERTEX_PFACE_FACE sub-entities
//! BL   num_owned_objects      -- R2004+ only
//! ```
//!
//! The single POLYLINE_PFACE record of `sample_AC1032.dwg`
//! (handle `0x422`) has a 30-bit data-field budget, and that reading is
//! the one that lands on it exactly — `delta 0` with
//! `examples/probe_entity_field_list.rs`. Its three values are `5`, `2`
//! and `7`, and the same file carries exactly **5** `VERTEX_PFACE`
//! records and **2** `VERTEX_PFACE_FACE` records, whose sum is 7. So
//! the counts are corroborated by the object stream itself, not just by
//! the bit budget.
//!
//! Two earlier readings are corrected here. The header used to claim
//! five `BS` count/density fields followed by two inline `H` handles;
//! the handles never sit in the data stream (they live in the record's
//! handle stream, which is what the boundary marks the start of), and
//! the density pair belongs to POLYGON_MESH, not to a face-list mesh.
//! Together those two mistakes overran the boundary by 52 bits.
//!
//! `BS` and `BL` are indistinguishable at this value — both spend
//! `01` plus one byte for a count below 256 — so the third field is
//! read as the `BL num_owned_objects` that
//! [`crate::tables::block_record`] already reads on R2004+ for the
//! same purpose, rather than as a third count. Only its width and
//! value are claimed by the corpus.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolyfaceMesh {
    /// Number of `VERTEX_PFACE` sub-entities the mesh owns.
    pub vertex_count: u16,
    /// Number of `VERTEX_PFACE_FACE` sub-entities the mesh owns.
    pub face_count: u16,
    /// R2004+ owned-object count — the sub-entities reachable through
    /// this record's handle stream. `None` before R2004, where the
    /// field is absent.
    pub num_owned_objects: Option<u32>,
}

/// Decode a POLYLINE_PFACE header from a cursor parked past the common
/// entity preamble.
///
/// Only the header is parsed here — the `VERTEX_PFACE` /
/// `VERTEX_PFACE_FACE` sub-entities are separate records in the object
/// stream, walked at the object-stream layer.
pub fn decode_record(c: &mut BitCursor<'_>, version: Version) -> Result<PolyfaceMesh> {
    let vertex_count = c.read_bs_u()?;
    let face_count = c.read_bs_u()?;
    let num_owned_objects = if version.is_r2004_plus() {
        Some(c.read_bl()? as u32)
    } else {
        None
    };
    Ok(PolyfaceMesh {
        vertex_count,
        face_count,
        num_owned_objects,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// The corpus record's own values: 5 vertices, 2 faces, 7 owned
    /// objects, in exactly 30 bits.
    #[test]
    fn roundtrip_measured_r2018_header() {
        let mut w = BitWriter::new();
        w.write_bs_u(5);
        w.write_bs_u(2);
        w.write_bl(7);
        assert_eq!(w.position_bits(), 30);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode_record(&mut c, Version::R2018).unwrap();
        assert_eq!(p.vertex_count, 5);
        assert_eq!(p.face_count, 2);
        assert_eq!(p.num_owned_objects, Some(7));
        assert_eq!(c.position_bits(), 30);
    }

    /// Before R2004 the owned-object count is absent, so the same two
    /// counts close 10 bits earlier.
    #[test]
    fn r2000_header_has_no_owned_count() {
        let mut w = BitWriter::new();
        w.write_bs_u(8);
        w.write_bs_u(6);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode_record(&mut c, Version::R2000).unwrap();
        assert_eq!(p.vertex_count, 8);
        assert_eq!(p.face_count, 6);
        assert_eq!(p.num_owned_objects, None);
        assert_eq!(c.position_bits(), 20);
    }
}
