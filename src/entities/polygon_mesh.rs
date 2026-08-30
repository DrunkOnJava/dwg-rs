//! POLYGON_MESH entity (§19.4.30) — legacy M×N indexed polygon mesh.
//!
//! A POLYGON_MESH header stores the (M, N) dimensions of an indexed
//! surface patch plus closed-direction flags. The vertex grid itself
//! lives in a chain of `VERTEX_MESH` sub-entities the record owns
//! through its handle stream; the traversal is a downstream concern,
//! not handled here.
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
//! ```
//!
//! # Not verified against real bytes
//!
//! No file in the corpus carries a POLYGON_MESH record, so unlike its
//! sibling [`super::polyface_mesh`] — whose list is pinned to the one
//! record of `sample_AC1032.dwg` — this field list has never been
//! landed on a data-stream boundary. It is wired through the same
//! exact-boundary check as every other entity (#63), so the first real
//! record to reach it will either confirm the list or report a delta;
//! it will not silently return plausible-looking numbers.
//!
//! Two `H` reads were removed from the list in #63. An object
//! reference never occupies data-stream bits from R2000 onward — the
//! handle stream begins exactly where the boundary this decoder is
//! checked against ends — so reading the vertex-chain endpoints inline
//! was wrong on every release the crate walks. That was measurable on
//! POLYLINE_PFACE, whose identical mistake overran its boundary by 52
//! bits; it is corrected here by the same rule rather than by a
//! measurement of this type.

use crate::bitcursor::BitCursor;
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
/// Only the header is parsed here — the VERTEX_MESH grid the record
/// owns is reached through its handle stream, which begins where this
/// decoder's data fields must end.
pub fn decode(c: &mut BitCursor<'_>) -> Result<PolygonMesh> {
    let flags = c.read_bs_u()?;
    let m_density = c.read_bs_u()?;
    let n_density = c.read_bs_u()?;
    let m_vert_count = c.read_bs_u()?;
    let n_vert_count = c.read_bs_u()?;
    Ok(PolygonMesh {
        flags,
        m_density,
        n_density,
        m_vert_count,
        n_vert_count,
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
    }

    fn write_pmesh(w: &mut BitWriter, f: &PmeshFields) {
        w.write_bs_u(f.flags);
        w.write_bs_u(f.md);
        w.write_bs_u(f.nd);
        w.write_bs_u(f.m);
        w.write_bs_u(f.n);
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
            },
        );
        let end = w.position_bits();
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
        // The five counts are the whole data-stream field list — the
        // vertex-chain handles are not in it.
        assert_eq!(c.position_bits(), end);
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
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c).unwrap();
        assert!(m.is_closed_m());
        assert!(m.is_closed_n());
        assert_eq!(m.flags & 0x3, 0x3);
        assert_eq!(m.m_vert_count, 24);
        assert_eq!(m.n_vert_count, 12);
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
