//! INSERT entity (§19.4.34) — block reference.
//!
//! An INSERT places one instance of a BLOCK (external or internal)
//! at a specific insertion point with optional scale, rotation, and
//! rectangular array parameters.
//!
//! # Stream shape
//!
//! ```text
//! BD3  insertion_point
//! BB   scale_flag        -- 00=all three BD, 01=scaled by BD-shared,
//!                           10=scaled (1.0, 1.0, 1.0) implicit,
//!                           11=reserved
//! (scale_flag == 00)
//!   BD  scale_x
//!   BD  scale_y
//!   BD  scale_z
//! (scale_flag == 01)
//!   BD  scale_x  (y and z = scale_x)
//! BD   rotation
//! BE   extrusion
//! B    has_attribs       -- legacy; modern DWG always false, attribs
//!                           come in via sub-entities referenced by
//!                           following handles
//! (if has_attribs in R13-R14 only)
//!   BL  num_attribs
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3, read_be};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Insert {
    pub insertion_point: Point3D,
    pub scale: Point3D,
    pub rotation: f64,
    pub extrusion: Vec3D,
    pub has_attribs: bool,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<Insert> {
    let insertion_point = read_bd3(c)?;
    let scale_flag = c.read_bb()?;
    let scale = match scale_flag {
        0b00 => Point3D {
            x: c.read_bd()?,
            y: c.read_bd()?,
            z: c.read_bd()?,
        },
        0b01 => {
            let s = c.read_bd()?;
            Point3D { x: s, y: s, z: s }
        }
        0b10 => Point3D { x: 1.0, y: 1.0, z: 1.0 },
        _ => Point3D { x: 1.0, y: 1.0, z: 1.0 },
    };
    let rotation = c.read_bd()?;
    let extrusion = read_be(c)?;
    let has_attribs = c.read_b()?;
    Ok(Insert {
        insertion_point,
        scale,
        rotation,
        extrusion,
        has_attribs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_insert_unit_scale() {
        let mut w = BitWriter::new();
        w.write_bd(5.0);
        w.write_bd(10.0);
        w.write_bd(0.0);
        w.write_bb(0b10); // scale flag = unit
        w.write_bd(0.0); // rotation
        w.write_b(true); // default extrusion
        w.write_b(false); // no attribs
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let i = decode(&mut c).unwrap();
        assert_eq!(i.insertion_point, Point3D { x: 5.0, y: 10.0, z: 0.0 });
        assert_eq!(i.scale, Point3D { x: 1.0, y: 1.0, z: 1.0 });
        assert!(!i.has_attribs);
    }

    #[test]
    fn roundtrip_insert_non_uniform_scale() {
        let mut w = BitWriter::new();
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bb(0b00); // explicit xyz
        w.write_bd(2.0);
        w.write_bd(3.0);
        w.write_bd(4.0);
        w.write_bd(std::f64::consts::FRAC_PI_4);
        w.write_b(true);
        w.write_b(false);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let i = decode(&mut c).unwrap();
        assert_eq!(i.scale, Point3D { x: 2.0, y: 3.0, z: 4.0 });
        assert!((i.rotation - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    }
}
