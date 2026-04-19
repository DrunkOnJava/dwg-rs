//! VPORT table entry (§19.5.62) — viewport preset for model-space
//! layout.
//!
//! A VPORT stores the viewport's lower-left/upper-right screen
//! rectangle plus all camera/view parameters. The current active
//! viewport is the one named "*Active".

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Point3D, read_bd3};
use crate::error::Result;
use crate::tables::{TableEntryHeader, read_table_entry_header};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct VPort {
    pub header: TableEntryHeader,
    pub view_height: f64,
    pub aspect_ratio: f64,
    pub view_center: Point2D,
    pub view_target: Point3D,
    pub view_direction: Point3D,
    pub view_twist: f64,
    pub lens_length: f64,
    pub front_clip: f64,
    pub back_clip: f64,
    pub lower_left: Point2D,
    pub upper_right: Point2D,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<VPort> {
    let header = read_table_entry_header(c, version)?;
    let view_height = c.read_bd()?;
    let aspect_ratio = c.read_bd()?;
    let view_center = Point2D {
        x: c.read_bd()?,
        y: c.read_bd()?,
    };
    let view_target = read_bd3(c)?;
    let view_direction = read_bd3(c)?;
    let view_twist = c.read_bd()?;
    let lens_length = c.read_bd()?;
    let front_clip = c.read_bd()?;
    let back_clip = c.read_bd()?;
    let lower_left = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let upper_right = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    Ok(VPort {
        header,
        view_height,
        aspect_ratio,
        view_center,
        view_target,
        view_direction,
        view_twist,
        lens_length,
        front_clip,
        back_clip,
        lower_left,
        upper_right,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_active_vport() {
        let mut w = BitWriter::new();
        let s = b"*Active";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
        w.write_bd(10.0); // view height
        w.write_bd(1.5); // aspect ratio
        w.write_bd(0.0);
        w.write_bd(0.0); // center
        for _ in 0..3 {
            w.write_bd(0.0);
        } // target
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // direction
        w.write_bd(0.0); // twist
        w.write_bd(50.0); // lens
        w.write_bd(0.0);
        w.write_bd(0.0); // clips
        w.write_rd(0.0);
        w.write_rd(0.0); // lower left
        w.write_rd(1.0);
        w.write_rd(1.0); // upper right
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let v = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(v.header.name, "*Active");
        assert_eq!(v.aspect_ratio, 1.5);
        assert_eq!(v.upper_right, Point2D { x: 1.0, y: 1.0 });
    }
}
