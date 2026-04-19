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
use crate::entities::{Point3D, Vec3D, read_be, read_bt};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub position: Point3D,
    pub thickness: f64,
    pub extrusion: Vec3D,
    pub x_axis_angle: f64,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<Point> {
    let x = c.read_bd()?;
    let y = c.read_bd()?;
    let z = c.read_bd()?;
    let thickness = read_bt(c)?;
    let extrusion = read_be(c)?;
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
        assert_eq!(p.position, Point3D { x: 1.25, y: 2.5, z: 3.75 });
        assert_eq!(p.thickness, 0.0);
        assert_eq!(p.extrusion, Vec3D { x: 0.0, y: 0.0, z: 1.0 });
        assert_eq!(p.x_axis_angle, 0.0);
    }
}
