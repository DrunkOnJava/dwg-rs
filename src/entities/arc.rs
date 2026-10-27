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
use crate::entities::{Point3D, Vec3D, read_be_for, read_bt_for};
use crate::error::Result;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Arc {
    pub center: Point3D,
    pub radius: f64,
    pub thickness: f64,
    pub extrusion: Vec3D,
    pub start_angle: f64,
    pub end_angle: f64,
}

/// Decodes the `Arc` payload that follows the common entity header.
///
/// Reads the R2000+ form; see [`decode_for`] for R13-R14, whose `BT` and `BE` are a bare `BD` and `3BD` (§2.8, §2.10).
pub fn decode(c: &mut BitCursor<'_>) -> Result<Arc> {
    decode_for(c, Version::R2000)
}

/// Decodes the `Arc` payload for `version`: the `BT` thickness and `BE` extrusion take their R13-R14 form (§2.8, §2.10) there.
pub fn decode_for(c: &mut BitCursor<'_>, version: Version) -> Result<Arc> {
    let cx = c.read_bd()?;
    let cy = c.read_bd()?;
    let cz = c.read_bd()?;
    let radius = c.read_bd()?;
    let thickness = read_bt_for(c, version)?;
    let extrusion = read_be_for(c, version)?;
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
