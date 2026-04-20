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

    // Defensive caps. Three layered checks (L4-12):
    //
    // 1. Hard sanity ceiling — 1 million vertices is already far beyond
    //    any real drawing. Previously this was 10M; the lower value
    //    still accommodates real-world usage and shrinks the worst-case
    //    allocation envelope by an order of magnitude.
    //
    // 2. Coarse remaining-payload derivation — a count larger than the
    //    number of BITS left on the cursor cannot possibly be real. Each
    //    claimed item needs at least 1 bit to exist; so
    //    `remaining_bits() >= count` is the cheapest sound upper bound.
    //
    // 3. Tighter per-item minimum-bits derivation (L4-12) — the spec
    //    requires each LWPOLYLINE vertex to carry at least two compressed
    //    doubles (x + y). A compressed double (BD, spec §2.10) occupies
    //    at least 2 bits (the 2-bit prefix selecting one of the three
    //    small-value sentinels — `00` = next BD, `01` = 1.0, `10` = 0.0,
    //    `11` = previous). Even in the densest encoding path a vertex
    //    therefore costs ≥ 2 × 2 = 4 bits; we use 2 × 2 = 4 as the floor
    //    here. The same 2×BD floor applies per bulge (1 × BD) and per
    //    width pair (2 × BD). This rejects adversarial counts that pass
    //    the coarse 1-bit check but whose *realised* bit cost would still
    //    blow past the remaining payload.
    const LWPOLYLINE_MAX: usize = 1_000_000;
    // 2 bits-per-BD × 2 BDs = 4 bits minimum per vertex point.
    const MIN_BITS_PER_POINT: usize = 4;
    // 2 bits-per-BD × 1 BD = 2 bits minimum per bulge.
    const MIN_BITS_PER_BULGE: usize = 2;
    // 32-bit BL vertex-id. BL encoding packs small values but the
    // shortest form is still 2 bits (00 prefix = 0).
    const MIN_BITS_PER_VERTEX_ID: usize = 2;
    // Variable-width entry is (start-width, end-width) — 2 × BD ⇒ 4 bits.
    const MIN_BITS_PER_WIDTH: usize = 4;
    let remaining = c.remaining_bits();
    let total_claimed = num_points
        .saturating_add(num_bulges)
        .saturating_add(num_ids)
        .saturating_add(num_widths);
    // Tighter cap-derived check: realised bit cost per field, then sum.
    // Use saturating math so a claimed count of usize::MAX doesn't wrap.
    let realised_bits = num_points
        .saturating_mul(MIN_BITS_PER_POINT)
        .saturating_add(num_bulges.saturating_mul(MIN_BITS_PER_BULGE))
        .saturating_add(num_ids.saturating_mul(MIN_BITS_PER_VERTEX_ID))
        .saturating_add(num_widths.saturating_mul(MIN_BITS_PER_WIDTH));
    if num_points > LWPOLYLINE_MAX
        || num_bulges > LWPOLYLINE_MAX
        || num_ids > LWPOLYLINE_MAX
        || num_widths > LWPOLYLINE_MAX
        || total_claimed > remaining
        || realised_bits > remaining
    {
        return Err(crate::error::Error::SectionMap(format!(
            "LWPOLYLINE has implausible counts (p={num_points}, b={num_bulges}, \
             i={num_ids}, w={num_widths}; remaining_bits={remaining}, \
             min_realised_bits={realised_bits})"
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

    // ------------------------------------------------------------------
    // L4-12 mutation / failure-mode tests (appended 2026-04-20).
    //
    // Feed adversarial counts to the decoder and assert that it rejects
    // the claim without allocating. The previous coarse "≥ 1 bit per
    // item" check only caught counts larger than the payload bit-length;
    // the tighter "min_bits_per_point" check rejects smaller counts
    // whose realised bit cost would still exceed the payload.
    // ------------------------------------------------------------------

    /// Build a minimal LWPOLYLINE header claiming `num_points` vertices,
    /// with no optional fields and no trailing vertex data. The claim
    /// intentionally lies about the stream so the cap check fires.
    fn build_oversized_claim(num_points: i32) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.write_bs_u(0); // no optional fields, no flags
        w.write_bl(num_points);
        // deliberately no vertex payload — the count check must fire
        // before any RD reads.
        w.into_bytes()
    }

    #[test]
    fn rejects_ten_million_point_claim() {
        // 10M points > LWPOLYLINE_MAX (1M) — must return Err without
        // attempting to allocate ~160 MiB of Vec<Point2D>.
        let bytes = build_oversized_claim(10_000_000);
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c).unwrap_err();
        assert!(
            matches!(&err, crate::error::Error::SectionMap(msg) if msg.contains("LWPOLYLINE")),
            "expected LWPOLYLINE SectionMap error, got: {err:?}"
        );
    }

    #[test]
    fn rejects_one_million_one_points_just_over_cap() {
        // Exactly one past the cap — still must reject.
        let bytes = build_oversized_claim(1_000_001);
        let mut c = BitCursor::new(&bytes);
        assert!(decode(&mut c).is_err());
    }

    #[test]
    fn rejects_point_count_exceeding_payload_bits() {
        // A 100-byte payload is at most 800 bits. A claim of 100_000
        // vertices × min 4 bits/point = 400_000 bits needed — far more
        // than the payload can hold. Under the tighter realised-bits
        // check, this rejects even though 100_000 < LWPOLYLINE_MAX.
        let mut w = BitWriter::new();
        w.write_bs_u(0);
        w.write_bl(100_000);
        // Pad to ~100 bytes total.
        while w.as_slice().len() < 100 {
            w.write_rc(0);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c).unwrap_err();
        // Error message must mention both the claim and the tighter
        // derivation so debuggers can diagnose adversarial inputs.
        if let crate::error::Error::SectionMap(msg) = &err {
            assert!(
                msg.contains("100000"),
                "expected claim count in error, got: {msg}"
            );
        } else {
            panic!("expected SectionMap error, got: {err:?}");
        }
    }

    #[test]
    fn tighter_check_rejects_where_coarse_would_pass() {
        // Craft a case where 1-bit-per-item (coarse) passes but the
        // tighter 4-bits-per-point check fails.
        //
        // Build a payload with roughly 1000 remaining bits after the BL
        // read. Claim 500 points: coarse (500 ≤ 1000) passes, tighter
        // (500 × 4 = 2000 > 1000) fails.
        let mut w = BitWriter::new();
        w.write_bs_u(0);
        w.write_bl(500);
        // Pad ~125 bytes ≈ 1000 bits of junk payload.
        for _ in 0..125 {
            w.write_rc(0);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c).unwrap_err();
        assert!(matches!(err, crate::error::Error::SectionMap(_)));
    }
}
