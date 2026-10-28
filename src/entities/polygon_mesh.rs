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
//! (explicit face → vertex-index tuples).
//!
//! # Stream shape (§20.4.34 POLYLINE (MESH) (30))
//!
//! ```text
//! BS   flags          -- DXF 70, kept verbatim
//! BS   curve_type     -- DXF 75: smooth surface type
//! BS   m_vert_count   -- DXF 71: grid dimension M
//! BS   n_vert_count   -- DXF 72: grid dimension N
//! BS   m_density      -- DXF 73: surface density, M direction
//! BS   n_density      -- DXF 74
//! BL   owned_count    -- R2004+: objects the record owns
//! ```
//!
//! That is §20.4.34 as printed, and its own example (§20.4.34.1, an
//! R13/R14 record) closes on its `RL` boundary with it: flags 17, curve
//! type 0, a 6 × 7 grid, densities 0. An
//! earlier reading had no curve type and put the densities before the
//! dimensions, which ends two bits short there and reads N as 0 (#273;
//! `tests/fixtures/spec-examples/r14-objects.tsv`). No corpus file holds a
//! POLYGON_MESH, so that example is the list's only measurement.
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
use crate::version::Version;

/// Flag bits (§20.4.34). Named constants for the documented bits; any
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
    /// Smooth surface type (DXF 75).
    pub curve_type: u16,
    /// Approximation density, M direction — display-only.
    pub m_density: u16,
    /// Approximation density, N direction — display-only.
    pub n_density: u16,
    /// Grid dimension along the M axis.
    pub m_vert_count: u16,
    /// Grid dimension along the N axis.
    pub n_vert_count: u16,
    /// R2004+ count of the objects the record owns (its VERTEX_MESH
    /// records and SEQEND); `None` before R2004.
    pub num_owned_objects: Option<u32>,
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
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<PolygonMesh> {
    let flags = c.read_bs_u()?;
    let curve_type = c.read_bs_u()?;
    let m_vert_count = c.read_bs_u()?;
    let n_vert_count = c.read_bs_u()?;
    let m_density = c.read_bs_u()?;
    let n_density = c.read_bs_u()?;
    let num_owned_objects = if version.is_r2004_plus() {
        Some(c.read_bl()? as u32)
    } else {
        None
    };
    Ok(PolygonMesh {
        flags,
        curve_type,
        m_density,
        n_density,
        m_vert_count,
        n_vert_count,
        num_owned_objects,
    })
}
