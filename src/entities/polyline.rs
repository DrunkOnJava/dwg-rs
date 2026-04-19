//! POLYLINE entity (§19.4.45) — legacy 2D/3D/mesh polyline.
//!
//! A POLYLINE holds flags and global width/thickness fields; the
//! actual vertex data lives in a chain of [`crate::entities::vertex::Vertex`]
//! sub-entities referenced by handle in the object stream.
//!
//! Modern AutoCAD writes LWPOLYLINE instead of POLYLINE for 2D work;
//! this decoder covers the legacy path and POLYLINE_3D / PFACE /
//! POLYMESH variants that still appear in older files.
//!
//! # Stream shape (2D POLYLINE — common variant)
//!
//! ```text
//! BS   flag             -- bits: 0x01 closed, 0x02 curve-fit, 0x04 spline-fit,
//!                          0x08 3D polyline, 0x10 3D polymesh, 0x20 closed
//!                          in N direction, 0x40 polyface, 0x80 linetype
//!                          generated continuously
//! BS   curve_type        -- 5=quadratic B-spline, 6=cubic, 8=Bezier
//! BD   default_start_width
//! BD   default_end_width
//! BT   thickness
//! BD   elevation
//! BE   extrusion
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Vec3D, read_be, read_bt};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    pub flag: u16,
    pub curve_type: i16,
    pub default_start_width: f64,
    pub default_end_width: f64,
    pub thickness: f64,
    pub elevation: f64,
    pub extrusion: Vec3D,
}

impl Polyline {
    pub fn is_closed(&self) -> bool {
        self.flag & 0x01 != 0
    }
    pub fn is_3d(&self) -> bool {
        self.flag & 0x08 != 0
    }
    pub fn is_polyface(&self) -> bool {
        self.flag & 0x40 != 0
    }
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<Polyline> {
    let flag = c.read_bs_u()?;
    let curve_type = c.read_bs()?;
    let default_start_width = c.read_bd()?;
    let default_end_width = c.read_bd()?;
    let thickness = read_bt(c)?;
    let elevation = c.read_bd()?;
    let extrusion = read_be(c)?;
    Ok(Polyline {
        flag,
        curve_type,
        default_start_width,
        default_end_width,
        thickness,
        elevation,
        extrusion,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_closed_2d_polyline() {
        let mut w = BitWriter::new();
        w.write_bs_u(0x01); // closed
        w.write_bs(0); // no curve fit
        w.write_bd(0.0); // start width
        w.write_bd(0.0); // end width
        w.write_b(true); // default thickness
        w.write_bd(0.0); // elevation
        w.write_b(true); // default extrusion
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c).unwrap();
        assert!(p.is_closed());
        assert!(!p.is_3d());
        assert_eq!(p.thickness, 0.0);
    }

    #[test]
    fn roundtrip_3d_polyline() {
        let mut w = BitWriter::new();
        w.write_bs_u(0x08); // 3D
        w.write_bs(6); // cubic B-spline
        w.write_bd(0.1);
        w.write_bd(0.2);
        w.write_b(false); // explicit thickness
        w.write_bd(1.5);
        w.write_bd(10.0); // elevation
        w.write_b(true); // default extrusion
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c).unwrap();
        assert!(p.is_3d());
        assert_eq!(p.curve_type, 6);
        assert_eq!(p.thickness, 1.5);
        assert_eq!(p.elevation, 10.0);
    }
}
