//! 3DFACE entity (ODA Open Design Specification for .dwg files v5.4
//! §20.4.32) — 3 or 4 point 3D face.
//!
//! # Stream shape (R2000+)
//!
//! ```text
//! B    has_no_flag_ind         -- when set, no invisible-edge mask follows
//! B    z_is_zero               -- when set, the first corner's z is not stored
//! RD   corner_1.x              10
//! RD   corner_1.y              20
//! RD   corner_1.z              30   -- only when z_is_zero is clear
//! 3DD  corner_2                11   -- defaults to corner_1
//! 3DD  corner_3                12   -- defaults to corner_2
//! 3DD  corner_4                13   -- defaults to corner_3
//! BS   invisible_edges         70   -- only when has_no_flag_ind is clear
//! ```
//!
//! # Measured: the trailing corners are `3DD`, not `BD` deltas
//!
//! The previous reading treated corners 2-4 as `BD` offsets added to the
//! previous corner, and dropped corner 4 entirely when
//! `has_no_flag_ind` was set. §20.4.32 types them as `3DD` — "default
//! double" triples that reuse the previous corner as the default — and
//! makes only the invisible-edge `BS` conditional. On the 3DFACE record
//! of `sample_AC1032.dwg` (handle `0x322`) the `BD` reading hits a
//! reserved `11` bit pattern; the `3DD` reading closes the record
//! exactly on its data-stream boundary at bit 503.

use crate::bitcursor::BitCursor;
use crate::entities::Point3D;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct ThreeDFace {
    pub corners: [Point3D; 4],
    pub invisible_edges: u16,
    pub is_triangle: bool,
}

/// Read a `3DD` (§2.6) — three `DD`s defaulting componentwise to `d`.
fn read_dd3(c: &mut BitCursor<'_>, d: Point3D) -> Result<Point3D> {
    Ok(Point3D {
        x: c.read_dd(d.x)?,
        y: c.read_dd(d.y)?,
        z: c.read_dd(d.z)?,
    })
}

/// Decodes the `ThreeDFace` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>) -> Result<ThreeDFace> {
    let has_no_flag = c.read_b()?;
    let z_is_zero = c.read_b()?;
    let c1 = Point3D {
        x: c.read_rd()?,
        y: c.read_rd()?,
        z: if z_is_zero { 0.0 } else { c.read_rd()? },
    };
    let c2 = read_dd3(c, c1)?;
    let c3 = read_dd3(c, c2)?;
    let c4 = read_dd3(c, c3)?;
    let invisible = if has_no_flag { 0u16 } else { c.read_bs_u()? };
    // A face whose fourth corner repeats the third is a triangle.
    let is_triangle = c4 == c3;
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

    /// A triangle: the fourth corner repeats the third, so every `DD`
    /// of the fourth `3DD` takes the "use default" code.
    #[test]
    fn roundtrip_triangle_face() {
        let mut w = BitWriter::new();
        w.write_b(true); // has_no_flag_ind → no invisible-edge mask
        w.write_b(true); // z_is_zero
        w.write_rd(0.0); // corner 1 x
        w.write_rd(0.0); // corner 1 y
        w.write_dd(0.0, 1.0); // corner 2 (1, 0, 0)
        w.write_dd(0.0, 0.0);
        w.write_dd(0.0, 0.0);
        w.write_dd(1.0, 1.0); // corner 3 (1, 1, 0)
        w.write_dd(0.0, 1.0);
        w.write_dd(0.0, 0.0);
        w.write_dd(1.0, 1.0); // corner 4 repeats corner 3
        w.write_dd(1.0, 1.0);
        w.write_dd(0.0, 0.0);
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
        assert_eq!(f.corners[3], f.corners[2]);
        assert_eq!(f.invisible_edges, 0);
    }

    #[test]
    fn roundtrip_quad_face_with_invisible_edges() {
        let mut w = BitWriter::new();
        w.write_b(false); // has_no_flag_ind clear → mask follows
        w.write_b(false); // z is stored
        w.write_rd(0.0);
        w.write_rd(0.0);
        w.write_rd(0.0);
        w.write_dd(0.0, 1.0); // corner 2 (1, 0, 0)
        w.write_dd(0.0, 0.0);
        w.write_dd(0.0, 0.0);
        w.write_dd(1.0, 1.0); // corner 3 (1, 1, 0)
        w.write_dd(0.0, 1.0);
        w.write_dd(0.0, 0.0);
        w.write_dd(1.0, 0.0); // corner 4 (0, 1, 0)
        w.write_dd(1.0, 1.0);
        w.write_dd(0.0, 0.0);
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
