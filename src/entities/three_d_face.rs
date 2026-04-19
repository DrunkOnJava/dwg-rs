//! 3DFACE entity (§19.4.32) — 3 or 4 point 3D face.
//!
//! # Stream shape (R2000+)
//!
//! ```text
//! B    hasNoFlagInd     -- R2000+; if true, 3 BD3 corners follow, no edge flags
//! B    Z-is-zero-flag   -- R24+; if true, all-z defaulted to 0
//! RD3  corner_1
//! BD3  corner_2         -- delta-encoded from corner_1
//! BD3  corner_3         -- delta from corner_2
//! (if has 4 corners)
//!   BD3  corner_4       -- delta from corner_3
//! BS   invisible_edges_mask (bits 0..3 mark edges as invisible)
//! ```
//!
//! For simpler handling we treat the face as always 4 corners, with
//! corner_4 = corner_3 for triangles.

use crate::bitcursor::BitCursor;
use crate::entities::Point3D;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct ThreeDFace {
    pub corners: [Point3D; 4],
    pub invisible_edges: u16,
    pub is_triangle: bool,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<ThreeDFace> {
    let has_no_flag = c.read_b()?;
    let z_is_zero = c.read_b()?;
    let c1x = c.read_rd()?;
    let c1y = c.read_rd()?;
    let c1z = if z_is_zero { 0.0 } else { c.read_rd()? };
    let c1 = Point3D {
        x: c1x,
        y: c1y,
        z: c1z,
    };
    let c2 = {
        let dx = c.read_bd()?;
        let dy = c.read_bd()?;
        let dz = if z_is_zero { 0.0 } else { c.read_bd()? };
        Point3D {
            x: c1.x + dx,
            y: c1.y + dy,
            z: c1.z + dz,
        }
    };
    let c3 = {
        let dx = c.read_bd()?;
        let dy = c.read_bd()?;
        let dz = if z_is_zero { 0.0 } else { c.read_bd()? };
        Point3D {
            x: c2.x + dx,
            y: c2.y + dy,
            z: c2.z + dz,
        }
    };
    // Per spec, when hasNoFlagInd==true there are only 3 corners and no invisible mask.
    let (c4, invisible, is_triangle) = if has_no_flag {
        (c3, 0u16, true)
    } else {
        let dx = c.read_bd()?;
        let dy = c.read_bd()?;
        let dz = if z_is_zero { 0.0 } else { c.read_bd()? };
        let c4 = Point3D {
            x: c3.x + dx,
            y: c3.y + dy,
            z: c3.z + dz,
        };
        let mask = c.read_bs_u()?;
        (c4, mask, false)
    };
    Ok(ThreeDFace {
        corners: [c1, c2, c3, c4],
        invisible_edges: invisible,
        is_triangle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_triangle_face() {
        let mut w = BitWriter::new();
        w.write_b(true); // has_no_flag → 3 corners
        w.write_b(true); // z_is_zero
        // corner 1
        w.write_rd(0.0);
        w.write_rd(0.0);
        // corner 2 deltas
        w.write_bd(1.0);
        w.write_bd(0.0);
        // corner 3 deltas
        w.write_bd(0.0);
        w.write_bd(1.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let f = decode(&mut c).unwrap();
        assert!(f.is_triangle);
        assert_eq!(
            f.corners[0],
            Point3D {
                x: 0.0,
                y: 0.0,
                z: 0.0
            }
        );
        assert_eq!(
            f.corners[1],
            Point3D {
                x: 1.0,
                y: 0.0,
                z: 0.0
            }
        );
        assert_eq!(
            f.corners[2],
            Point3D {
                x: 1.0,
                y: 1.0,
                z: 0.0
            }
        );
        assert_eq!(f.corners[3], f.corners[2]); // triangle
    }

    #[test]
    fn roundtrip_quad_face() {
        let mut w = BitWriter::new();
        w.write_b(false); // has all 4 corners
        w.write_b(true); // z_is_zero
        w.write_rd(0.0);
        w.write_rd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(-1.0);
        w.write_bd(0.0);
        w.write_bs_u(0b0101); // edges 0 and 2 invisible
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let f = decode(&mut c).unwrap();
        assert!(!f.is_triangle);
        assert_eq!(f.invisible_edges, 0b0101);
        assert_eq!(
            f.corners[3],
            Point3D {
                x: 0.0,
                y: 1.0,
                z: 0.0
            }
        );
    }
}
