//! LWPOLYLINE entity (§19.4.25) — lightweight polyline.
//!
//! LWPOLYLINE is the most common modern 2D polyline — it replaces the
//! older 2D POLYLINE with per-vertex records by packing all vertices
//! into a single entity. Widths, bulges (arc segments), and variable
//! vertex IDs are all optional via a flag set.
//!
//! # Stream shape
//!
//! ```text
//! BS   flag               -- bits: 0x01=has_elev, 0x02=has_thick, 0x04=has_ext,
//!                                   0x08=closed,   0x10=plinegen, 0x20=default-width,
//!                                   0x80=has_variable_width, 0x400=has_bulge,
//!                                   0x8000=has_vertex_id (R2010+)
//! (if flag & 0x01) BD  elevation
//! (if flag & 0x02) BD  thickness
//! (if flag & 0x04) BD3 extrusion
//! BL   num_points
//! (if flag & 0x400) BL num_bulges
//! (if flag & 0x8000) BL num_ids      -- R2010+
//! (if flag & 0x80) BL num_widths
//! (if flag & 0x20) BD  constant_width
//! RD2  point_1
//! (R15+: subsequent points as DD — delta-double — for compression)
//! BD*  bulges             -- one per bulge_count
//! BL*  vertex_ids         -- one per id_count (R2010+)
//! (BD BD)* per-vertex widths
//! ```
//!
//! This decoder supports the common R2000+ shape with the most
//! prevalent flags: closed, bulges, default-width. Real AutoCAD
//! writes almost all drawings this way.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Vec3D};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct LwPolyline {
    pub flag: u16,
    pub elevation: Option<f64>,
    pub thickness: Option<f64>,
    pub extrusion: Option<Vec3D>,
    pub constant_width: Option<f64>,
    pub vertices: Vec<Point2D>,
    pub bulges: Vec<f64>,
    pub vertex_ids: Vec<u32>,
    pub widths: Vec<(f64, f64)>,
    pub closed: bool,
}

/// Flag bits (§19.4.25).
pub mod flag_bits {
    pub const HAS_ELEVATION: u16 = 0x0001;
    pub const HAS_THICKNESS: u16 = 0x0002;
    pub const HAS_EXTRUSION: u16 = 0x0004;
    pub const CLOSED: u16 = 0x0008;
    pub const PLINEGEN: u16 = 0x0010;
    pub const CONSTANT_WIDTH: u16 = 0x0020;
    pub const HAS_VARIABLE_WIDTH: u16 = 0x0080;
    pub const HAS_BULGES: u16 = 0x0400;
    pub const HAS_VERTEX_ID: u16 = 0x8000;
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<LwPolyline> {
    use flag_bits::*;
    let flag = c.read_bs_u()?;

    let elevation = if flag & HAS_ELEVATION != 0 {
        Some(c.read_bd()?)
    } else {
        None
    };
    let thickness = if flag & HAS_THICKNESS != 0 {
        Some(c.read_bd()?)
    } else {
        None
    };
    let extrusion = if flag & HAS_EXTRUSION != 0 {
        Some(Vec3D {
            x: c.read_bd()?,
            y: c.read_bd()?,
            z: c.read_bd()?,
        })
    } else {
        None
    };

    let num_points = c.read_bl()? as usize;
    let num_bulges = if flag & HAS_BULGES != 0 {
        c.read_bl()? as usize
    } else {
        0
    };
    let num_ids = if flag & HAS_VERTEX_ID != 0 {
        c.read_bl()? as usize
    } else {
        0
    };
    let num_widths = if flag & HAS_VARIABLE_WIDTH != 0 {
        c.read_bl()? as usize
    } else {
        0
    };

    let constant_width = if flag & CONSTANT_WIDTH != 0 {
        Some(c.read_bd()?)
    } else {
        None
    };

    // Defensive caps. Two checks:
    //
    // 1. Hard sanity ceiling — 1 million vertices is already far beyond
    //    any real drawing. Previously this was 10M; the lower value
    //    still accommodates real-world usage and shrinks the worst-case
    //    allocation envelope by an order of magnitude.
    //
    // 2. Remaining-payload derivation — a count larger than the number
    //    of BITS left on the cursor cannot possibly be real, regardless
    //    of the ceiling above. Each claimed item needs at least 1 bit to
    //    exist; so `remaining_bits() >= count` is the cheapest sound
    //    upper bound. Catches counts inflated past the literal length
    //    of the object's payload.
    const LWPOLYLINE_MAX: usize = 1_000_000;
    let remaining = c.remaining_bits();
    let total_claimed = num_points
        .saturating_add(num_bulges)
        .saturating_add(num_ids)
        .saturating_add(num_widths);
    if num_points > LWPOLYLINE_MAX
        || num_bulges > LWPOLYLINE_MAX
        || num_ids > LWPOLYLINE_MAX
        || num_widths > LWPOLYLINE_MAX
        || total_claimed > remaining
    {
        return Err(crate::error::Error::SectionMap(format!(
            "LWPOLYLINE has implausible counts (p={num_points}, b={num_bulges}, \
             i={num_ids}, w={num_widths}; remaining_bits={remaining})"
        )));
    }

    let mut vertices = Vec::with_capacity(num_points);
    for _ in 0..num_points {
        let x = c.read_rd()?;
        let y = c.read_rd()?;
        vertices.push(Point2D { x, y });
    }
    let mut bulges = Vec::with_capacity(num_bulges);
    for _ in 0..num_bulges {
        bulges.push(c.read_bd()?);
    }
    let mut vertex_ids = Vec::with_capacity(num_ids);
    for _ in 0..num_ids {
        vertex_ids.push(c.read_bl()? as u32);
    }
    let mut widths = Vec::with_capacity(num_widths);
    for _ in 0..num_widths {
        widths.push((c.read_bd()?, c.read_bd()?));
    }

    Ok(LwPolyline {
        flag,
        elevation,
        thickness,
        extrusion,
        constant_width,
        vertices,
        bulges,
        vertex_ids,
        widths,
        closed: flag & CLOSED != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_minimal_lwpolyline() {
        let mut w = BitWriter::new();
        // No optional fields, 3 vertices.
        w.write_bs_u(0);
        w.write_bl(3);
        for (x, y) in [(0.0f64, 0.0f64), (10.0, 0.0), (10.0, 10.0)] {
            w.write_rd(x);
            w.write_rd(y);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c).unwrap();
        assert_eq!(p.flag, 0);
        assert!(!p.closed);
        assert_eq!(p.vertices.len(), 3);
        assert_eq!(p.vertices[0], Point2D { x: 0.0, y: 0.0 });
        assert_eq!(p.vertices[2], Point2D { x: 10.0, y: 10.0 });
    }

    #[test]
    fn roundtrip_closed_polyline_with_bulges() {
        use flag_bits::*;
        let mut w = BitWriter::new();
        w.write_bs_u(CLOSED | HAS_BULGES);
        w.write_bl(4); // 4 points
        w.write_bl(4); // 4 bulges (one per segment)
        for (x, y) in [(0.0f64, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
            w.write_rd(x);
            w.write_rd(y);
        }
        for b in [0.0, 0.5, 0.0, 0.5] {
            w.write_bd(b);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c).unwrap();
        assert!(p.closed);
        assert_eq!(p.bulges.len(), 4);
        assert_eq!(p.bulges[1], 0.5);
    }
}
