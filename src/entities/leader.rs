//! LEADER entity (§20.4.47, type 45) — the legacy leader line that
//! points from an annotation to a feature (before MULTILEADER).
//!
//! # Stream shape (§20.4.47)
//!
//! ```text
//! B     unknown               -- "always seems to be 0"
//! BS    annotation type       -- 0 MTEXT, 1 TOLERANCE, 2 INSERT, 3 none
//! BS    path type             -- 0 straight, 1 spline
//! BL    numpts
//! 3BD   point × numpts                              10
//! 3BD   origin                -- the leader plane origin
//! 3BD   extrusion                                   210
//! 3BD   x direction                                 211
//! 3BD   offset to block insertion point             212
//! R14+:     3BD  end point projection
//! R13-R14:  BD   dimgap        -- DIMGAP x DIMSCALE at creation
//! R13-R2007 (measured, see below):
//!           BD   box height                         40
//!           BD   box width                          41
//! B     hookline on x direction
//! B     arrowhead on
//! R13-R14:  BS arrowhead type, BD dimasz, B, B, BS, BS byblock colour, B, B
//! R2000+:   BS, B, B           -- unknown
//! ```
//!
//! The associated annotation and DIMSTYLE handles follow in the handle
//! stream and are not read here.
//!
//! # Measured (2026-10-26, #71)
//!
//! The previous decoder read four `3BD`s after the points where
//! §20.4.47 has five, so the field it called the end projection was the
//! origin (equal to the first point, the default the section gives
//! it), and it stopped there: `delta -12` on R2010+ records, `-144` on
//! R2000-R2007, `-282` on R14.
//!
//! The section's list closes at `delta 0` on the one LEADER of each of
//! ACadSharp's seven saves of one drawing, R14 to R2018, with one
//! departure: **from R2010 the box height and width are not written.**
//! On R2000, R2004 and R2007 they are two full `BD`s, 1.844821… and
//! 16.374004…, equal to groups 40 and 41 of the DXF twin; on R2010,
//! R2013 and R2018 the record has exactly 132 bits less, and the six
//! bits after the end point projection are the same `011000` as the six
//! after the box width on R2000. The DXF exports of R2010+ still carry
//! 40 and 41, so AutoCAD has them from elsewhere; the record does not.
//!
//! On R14 the R13-R14 branch closes exactly too, and its two creation
//! values are the drawing's own ISO-25 DIMSTYLE: dimgap `0.625`
//! (DIMGAP, group 147) and dimasz `2.5` (DIMASZ, group 41).
//! ARCHITECTURE.md §7ab has the table; `examples/probe_leader.rs`
//! reproduces it.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

/// A decoded LEADER entity (§20.4.47).
#[derive(Debug, Clone, PartialEq)]
pub struct Leader {
    /// `BS` annotation type: 0 MTEXT, 1 TOLERANCE, 2 INSERT, 3 none.
    pub annot_type: i16,
    /// `BS` path type: 0 straight, 1 spline.
    pub path_type: i16,
    /// `3BD` vertices (DXF 10).
    pub points: Vec<Point3D>,
    /// `3BD` leader plane origin; by default the first point.
    pub origin: Point3D,
    /// `3BD` extrusion (DXF 210).
    pub extrusion: Vec3D,
    /// `3BD` x direction of the leader plane (DXF 211).
    pub x_direction: Vec3D,
    /// `3BD` offset to the block insertion point (DXF 212).
    pub offset_to_block_insertion: Vec3D,
    /// `3BD` end point projection (R14+): the offset from the last point
    /// to the annotation. R13 stores none (§20.4.47), so it is the zero
    /// vector there.
    pub end_point_projection: Vec3D,
    /// `BD` DIMGAP × DIMSCALE at creation; R13-R14 only.
    pub dimgap: Option<f64>,
    /// `BD` annotation box height (DXF 40); not written from R2010.
    pub box_height: Option<f64>,
    /// `BD` annotation box width (DXF 41); not written from R2010.
    pub box_width: Option<f64>,
    /// `B` hook line on the x direction (DXF 74).
    pub hookline_on_x_dir: bool,
    /// `B` arrowhead on.
    pub arrowhead_on: bool,
    /// The R13-R14 tail.
    pub r14: Option<LeaderR14Tail>,
    /// The R2000+ tail: `BS`, `B`, `B`, all unnamed by §20.4.47.
    pub unknown_r2000: Option<(i16, bool, bool)>,
}

