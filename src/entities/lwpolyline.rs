//! LWPOLYLINE entity (ODA Open Design Specification for .dwg files
//! v5.4 §20.4.85 "LWPLINE") — lightweight polyline.
//!
//! LWPOLYLINE is the most common modern 2D polyline — it replaces the
//! older 2D POLYLINE with per-vertex records by packing all vertices
//! into a single entity. Widths, bulges (arc segments) and vertex IDs
//! are optional, and the leading `BS` flag word says which are present.
//!
//! # Stream shape (R2000+)
//!
//! ```text
//! BS   flag
//! (flag & 0x004) BD  constant_width
//! (flag & 0x008) BD  elevation
//! (flag & 0x002) BD  thickness
//! (flag & 0x001) 3BD extrusion
//! BL   num_points
//! (flag & 0x010) BL  num_bulges
//! (flag & 0x400) BL  num_vertex_ids    -- R2010+
//! (flag & 0x020) BL  num_widths
//! 2RD  vertex[0]
//! 2DD  vertex[1..]                     -- previous vertex as default
//! BD × num_bulges
//! BL × num_vertex_ids
//! 2BD × num_widths
//! ```
//!
//! # Measured: the flag bits and the field order
//!
//! An earlier reading of this entity used a different bit assignment
//! (`0x01` elevation, `0x02` thickness, `0x04` extrusion, `0x20`
//! constant width, `0x80` variable width, `0x400` bulges, `0x8000`
//! vertex ids) and put the constant width *after* the counts. That
//! reading survives on a polyline whose flag word is `0` or `0x200`,
//! which is 19 of the 20 LWPOLYLINE records of `sample_AC1032.dwg`. The
//! twentieth (handle `0x4A6`) has flag `4`: under §20.4.85 that is a
//! constant width read straight after the flag, and under the old
//! reading it was an extrusion read three fields later — which put the
//! vertex count on a reserved `11` bit pattern.
//!
//! `0x200` is not one of §20.4.85's presence bits ({`0x001`, `0x002`,
//! `0x004`, `0x008`, `0x010`, `0x020`, `0x400`}), and twelve records of
//! the sample carry exactly `0x200` with no optional field at all. It
//! is read here as the closed flag, the conventional meaning; nothing
//! in this corpus contradicts it and nothing in it proves it either.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Vec3D};
use crate::error::Result;
use crate::version::Version;

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

/// Flag bits of the leading `BS` (§20.4.85).
pub mod flag_bits {
    /// `0x001` — an extrusion (`3BD`) is stored.
    pub const HAS_EXTRUSION: u16 = 0x0001;
    /// `0x002` — a thickness (`BD`) is stored.
    pub const HAS_THICKNESS: u16 = 0x0002;
    /// `0x004` — a constant width (`BD`) is stored.
    pub const CONSTANT_WIDTH: u16 = 0x0004;
    /// `0x008` — an elevation (`BD`) is stored.
    pub const HAS_ELEVATION: u16 = 0x0008;
    /// `0x010` — a bulge count (`BL`) and that many `BD`s are stored.
    pub const HAS_BULGES: u16 = 0x0010;
    /// `0x020` — a width count (`BL`) and that many `2BD`s are stored.
    pub const HAS_VARIABLE_WIDTH: u16 = 0x0020;
    /// `0x200` — the polyline is closed. Not a presence bit; see the
    /// module docs for how far the corpus supports the name.
    pub const CLOSED: u16 = 0x0200;
    /// `0x400` — a vertex-id count (`BL`) and that many `BL`s are
    /// stored (R2010+).
    pub const HAS_VERTEX_ID: u16 = 0x0400;
}

/// Decodes the `LwPolyline` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<LwPolyline> {
    use flag_bits::*;
    let flag = c.read_bs_u()?;

    let constant_width = if flag & CONSTANT_WIDTH != 0 {
        Some(c.read_bd()?)
    } else {
        None
    };
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
    let num_ids = if version.is_r2010_plus() && flag & HAS_VERTEX_ID != 0 {
        c.read_bl()? as usize
    } else {
        0
    };
    let num_widths = if flag & HAS_VARIABLE_WIDTH != 0 {
        c.read_bl()? as usize
    } else {
        0
    };

    // Defensive caps. Three layered checks (L4-12):
    //
    // 1. Hard sanity ceiling — 1 million vertices is already far beyond
    //    any real drawing.
    //
    // 2. Coarse remaining-payload derivation — a count larger than the
    //    number of BITS left on the cursor cannot possibly be real.
    //
    // 3. Tighter per-item minimum-bits derivation (L4-12) — each vertex
    //    costs at least two 2-bit `DD` prefixes, each bulge at least one
    //    2-bit `BD` prefix, each id at least a 2-bit `BL` prefix and each
    //    width pair at least two 2-bit `BD` prefixes.
    const LWPOLYLINE_MAX: usize = 1_000_000;
    const MIN_BITS_PER_POINT: usize = 4;
    const MIN_BITS_PER_BULGE: usize = 2;
    const MIN_BITS_PER_VERTEX_ID: usize = 2;
    const MIN_BITS_PER_WIDTH: usize = 4;
    let remaining = c.remaining_bits();
    let total_claimed = num_points
        .saturating_add(num_bulges)
        .saturating_add(num_ids)
        .saturating_add(num_widths);
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
    if num_points > 0 {
        let mut previous = Point2D {
            x: c.read_rd()?,
            y: c.read_rd()?,
        };
        vertices.push(previous);
        for _ in 1..num_points {
            previous = Point2D {
                x: c.read_dd(previous.x)?,
                y: c.read_dd(previous.y)?,
            };
            vertices.push(previous);
        }
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
        w.write_rd(0.0);
        w.write_rd(0.0);
        w.write_dd(0.0, 10.0);
        w.write_dd(0.0, 0.0);
        w.write_dd(10.0, 10.0);
        w.write_dd(0.0, 10.0);
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
        w.write_rd(0.0);
        w.write_rd(0.0);
        w.write_dd(0.0, 10.0);
        w.write_dd(0.0, 0.0);
        w.write_dd(10.0, 10.0);
        w.write_dd(0.0, 10.0);
        w.write_dd(10.0, 0.0);
        w.write_dd(10.0, 10.0);
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
