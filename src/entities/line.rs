//! LINE entity (§19.4.20).
//!
//! # Stream shape
//!
//! ```text
//! B   zflag             -- true ⇒ entity is 2D (z coords defaulted to 0.0)
//! RD  start.x
//! BD  end.x             -- delta-encoded relative to start.x
//! RD  start.y
//! BD  end.y             -- delta-encoded relative to start.y
//! (if !zflag)
//!   RD  start.z
//!   BD  end.z           -- delta-encoded relative to start.z
//! BT  thickness         -- default 0.0
//! BE  extrusion         -- default (0,0,1)
//! ```
//!
//! The delta encoding of end coordinates is the spec's "z-flag 2D
//! shortcut" (§19.4.20). A short line has end-x ≈ start-x, so the
//! delta typically packs into one BD-bit-2 (1.0) or zero byte.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_be, read_bt};
use crate::error::Result;

/// Fully-decoded LINE entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub start: Point3D,
    pub end: Point3D,
    pub thickness: f64,
    pub extrusion: Vec3D,
    /// Whether the line was encoded as 2D (z=0 for both endpoints).
    pub is_2d: bool,
}

/// Decode a LINE entity's payload from `c`.
///
/// The cursor must already be positioned past the common entity
/// preamble.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Line> {
    let zflag = c.read_b()?;
    let sx = c.read_rd()?;
    let ex = sx + c.read_bd()?;
    let sy = c.read_rd()?;
    let ey = sy + c.read_bd()?;
    let (sz, ez) = if zflag {
        (0.0, 0.0)
    } else {
        let sz = c.read_rd()?;
        let ez = sz + c.read_bd()?;
        (sz, ez)
    };
    let thickness = read_bt(c)?;
    let extrusion = read_be(c)?;
    Ok(Line {
        start: Point3D { x: sx, y: sy, z: sz },
        end: Point3D { x: ex, y: ey, z: ez },
        thickness,
        extrusion,
        is_2d: zflag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_2d_line() {
        let mut w = BitWriter::new();
        w.write_b(true); // 2D
        w.write_rd(1.0); // start.x
        w.write_bd(5.0); // end.x delta: 5.0, so end.x = 6.0
        w.write_rd(2.0); // start.y
        w.write_bd(3.0); // end.y delta: 3.0, end.y = 5.0
        w.write_b(true); // thickness default 0.0
        w.write_b(true); // extrusion default
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c).unwrap();
        assert!(l.is_2d);
        assert_eq!(l.start, Point3D { x: 1.0, y: 2.0, z: 0.0 });
        assert_eq!(l.end, Point3D { x: 6.0, y: 5.0, z: 0.0 });
        assert_eq!(l.thickness, 0.0);
        assert_eq!(l.extrusion, Vec3D { x: 0.0, y: 0.0, z: 1.0 });
    }

    #[test]
    fn roundtrip_3d_line() {
        let mut w = BitWriter::new();
        w.write_b(false); // 3D
        w.write_rd(1.0);
        w.write_bd(2.0);
        w.write_rd(3.0);
        w.write_bd(4.0);
        w.write_rd(5.0);
        w.write_bd(6.0);
        w.write_b(false); // explicit thickness
        w.write_bd(2.5);
        w.write_b(false); // explicit extrusion
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c).unwrap();
        assert!(!l.is_2d);
        assert_eq!(l.start, Point3D { x: 1.0, y: 3.0, z: 5.0 });
        assert_eq!(l.end, Point3D { x: 3.0, y: 7.0, z: 11.0 });
        assert_eq!(l.thickness, 2.5);
    }
}
