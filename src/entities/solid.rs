//! SOLID entity (§19.4.43) — 3 or 4 corner filled quad/triangle.
//!
//! # Stream shape
//!
//! ```text
//! BT  thickness
//! BD  elevation          -- z coordinate shared by all 4 corners
//! RD  corner_1.x
//! RD  corner_1.y
//! RD  corner_2.x
//! RD  corner_2.y
//! RD  corner_3.x
//! RD  corner_3.y
//! RD  corner_4.x
//! RD  corner_4.y
//! BE  extrusion
//! ```
//!
//! When a SOLID represents a triangle, corner_3 == corner_4. The
//! coordinate order defines a bow-tie when naively connected — real
//! renderers draw the implicit "quadrilateral" 1–2–4–3 (yes, swapped)
//! per AutoCAD's convention.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Vec3D, read_be, read_bt};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    pub thickness: f64,
    pub elevation: f64,
    pub corners: [Point2D; 4],
    pub extrusion: Vec3D,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<Solid> {
    let thickness = read_bt(c)?;
    let elevation = c.read_bd()?;
    let mut corners = [Point2D::default(); 4];
    for corner in &mut corners {
        corner.x = c.read_rd()?;
        corner.y = c.read_rd()?;
    }
    let extrusion = read_be(c)?;
    Ok(Solid {
        thickness,
        elevation,
        corners,
        extrusion,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_solid_quad() {
        let mut w = BitWriter::new();
        w.write_b(true); // default thickness
        w.write_bd(0.0); // elevation
        w.write_rd(0.0); w.write_rd(0.0); // (0,0)
        w.write_rd(1.0); w.write_rd(0.0); // (1,0)
        w.write_rd(0.0); w.write_rd(1.0); // (0,1)
        w.write_rd(1.0); w.write_rd(1.0); // (1,1)
        w.write_b(true); // default extrusion
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert_eq!(s.thickness, 0.0);
        assert_eq!(s.elevation, 0.0);
        assert_eq!(s.corners[0], Point2D { x: 0.0, y: 0.0 });
        assert_eq!(s.corners[3], Point2D { x: 1.0, y: 1.0 });
    }
}
