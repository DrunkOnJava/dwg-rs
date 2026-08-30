//! LINE entity (§20.4.21).
//!
//! # Stream shape — R13/R14
//!
//! ```text
//! 3BD start
//! 3BD end
//! BT  thickness
//! BE  extrusion
//! ```
//!
//! §20.4.21 gives the two endpoints as plain `3BD`s on R13-R14 and only
//! introduces the `Z's are zero` / `RD` + `DD` compression at R2000. The
//! LINE record of every R14 corpus file runs off the end of its payload
//! under the R2000+ reading (`wanted 8 bits, 2 bits remain`) and closes
//! exactly on its `RL` boundary under this one — see
//! [`decode_versioned`].
//!
//! # Stream shape — R2000+
//!
//! ```text
//! B   zflag             -- true ⇒ entity is 2D (z coords defaulted to 0.0)
//! RD  start.x
//! DD  end.x             -- defaults to start.x
//! RD  start.y
//! DD  end.y             -- defaults to start.y
//! (if !zflag)
//!   RD  start.z
//!   DD  end.z           -- defaults to start.z
//! BT  thickness         -- default 0.0
//! BE  extrusion         -- default (0,0,1)
//! ```
//!
//! The end coordinates use the spec's DD (bitdouble with default)
//! primitive, where each start coordinate is the corresponding default.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3, read_be, read_bt};
use crate::error::Result;
use crate::version::Version;

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

/// Decode a LINE entity's payload for `version`.
///
/// Routes to the R13/R14 `3BD` + `3BD` form or the R2000+
/// `RD` + `DD` form; see the module docs. The cursor must already be
/// positioned past the common entity preamble.
pub fn decode_versioned(c: &mut BitCursor<'_>, version: Version) -> Result<Line> {
    if matches!(version, Version::R14) {
        return decode_r13_r14(c);
    }
    decode(c)
}

/// Decode the R13/R14 LINE payload: both endpoints as plain `3BD`s.
///
/// Kept separate from [`decode`] because the two layouts share no
/// fields — R13/R14 has no `Z's are zero` flag and no `DD` defaults.
pub fn decode_r13_r14(c: &mut BitCursor<'_>) -> Result<Line> {
    let start = read_bd3(c)?;
    let end = read_bd3(c)?;
    let thickness = read_bt(c)?;
    let extrusion = read_be(c)?;
    Ok(Line {
        start,
        end,
        thickness,
        extrusion,
        is_2d: start.z == 0.0 && end.z == 0.0,
    })
}

/// Decode an R2000+ LINE entity's payload from `c`.
///
/// The cursor must already be positioned past the common entity
/// preamble. For a version-dispatching entry point see
/// [`decode_versioned`].
pub fn decode(c: &mut BitCursor<'_>) -> Result<Line> {
    let zflag = c.read_b()?;
    let sx = c.read_rd()?;
    let ex = c.read_dd(sx)?;
    let sy = c.read_rd()?;
    let ey = c.read_dd(sy)?;
    let (sz, ez) = if zflag {
        (0.0, 0.0)
    } else {
        let sz = c.read_rd()?;
        let ez = c.read_dd(sz)?;
        (sz, ez)
    };
    let thickness = read_bt(c)?;
    let extrusion = read_be(c)?;
    Ok(Line {
        start: Point3D {
            x: sx,
            y: sy,
            z: sz,
        },
        end: Point3D {
            x: ex,
            y: ey,
            z: ez,
        },
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
        w.write_dd(1.0, 6.0); // end.x
        w.write_rd(2.0); // start.y
        w.write_dd(2.0, 5.0); // end.y
        w.write_b(true); // thickness default 0.0
        w.write_b(true); // extrusion default
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c).unwrap();
        assert!(l.is_2d);
        assert_eq!(
            l.start,
            Point3D {
                x: 1.0,
                y: 2.0,
                z: 0.0
            }
        );
        assert_eq!(
            l.end,
            Point3D {
                x: 6.0,
                y: 5.0,
                z: 0.0
            }
        );
        assert_eq!(l.thickness, 0.0);
        assert_eq!(
            l.extrusion,
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0
            }
        );
    }

    #[test]
    fn roundtrip_3d_line() {
        let mut w = BitWriter::new();
        w.write_b(false); // 3D
        w.write_rd(1.0);
        w.write_dd(1.0, 3.0);
        w.write_rd(3.0);
        w.write_dd(3.0, 7.0);
        w.write_rd(5.0);
        w.write_dd(5.0, 11.0);
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
        assert_eq!(
            l.start,
            Point3D {
                x: 1.0,
                y: 3.0,
                z: 5.0
            }
        );
        assert_eq!(
            l.end,
            Point3D {
                x: 3.0,
                y: 7.0,
                z: 11.0
            }
        );
        assert_eq!(l.thickness, 2.5);
    }
}
