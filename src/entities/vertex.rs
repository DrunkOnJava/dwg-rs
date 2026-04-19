//! VERTEX entity (§19.4.55..§19.4.57) — single vertex of the legacy
//! POLYLINE entity. Newer drawings use LWPOLYLINE (one entity holds
//! all vertices). POLYLINE vertices come in three flavors:
//!
//! | Variant   | Type code (pre-R2010) | Used by |
//! |-----------|-----------------------|---------|
//! | VERTEX_2D | 10 (`0x0A`)           | 2D POLYLINE |
//! | VERTEX_3D | 11 (`0x0B`)           | 3D POLYLINE |
//! | VERTEX_MESH | 12..13              | PFACE MESH / POLYFACEMESH |
//!
//! # Stream shape (2D variant — the common one)
//!
//! ```text
//! RC   flag             -- bits: 0x01 extra vertex follows,
//!                          0x02 tangent present, 0x04 not used,
//!                          0x08 plinefit spline control point,
//!                          0x10 plinefit spline frame control,
//!                          0x20 3D polyline vertex,
//!                          0x40 3D polygon mesh vertex,
//!                          0x80 polyface mesh vertex
//! BD3  location
//! BD   start_width
//! BD   end_width
//! BD   bulge
//! BL   vertex_id        -- R2010+
//! BD   tangent_direction  -- only if (flag & 0x02)
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    pub flag: u8,
    pub location: Point3D,
    pub start_width: f64,
    pub end_width: f64,
    pub bulge: f64,
    pub vertex_id: Option<u32>,
    pub tangent_direction: Option<f64>,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Vertex> {
    let flag = c.read_rc()?;
    let location = read_bd3(c)?;
    let start_width = c.read_bd()?;
    let end_width = c.read_bd()?;
    let bulge = c.read_bd()?;
    let vertex_id = if version.is_r2010_plus() {
        Some(c.read_bl()? as u32)
    } else {
        None
    };
    let tangent_direction = if flag & 0x02 != 0 {
        Some(c.read_bd()?)
    } else {
        None
    };
    Ok(Vertex {
        flag,
        location,
        start_width,
        end_width,
        bulge,
        vertex_id,
        tangent_direction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_simple_vertex_r2000() {
        let mut w = BitWriter::new();
        w.write_rc(0x00); // no flags
        w.write_bd(1.0);
        w.write_bd(2.0);
        w.write_bd(3.0);
        w.write_bd(0.0); // start width
        w.write_bd(0.0); // end width
        w.write_bd(0.0); // bulge
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let v = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(
            v.location,
            Point3D {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }
        );
        assert!(v.vertex_id.is_none());
        assert!(v.tangent_direction.is_none());
    }

    #[test]
    fn roundtrip_vertex_with_tangent_r2018() {
        let mut w = BitWriter::new();
        w.write_rc(0x02); // tangent present
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // start width
        w.write_bd(2.0); // end width
        w.write_bd(0.5); // bulge
        w.write_bl(42); // vertex id
        w.write_bd(std::f64::consts::FRAC_PI_4); // tangent
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let v = decode(&mut c, Version::R2018).unwrap();
        assert_eq!(v.vertex_id, Some(42));
        assert!((v.tangent_direction.unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert_eq!(v.bulge, 0.5);
        assert_eq!(v.start_width, 1.0);
        assert_eq!(v.end_width, 2.0);
    }
}
