//! ELLIPSE entity (§19.4.17).
//!
//! # Stream shape
//!
//! ```text
//! BD3 center
//! BD3 major-axis endpoint (relative to center, vector form)
//! BD3 extrusion (unnormalized; overrides BE for ellipse entities)
//! BD  axis-ratio         -- minor/major ∈ (0..1]
//! BD  start-parameter    -- parametric angle along the ellipse
//! BD  end-parameter
//! ```
//!
//! Unlike most entities, ELLIPSE writes a full 3-double extrusion
//! vector (not BE) so that non-Z-up ellipses can be expressed with
//! a single code path.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Ellipse {
    pub center: Point3D,
    /// Vector from center to the end of the major axis.
    pub major_axis: Vec3D,
    pub extrusion: Vec3D,
    pub axis_ratio: f64,
    pub start_param: f64,
    pub end_param: f64,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<Ellipse> {
    let center = read_bd3(c)?;
    let major_axis = read_bd3(c)?;
    let extrusion = read_bd3(c)?;
    let axis_ratio = c.read_bd()?;
    let start_param = c.read_bd()?;
    let end_param = c.read_bd()?;
    Ok(Ellipse {
        center,
        major_axis,
        extrusion,
        axis_ratio,
        start_param,
        end_param,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_ellipse() {
        let mut w = BitWriter::new();
        // center
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // major axis (10, 0, 0)
        w.write_bd(10.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // extrusion (0, 0, 1)
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        // axis_ratio
        w.write_bd(0.5);
        w.write_bd(0.0);
        w.write_bd(std::f64::consts::TAU);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let e = decode(&mut c).unwrap();
        assert_eq!(e.axis_ratio, 0.5);
        assert_eq!(e.major_axis.x, 10.0);
    }
}
