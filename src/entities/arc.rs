//! ARC entity (§19.4.2).
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
//! BD  start_angle       -- radians
//! BD  end_angle         -- radians, CCW from start
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_be, read_bt};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Arc {
    pub center: Point3D,
    pub radius: f64,
    pub thickness: f64,
    pub extrusion: Vec3D,
    pub start_angle: f64,
    pub end_angle: f64,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<Arc> {
    let cx = c.read_bd()?;
    let cy = c.read_bd()?;
    let cz = c.read_bd()?;
    let radius = c.read_bd()?;
    let thickness = read_bt(c)?;
    let extrusion = read_be(c)?;
    let start_angle = c.read_bd()?;
    let end_angle = c.read_bd()?;
    Ok(Arc {
        center: Point3D {
            x: cx,
            y: cy,
            z: cz,
        },
        radius,
        thickness,
        extrusion,
        start_angle,
        end_angle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_arc_quarter_circle() {
        let mut w = BitWriter::new();
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(10.0);
        w.write_b(true);
        w.write_b(true);
        w.write_bd(0.0);
        w.write_bd(std::f64::consts::FRAC_PI_2);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let a = decode(&mut c).unwrap();
        assert_eq!(a.radius, 10.0);
        assert_eq!(a.start_angle, 0.0);
        assert!((a.end_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }
}
