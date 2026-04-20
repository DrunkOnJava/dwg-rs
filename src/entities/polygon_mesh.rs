//! POLYGON_MESH entity (§19.4.30) — legacy M×N indexed polygon mesh.
//!
//! A POLYGON_MESH header stores the (M, N) dimensions of an indexed
//! surface patch plus closed-direction flags. The vertex grid itself
//! lives in a handle chain of `VERTEX_MESH` sub-entities referenced by
//! `first_vertex_handle` .. `last_vertex_handle`; the
//! traversal is a downstream concern, not handled here.
//!
//! POLYGON_MESH and [`super::polyface_mesh::PolyfaceMesh`] are sibling
//! legacy 3D representations — polygon-mesh is indexed (parametric M×N
//! grid, common for lofted surfaces), polyface-mesh is face-list
//! (explicit face → vertex-index tuples). The stream shapes are very
//! similar, but the field ordering differs by one swap: POLYGON_MESH
//! writes the density pair **before** the dimension pair, while
//! POLYFACE_MESH writes the dimensions first. This module keeps the
//! spec-mandated order.
//!
//! # Stream shape (all supported versions, L4-36)
//!
//! ```text
//! BS   flags          -- bit 0 = closed in M, bit 1 = closed in N,
//!                        higher bits preserved verbatim for round-trip
//! BS   m_density      -- approximation density (display-only)
//! BS   n_density
//! BS   m_vert_count   -- grid dimension M
//! BS   n_vert_count   -- grid dimension N
//! H    first_vertex_handle
//! H    last_vertex_handle
//! ```

use crate::bitcursor::{BitCursor, Handle};
use crate::error::Result;

/// Flag bits (§19.4.30). Named constants for the documented bits; any
/// value outside the documented set round-trips verbatim.
pub mod flag_bits {
    /// Mesh is closed in the M direction — the last M-column
    /// wraps back to the first.
    pub const CLOSED_M: u16 = 0x0001;
    /// Mesh is closed in the N direction.
    pub const CLOSED_N: u16 = 0x0002;
}

#[derive(Debug, Clone, PartialEq)]
pub struct PolygonMesh {
    pub flags: u16,
    /// Approximation density, M direction — display-only.
    pub m_density: u16,
    /// Approximation density, N direction — display-only.
    pub n_density: u16,
    /// Grid dimension along the M axis.
    pub m_vert_count: u16,
    /// Grid dimension along the N axis.
    pub n_vert_count: u16,
    pub first_vertex_handle: Handle,
    pub last_vertex_handle: Handle,
}

impl PolygonMesh {
    /// Is the mesh closed in the M direction? (Spec flag bit 0.)
    pub fn is_closed_m(&self) -> bool {
        self.flags & flag_bits::CLOSED_M != 0
    }
    /// Is the mesh closed in the N direction? (Spec flag bit 1.)
    pub fn is_closed_n(&self) -> bool {
        self.flags & flag_bits::CLOSED_N != 0
    }
}

/// Decode a POLYGON_MESH header.
///
/// Only the header is parsed here — the VERTEX_MESH grid referenced by
/// the handle chain is walked at the object-stream layer.
pub fn decode(c: &mut BitCursor<'_>) -> Result<PolygonMesh> {
    let flags = c.read_bs_u()?;
    let m_density = c.read_bs_u()?;
    let n_density = c.read_bs_u()?;
    let m_vert_count = c.read_bs_u()?;
    let n_vert_count = c.read_bs_u()?;
    let first_vertex_handle = c.read_handle()?;
    let last_vertex_handle = c.read_handle()?;
    Ok(PolygonMesh {
        flags,
        m_density,
        n_density,
        m_vert_count,
        n_vert_count,
        first_vertex_handle,
        last_vertex_handle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    struct PmeshFields {
        flags: u16,
        md: u16,
        nd: u16,
        m: u16,
        n: u16,
        first: (u8, u64),
        last: (u8, u64),
    }

    fn write_pmesh(w: &mut BitWriter, f: &PmeshFields) {
        w.write_bs_u(f.flags);
        w.write_bs_u(f.md);
        w.write_bs_u(f.nd);
        w.write_bs_u(f.m);
        w.write_bs_u(f.n);
        w.write_handle(f.first.0, f.first.1);
        w.write_handle(f.last.0, f.last.1);
    }

    #[test]
    fn roundtrip_open_mesh() {
        let mut w = BitWriter::new();
        write_pmesh(
            &mut w,
            &PmeshFields {
                flags: 0,
                md: 6,
                nd: 6,
                m: 5,
                n: 4,
                first: (3, 0x200),
                last: (3, 0x214),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c).unwrap();
        assert_eq!(m.flags, 0);
        assert!(!m.is_closed_m());
        assert!(!m.is_closed_n());
        assert_eq!(m.m_density, 6);
        assert_eq!(m.n_density, 6);
        assert_eq!(m.m_vert_count, 5);
        assert_eq!(m.n_vert_count, 4);
        assert_eq!(m.first_vertex_handle.value, 0x200);
        assert_eq!(m.last_vertex_handle.value, 0x214);
    }

    #[test]
    fn roundtrip_cylinder_closed_in_m() {
        let mut w = BitWriter::new();
        write_pmesh(
            &mut w,
            &PmeshFields {
                flags: flag_bits::CLOSED_M,
                md: 8,
                nd: 4,
                m: 16,
                n: 8,
                first: (3, 0x300),
                last: (3, 0x310),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c).unwrap();
        assert!(m.is_closed_m());
        assert!(!m.is_closed_n());
        assert_eq!(m.m_vert_count, 16);
        assert_eq!(m.n_vert_count, 8);
    }

    #[test]
    fn roundtrip_torus_closed_both() {
        let mut w = BitWriter::new();
        write_pmesh(
            &mut w,
            &PmeshFields {
                flags: flag_bits::CLOSED_M | flag_bits::CLOSED_N,
                md: 8,
                nd: 8,
                m: 24,
                n: 12,
                first: (3, 0x400),
                last: (3, 0x41C),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c).unwrap();
        assert!(m.is_closed_m());
        assert!(m.is_closed_n());
        assert_eq!(m.flags & 0x3, 0x3);
        assert_eq!(m.first_vertex_handle.value, 0x400);
        assert_eq!(m.last_vertex_handle.value, 0x41C);
    }

    #[test]
    fn preserves_unknown_high_flag_bits() {
        // Files in the wild sometimes carry extra bits we don't
        // semantically interpret; round-tripping them verbatim is how
        // the reader stays honest with unknown metadata.
        let mut w = BitWriter::new();
        write_pmesh(
            &mut w,
            &PmeshFields {
                flags: 0x8000,
                md: 2,
                nd: 3,
                m: 7,
                n: 5,
                first: (3, 1),
                last: (3, 2),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c).unwrap();
        assert_eq!(m.flags, 0x8000);
        assert!(!m.is_closed_m());
        assert!(!m.is_closed_n());
    }
}
