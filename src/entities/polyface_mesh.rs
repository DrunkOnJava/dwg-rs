//! POLYFACE_MESH entity (§19.4.29) — legacy face-list 3D mesh.
//!
//! Predates both ACIS solids and the R2010 subdivision [`super::mesh::Mesh`].
//! A POLYFACE_MESH header holds only the counts and the endpoints of a
//! handle chain — the actual vertex and face records live in separate
//! `VERTEX_PFACE` / `VERTEX_PFACE_FACE` sub-entities whose traversal is
//! a downstream concern (see [`crate::entities::vertex::Vertex`]).
//!
//! The `m_vert_count` / `n_vert_count` naming is preserved verbatim from
//! the ODA spec, which is regrettably misleading: in POLYFACE_MESH the
//! two fields are **vertex count** and **face count**, not the M×N grid
//! dimensions they imply in [`super::polygon_mesh::PolygonMesh`]. The
//! doc-comments call this out explicitly to keep spec-matching code
//! out of the naming trap.
//!
//! # Stream shape (all supported versions, L4-35)
//!
//! ```text
//! BS   flags
//! BS   m_vert_count          -- vertices in the mesh
//! BS   n_vert_count          -- faces in the mesh (see note above)
//! BS   m_density             -- approximation density (carried for
//!                                shape compatibility with POLYGON_MESH;
//!                                ignored for POLYFACE semantics)
//! BS   n_density             -- ditto
//! H    first_vertex_handle   -- head of the VERTEX_PFACE chain
//! H    last_vertex_handle    -- tail of the VERTEX_PFACE chain
//! ```

use crate::bitcursor::{BitCursor, Handle};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct PolyfaceMesh {
    /// Flag bits per §19.4.29 (closed M / closed N / face-type bits).
    /// Kept verbatim so round-trip writers can re-emit the same form.
    pub flags: u16,
    /// Number of vertices in the mesh. Despite the spec name (`m_vert_count`)
    /// this is a linear vertex count — the mesh is not a grid.
    pub vertex_count: u16,
    /// Number of faces in the mesh (`n_vert_count` in ODA naming).
    pub face_count: u16,
    /// Approximation density, M direction. Semantically meaningful only
    /// for POLYGON_MESH; carried here for shape compatibility.
    pub m_density: u16,
    /// Approximation density, N direction. See [`Self::m_density`].
    pub n_density: u16,
    /// First VERTEX_PFACE / VERTEX_PFACE_FACE sub-entity in the handle chain.
    pub first_vertex_handle: Handle,
    /// Last VERTEX_PFACE / VERTEX_PFACE_FACE sub-entity in the handle chain.
    pub last_vertex_handle: Handle,
}

/// Decode a POLYFACE_MESH header.
///
/// Only the header is parsed here — the vertex handle chain between
/// `first_vertex_handle` and `last_vertex_handle` is walked at the
/// object-stream layer, not by this decoder.
pub fn decode(c: &mut BitCursor<'_>) -> Result<PolyfaceMesh> {
    let flags = c.read_bs_u()?;
    let vertex_count = c.read_bs_u()?;
    let face_count = c.read_bs_u()?;
    let m_density = c.read_bs_u()?;
    let n_density = c.read_bs_u()?;
    let first_vertex_handle = c.read_handle()?;
    let last_vertex_handle = c.read_handle()?;
    Ok(PolyfaceMesh {
        flags,
        vertex_count,
        face_count,
        m_density,
        n_density,
        first_vertex_handle,
        last_vertex_handle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    struct PfaceFields {
        flags: u16,
        vc: u16,
        fc: u16,
        md: u16,
        nd: u16,
        first: (u8, u64),
        last: (u8, u64),
    }

    fn write_pface(w: &mut BitWriter, f: &PfaceFields) {
        w.write_bs_u(f.flags);
        w.write_bs_u(f.vc);
        w.write_bs_u(f.fc);
        w.write_bs_u(f.md);
        w.write_bs_u(f.nd);
        w.write_handle(f.first.0, f.first.1);
        w.write_handle(f.last.0, f.last.1);
    }

    #[test]
    fn roundtrip_minimal_header() {
        let mut w = BitWriter::new();
        write_pface(
            &mut w,
            &PfaceFields {
                flags: 0,
                vc: 8,
                fc: 6,
                md: 0,
                nd: 0,
                first: (3, 0x100),
                last: (3, 0x110),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c).unwrap();
        assert_eq!(p.flags, 0);
        assert_eq!(p.vertex_count, 8);
        assert_eq!(p.face_count, 6);
        assert_eq!(p.m_density, 0);
        assert_eq!(p.n_density, 0);
        assert_eq!(p.first_vertex_handle.code, 3);
        assert_eq!(p.first_vertex_handle.value, 0x100);
        assert_eq!(p.last_vertex_handle.value, 0x110);
    }

    #[test]
    fn roundtrip_nonzero_flags_and_densities() {
        let mut w = BitWriter::new();
        // flags bit 0x40 is the POLYFACE-MESH indicator in related
        // POLYLINE flags; any nonzero mask round-trips here.
        write_pface(
            &mut w,
            &PfaceFields {
                flags: 0x0040,
                vc: 24,
                fc: 32,
                md: 4,
                nd: 6,
                first: (3, 0xAB),
                last: (3, 0xCD),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c).unwrap();
        assert_eq!(p.flags, 0x0040);
        assert_eq!(p.vertex_count, 24);
        assert_eq!(p.face_count, 32);
        assert_eq!(p.m_density, 4);
        assert_eq!(p.n_density, 6);
        assert_eq!(p.first_vertex_handle.value, 0xAB);
        assert_eq!(p.last_vertex_handle.value, 0xCD);
    }

    #[test]
    fn roundtrip_zero_handles() {
        // A handle with value 0 encodes with counter = 0 (no payload
        // bytes). Important edge case — round-tripping zero-valued
        // handles is how empty meshes are expressed.
        let mut w = BitWriter::new();
        write_pface(
            &mut w,
            &PfaceFields {
                flags: 0,
                vc: 0,
                fc: 0,
                md: 0,
                nd: 0,
                first: (5, 0),
                last: (5, 0),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c).unwrap();
        assert_eq!(p.first_vertex_handle.counter, 0);
        assert_eq!(p.first_vertex_handle.value, 0);
        assert_eq!(p.last_vertex_handle.counter, 0);
        assert_eq!(p.last_vertex_handle.value, 0);
    }
}
