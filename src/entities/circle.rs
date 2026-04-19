//! CIRCLE entity (§19.4.8).
//!
//! # Stream shape
//!
//! ```text
//! BD  center.x
//! BD  center.y
//! BD  center.z
//! BD  radius
//! BT  thickness
//! BE  extrusion
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_be, read_bt};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Circle {
    pub center: Point3D,
    pub radius: f64,
    pub thickness: f64,
    pub extrusion: Vec3D,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<Circle> {
    let cx = c.read_bd()?;
    let cy = c.read_bd()?;
    let cz = c.read_bd()?;
    let radius = c.read_bd()?;
    let thickness = read_bt(c)?;
    let extrusion = read_be(c)?;
    Ok(Circle {
        center: Point3D { x: cx, y: cy, z: cz },
        radius,
        thickness,
        extrusion,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_circle() {
        let mut w = BitWriter::new();
        w.write_bd(10.0);
        w.write_bd(20.0);
        w.write_bd(0.0);
        w.write_bd(5.0);
        w.write_b(true);
        w.write_b(true);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let circ = decode(&mut c).unwrap();
        assert_eq!(circ.center, Point3D { x: 10.0, y: 20.0, z: 0.0 });
        assert_eq!(circ.radius, 5.0);
        assert_eq!(circ.thickness, 0.0);
        assert_eq!(circ.extrusion.z, 1.0);
    }
}
