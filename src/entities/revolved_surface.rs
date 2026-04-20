//! REVOLVEDSURFACE entity — ODA Open Design Specification v5.4.1
//! §19.4.79 (L4-38 in the entity inventory).
//!
//! A revolved surface is a 2D profile rotated around an axis by a
//! signed sweep angle. Like the other SURFACE variants it caches the
//! resulting geometry as an ACIS SAT blob and stores the parametric
//! axis + angle so callers can re-sweep without parsing SAT.
//!
//! # Stream shape
//!
//! ```text
//! <SAT blob>              -- crate::entities::modeler::decode_sat_blob
//! BD3 axis_origin         -- a point on the axis, WCS
//! BD3 axis_direction      -- axis unit vector (WCS); sense encodes rotation dir
//! BD  sweep_angle         -- radians; sign follows right-hand rule
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::modeler::{SatBlob, decode_sat_blob};
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct RevolvedSurface {
    /// Opaque ACIS SAT body — may be empty.
    pub sat: SatBlob,
    /// A world-space point on the axis of revolution.
    pub axis_origin: Point3D,
    /// Axis direction in WCS (usually, but not required to be, a
    /// unit vector — callers should normalize if they need one).
    pub axis_direction: Vec3D,
    /// Signed sweep angle in radians. Right-hand rule relative to
    /// `axis_direction`. 2π for a closed solid of revolution.
    pub sweep_angle: f64,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<RevolvedSurface> {
    let sat = decode_sat_blob(c)?;
    let axis_origin = read_bd3(c)?;
    let axis_direction = read_bd3(c)?;
    let sweep_angle = c.read_bd()?;
    Ok(RevolvedSurface {
        sat,
        axis_origin,
        axis_direction,
        sweep_angle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::entities::modeler::tests::write_sat_blob;
    use std::f64::consts::PI;

    #[test]
    fn roundtrip_full_revolution() {
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: false,
                version: 1,
                bytes: b"R-SAT".to_vec(),
            },
        );
        // axis origin at (0, 0, 0)
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // axis direction +Z
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        // 360°
        w.write_bd(2.0 * PI);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert_eq!(s.axis_origin, Point3D::default());
        assert_eq!(
            s.axis_direction,
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0
            }
        );
        assert!((s.sweep_angle - 2.0 * PI).abs() < 1e-12);
        assert_eq!(s.sat.version, 1);
    }

    #[test]
    fn roundtrip_partial_sweep_with_empty_sat() {
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: true,
                version: 0,
                bytes: Vec::new(),
            },
        );
        w.write_bd(5.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(PI / 2.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert!(s.sat.empty);
        assert!((s.sweep_angle - PI / 2.0).abs() < 1e-12);
    }
}
