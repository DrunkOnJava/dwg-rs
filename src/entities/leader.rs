//! LEADER entity (§19.4.19) — legacy leader line (pre-MLEADER).
//!
//! A LEADER is a connected chain of vertices that typically points at
//! an annotation. It holds the vertex list plus arrow and hook-line
//! decoration flags.
//!
//! # Stream shape
//!
//! ```text
//! B     unknown_bit_1    -- always false in practice
//! BS    annot_type       -- 0 = text, 1 = tolerance, 2 = block
//! BS    path_type        -- 0 = straight, 1 = spline
//! BL    num_points
//! BD3 × num_points   path_points
//! BD3   end_pt_proj     -- the point the leader "ends at"
//! BD3   extrusion
//! BD3   horiz_dir       -- X-axis for the leader's text plane
//! BD3   offset_to_block_ins
//! (R14+) BD3 offset_to_text
//! BD    dimgap
//! BD    box_height
//! BD    box_width
//! B     hookline_on_x_dir
//! B     arrowhead_on
//! BS    arrowhead_type        -- R13-R14
//! (R13-R14 only): BD dimasz, B unknown_b1, B unknown_b2, BS unknown_short
//! (R2000+) B byblock_color, B hookline_on_dir, B has_arrowhead_ref
//! ```
//!
//! Full spec is over 30 fields. This decoder covers the core
//! geometric fields (points, end projection, extrusion) — the text
//! attachment details are skipped via deliberate field count.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Leader {
    pub annot_type: i16,
    pub path_type: i16,
    pub points: Vec<Point3D>,
    pub end_projection: Point3D,
    pub extrusion: Vec3D,
    pub horiz_direction: Vec3D,
    pub offset_to_block_insertion: Vec3D,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<Leader> {
    let _unknown = c.read_b()?;
    let annot_type = c.read_bs()?;
    let path_type = c.read_bs()?;
    let num_points = c.read_bl()? as usize;
    if num_points > 1_000_000 {
        return Err(Error::SectionMap(format!(
            "LEADER claims {num_points} points (>1M cap)"
        )));
    }
    let mut points = Vec::with_capacity(num_points);
    for _ in 0..num_points {
        points.push(read_bd3(c)?);
    }
    let end_projection = read_bd3(c)?;
    let extrusion = read_bd3(c)?;
    let horiz_direction = read_bd3(c)?;
    let offset_to_block_insertion = read_bd3(c)?;
    Ok(Leader {
        annot_type,
        path_type,
        points,
        end_projection,
        extrusion,
        horiz_direction,
        offset_to_block_insertion,
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
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c).unwrap();
        assert_eq!(l.annot_type, 0);
        assert_eq!(l.path_type, 0);
        assert_eq!(l.points.len(), 3);
        assert_eq!(
            l.end_projection,
            Point3D {
                x: 10.0,
                y: 5.0,
                z: 0.0
            }
        );
    }
}
