//! VERTEX entity (§19.4.55..§19.4.57) — single vertex of the legacy
//! POLYLINE entity. Newer drawings use LWPOLYLINE (one entity holds
//! all vertices). POLYLINE vertices come in three flavors:
//!
//! | Variant   | Type code (pre-R2010) | Used by |
//! |-----------|-----------------------|---------|
//! | VERTEX_2D | 10 (`0x0A`)           | 2D POLYLINE |
//! | VERTEX_3D | 11 (`0x0B`)           | 3D POLYLINE |
//! | VERTEX_MESH | 12..13              | PFACE MESH / POLYFACEMESH |
//!
//! # Stream shape (2D variant — the common one)
//!
//! ```text
//! RC   flag             -- bits: 0x01 extra vertex follows,
//!                          0x02 tangent present, 0x04 not used,
//!                          0x08 plinefit spline control point,
//!                          0x10 plinefit spline frame control,
//!                          0x20 3D polyline vertex,
//!                          0x40 3D polygon mesh vertex,
//!                          0x80 polyface mesh vertex
//! BD3  location
//! BD   start_width      -- negative: |start| is both widths, no end_width follows
//! BD   end_width        -- only if start_width >= 0
//! BD   bulge
//! BL   vertex_id        -- R2010+
//! BD   tangent_direction  -- every release
//! ```
//!
//! That is §20.4.11 as printed. Its own R14 example (§20.4.11.1, flags 0)
//! closes on its `RL` boundary only with the tangent present, which an
//! earlier reading made conditional on flag `0x02` (#272;
//! `tests/fixtures/spec-examples/r14-objects.tsv`).

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    pub flag: u8,
    pub location: Point3D,
    pub start_width: f64,
    pub end_width: f64,
    pub bulge: f64,
    pub vertex_id: Option<u32>,
    /// §20.4.11 `Tangent dir BD 50`: every decoded record carries it;
    /// `None` only on vertices this crate builds itself.
    pub tangent_direction: Option<f64>,
}

/// Decodes the `Vertex` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Vertex> {
    let flag = c.read_rc()?;
    let location = read_bd3(c)?;
    let start_width = c.read_bd()?;
    // §20.4.11: a negative start width stands for both widths, and then no
    // end width is stored.
    let (start_width, end_width) = if start_width < 0.0 {
        (-start_width, -start_width)
    } else {
        (start_width, c.read_bd()?)
    };
    let bulge = c.read_bd()?;
    let vertex_id = if version.is_r2010_plus() {
        Some(c.read_bl()? as u32)
    } else {
        None
    };
    let tangent_direction = Some(c.read_bd()?);
    Ok(Vertex {
        flag,
        location,
        start_width,
        end_width,
        bulge,
        vertex_id,
        tangent_direction,
    })
}

/// VERTEX_3D (0x0B), VERTEX_MESH (0x0C) and VERTEX_PFACE (0x0D) — the
/// three variants whose whole field list is a flag byte and a point.
///
/// The 2D variant ([`Vertex`]) adds the width / bulge / tangent fields;
/// these three do not carry them, and none of them carries the R2010+
/// `BL vertex_id` either.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertexPoint {
    /// Flag byte, same bit meanings as [`Vertex::flag`].
    pub flag: u8,
    /// Vertex location in the owning polyline's coordinate system.
    pub location: Point3D,
}

/// Decode a VERTEX_3D / VERTEX_MESH / VERTEX_PFACE record.
///
/// # Stream shape — measured
///
/// ```text
/// RC   flag
/// 3BD  location
/// ```
///
/// Every one of the ten records in `sample_AC1032.dwg` closes on its
/// data-stream boundary with this list and no other: the five
/// VERTEX_3D (handles `0x42C`..`0x430`) have 142- and 206-bit budgets —
/// `8 + 3BD` with one and with no defaulted coordinate — and the five
/// VERTEX_PFACE (`0x423`..`0x427`) have 142 bits each. `delta 0` on all
/// ten, via `examples/probe_entity_field_list.rs`.
///
/// The flag values corroborate the reading rather than merely fitting
/// it: the VERTEX_3D records read `0x20`, the "3D polyline vertex" bit
/// of the flag table above, and the VERTEX_PFACE records read `0xC0`,
/// its "polyface mesh vertex" and "3D polygon mesh vertex" bits
/// together. A misaligned byte would have no reason to land on either.
pub fn decode_point(c: &mut BitCursor<'_>) -> Result<VertexPoint> {
    let flag = c.read_rc()?;
    let location = read_bd3(c)?;
    Ok(VertexPoint { flag, location })
}

/// VERTEX_PFACE_FACE (0x0E) — one face of a POLYLINE_PFACE, given as
/// four 1-based indices into the mesh's VERTEX_PFACE list.
///
/// A negative index marks the edge starting at that vertex as
/// invisible; a zero index means the face has fewer than four corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexPfaceFace {
    /// The four corner indices, in face order.
    pub vertex_indices: [i16; 4],
}

/// Decode a VERTEX_PFACE_FACE record.
///
/// # Stream shape — measured
///
/// ```text
/// BS   vertex_index_1 .. vertex_index_4
/// ```
///
/// The two records of `sample_AC1032.dwg` close on their boundary with
/// exactly four `BS` and nothing else — 48 bits for `0x428` and 40 for
/// `0x429`, `delta 0` on both. Their values are `[1, 2, 3, -4]` and
/// `[-1, 4, 5, 0]`: every magnitude falls inside the 1..=5 range the
/// owning POLYLINE_PFACE declares (`vertex_count = 5`), the signs are
/// the invisible-edge convention, and the trailing `0` is the
/// three-corner face. A wrong field list has no reason to produce five
/// in-range indices and one terminator.
pub fn decode_pface_face(c: &mut BitCursor<'_>) -> Result<VertexPfaceFace> {
    let mut vertex_indices = [0i16; 4];
    for index in &mut vertex_indices {
        *index = c.read_bs()?;
    }
    Ok(VertexPfaceFace { vertex_indices })
}
