//! POINT entity (§19.4.27).
//!
//! # Stream shape
//!
//! ```text
//! BD  x
//! BD  y
//! BD  z
//! BT  thickness
//! BE  extrusion
//! BD  x-axis-angle      -- in radians (UCS-local rotation of the point marker)
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_be_for, read_bt_for};
use crate::error::Result;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub position: Point3D,
    pub thickness: f64,
    pub extrusion: Vec3D,
    pub x_axis_angle: f64,
}

/// Decodes the `Point` payload that follows the common entity header.
///
/// Reads the R2000+ form; see [`decode_for`] for R13-R14, whose `BT` and `BE` are a bare `BD` and `3BD` (§2.8, §2.10).
pub fn decode(c: &mut BitCursor<'_>) -> Result<Point> {
    decode_for(c, Version::R2000)
}

/// Decodes the `Point` payload for `version`: the `BT` thickness and `BE` extrusion take their R13-R14 form (§2.8, §2.10) there.
pub fn decode_for(c: &mut BitCursor<'_>, version: Version) -> Result<Point> {
    let x = c.read_bd()?;
    let y = c.read_bd()?;
    let z = c.read_bd()?;
    let thickness = read_bt_for(c, version)?;
    let extrusion = read_be_for(c, version)?;
    let x_axis_angle = c.read_bd()?;
    Ok(Point {
        position: Point3D { x, y, z },
        thickness,
        extrusion,
        x_axis_angle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_point() {
        let mut w = BitWriter::new();
        w.write_bd(1.25);
        w.write_bd(2.5);
        w.write_bd(3.75);
        w.write_b(true); // default thickness
        w.write_b(true); // default extrusion
        w.write_bd(0.0); // no rotation
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c).unwrap();
        assert_eq!(
            p.position,
            Point3D {
                x: 1.25,
                y: 2.5,
                z: 3.75
            }
        );
        assert_eq!(p.thickness, 0.0);
        assert_eq!(
            p.extrusion,
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0
            }
        );
        assert_eq!(p.x_axis_angle, 0.0);
    }
}
