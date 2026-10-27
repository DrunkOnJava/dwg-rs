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
use crate::entities::{Point3D, Vec3D, read_be_for, read_bt_for};
use crate::error::Result;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Circle {
    pub center: Point3D,
    pub radius: f64,
    pub thickness: f64,
    pub extrusion: Vec3D,
}

/// Decodes the `Circle` payload that follows the common entity header.
///
/// Reads the R2000+ form; see [`decode_for`] for R13-R14, whose `BT` and `BE` are a bare `BD` and `3BD` (§2.8, §2.10).
pub fn decode(c: &mut BitCursor<'_>) -> Result<Circle> {
    decode_for(c, Version::R2000)
}

/// Decodes the `Circle` payload for `version`: the `BT` thickness and `BE` extrusion take their R13-R14 form (§2.8, §2.10) there.
pub fn decode_for(c: &mut BitCursor<'_>, version: Version) -> Result<Circle> {
    let cx = c.read_bd()?;
    let cy = c.read_bd()?;
    let cz = c.read_bd()?;
    let radius = c.read_bd()?;
    let thickness = read_bt_for(c, version)?;
    let extrusion = read_be_for(c, version)?;
    Ok(Circle {
        center: Point3D {
            x: cx,
            y: cy,
            z: cz,
        },
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
        assert_eq!(
            circ.center,
            Point3D {
                x: 10.0,
                y: 20.0,
                z: 0.0
            }
        );
        assert_eq!(circ.radius, 5.0);
        assert_eq!(circ.thickness, 0.0);
        assert_eq!(circ.extrusion.z, 1.0);
    }
}