/// The R13-R14-only fields at the end of a LEADER record (§20.4.47).
#[derive(Debug, Clone, PartialEq)]
pub struct LeaderR14Tail {
    /// `BS` arrowhead type.
    pub arrowhead_type: i16,
    /// `BD` DIMASZ × DIMSCALE at creation.
    pub dimasz: f64,
    /// The two `B`s, the `BS`, then the two `B`s §20.4.47 leaves unnamed.
    pub unknown: (bool, bool, i16),
    /// `BS` byblock colour.
    pub byblock_color: i16,
    /// The last two unnamed `B`s.
    pub unknown_tail: (bool, bool),
}

/// Decodes the §20.4.47 LEADER payload that follows the common entity
/// data.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Leader> {
    let _unknown = c.read_b()?;
    let annot_type = c.read_bs()?;
    let path_type = c.read_bs()?;
    let num_points = c.read_bl()? as usize;
    // Leader callouts are a small handful of points in every real
    // drawing; a 100K ceiling catches pathological inputs while still
    // covering any imaginable real use.
    const LEADER_MAX_POINTS: usize = 100_000;
    if num_points > LEADER_MAX_POINTS || num_points > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "LEADER claims {num_points} points (cap {LEADER_MAX_POINTS}, \
             remaining_bits {})",
            c.remaining_bits()
        )));
    }
    let mut points = Vec::with_capacity(num_points);
    for _ in 0..num_points {
        points.push(read_bd3(c)?);
    }
    let origin = read_bd3(c)?;
    let extrusion = read_bd3(c)?;
    let x_direction = read_bd3(c)?;
    let offset_to_block_insertion = read_bd3(c)?;
    let end_point_projection = if version == Version::R13 {
        Vec3D::default()
    } else {
        read_bd3(c)?
    };
    let r13_r14 = version.is_r13_r14();
    let dimgap = if r13_r14 { Some(c.read_bd()?) } else { None };
    let (box_height, box_width) = if version.is_r2010_plus() {
        (None, None)
    } else {
        (Some(c.read_bd()?), Some(c.read_bd()?))
    };
    let hookline_on_x_dir = c.read_b()?;
    let arrowhead_on = c.read_b()?;
    let (r14, unknown_r2000) = if r13_r14 {
        let arrowhead_type = c.read_bs()?;
        let dimasz = c.read_bd()?;
        let unknown = (c.read_b()?, c.read_b()?, c.read_bs()?);
        let byblock_color = c.read_bs()?;
        let unknown_tail = (c.read_b()?, c.read_b()?);
        (
            Some(LeaderR14Tail {
                arrowhead_type,
                dimasz,
                unknown,
                byblock_color,
                unknown_tail,
            }),
            None,
        )
    } else {
        (None, Some((c.read_bs()?, c.read_b()?, c.read_b()?)))
    };
    Ok(Leader {
        annot_type,
        path_type,
        points,
        origin,
        extrusion,
        x_direction,
        offset_to_block_insertion,
        end_point_projection,
        dimgap,
        box_height,
        box_width,
        hookline_on_x_dir,
        arrowhead_on,
        r14,
        unknown_r2000,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_3pt_leader() {
        let mut w = BitWriter::new();
        w.write_b(false); // unknown
        w.write_bs(0); // text annot
        w.write_bs(0); // straight path
        w.write_bl(3);
        for (x, y, z) in [(0.0f64, 0.0, 0.0), (5.0, 5.0, 0.0), (10.0, 5.0, 0.0)] {
            w.write_bd(x);
            w.write_bd(y);
            w.write_bd(z);
        }
        // end projection
        w.write_bd(10.0);
        w.write_bd(5.0);
        w.write_bd(0.0);
        // extrusion
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        // horiz direction
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // offset
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // end point projection, then the R2010+ tail
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_b(false);
        w.write_b(true);
        w.write_bs(0);
        w.write_b(false);
        w.write_b(false);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2018).unwrap();
        assert_eq!(l.annot_type, 0);
        assert_eq!(l.path_type, 0);
        assert_eq!(l.points.len(), 3);
        assert_eq!(
            l.origin,
            Point3D {
                x: 10.0,
                y: 5.0,
                z: 0.0
            }
        );
    }
}
