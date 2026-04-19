//! VIEW table entry (§19.5.58) — saved 3D camera setup.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Point3D, read_bd3};
use crate::error::Result;
use crate::tables::{TableEntryHeader, read_table_entry_header};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub header: TableEntryHeader,
    pub view_height: f64,
    pub view_width: f64,
    pub view_center: Point2D,
    pub target: Point3D,
    pub view_direction: Point3D,
    pub twist_angle: f64,
    pub lens_length: f64,
    pub front_clip: f64,
    pub back_clip: f64,
    pub render_mode: u8,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<View> {
    let header = read_table_entry_header(c, version)?;
    let view_height = c.read_bd()?;
    let view_width = c.read_bd()?;
    let vcx = c.read_bd()?;
    let vcy = c.read_bd()?;
    let target = read_bd3(c)?;
    let view_direction = read_bd3(c)?;
    let twist_angle = c.read_bd()?;
    let lens_length = c.read_bd()?;
    let front_clip = c.read_bd()?;
    let back_clip = c.read_bd()?;
    let render_mode = c.read_rc()?;
    Ok(View {
        header,
        view_height,
        view_width,
        view_center: Point2D { x: vcx, y: vcy },
        target,
        view_direction,
        twist_angle,
        lens_length,
        front_clip,
        back_clip,
        render_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_isometric_view() {
        let mut w = BitWriter::new();
        // Header
        let s = b"SW-Iso";
        w.write_bs_u(s.len() as u16);
        for b in s { w.write_rc(*b); }
        w.write_b(false); w.write_bs(0); w.write_b(false);
        // View body
        w.write_bd(10.0);
        w.write_bd(20.0);
        w.write_bd(5.0); w.write_bd(5.0);
        w.write_bd(0.0); w.write_bd(0.0); w.write_bd(0.0); // target
        w.write_bd(-1.0); w.write_bd(-1.0); w.write_bd(1.0); // view dir
        w.write_bd(0.0); // twist
        w.write_bd(50.0); // lens
        w.write_bd(0.0);  // front
        w.write_bd(0.0);  // back
        w.write_rc(0x04); // render mode
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let v = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(v.header.name, "SW-Iso");
        assert_eq!(v.view_width, 20.0);
        assert_eq!(v.lens_length, 50.0);
        assert_eq!(v.render_mode, 0x04);
    }
}
