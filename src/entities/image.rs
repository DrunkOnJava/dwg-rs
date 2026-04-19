//! IMAGE entity (§19.4.35) — external raster image reference.
//!
//! IMAGE places a raster image (PNG, JPEG, TIFF) into a drawing.
//! The image bits themselves live in an external file (or an
//! ACAD_IMAGE_DEF entry in the named-object dictionary); the IMAGE
//! entity carries only the placement + clipping geometry + display
//! flags.
//!
//! # Stream shape
//!
//! ```text
//! BL    class_version    -- always 0
//! BD3   insertion_point
//! BD3   u_vector         -- image-space X axis scaled to WCS
//! BD3   v_vector         -- image-space Y axis scaled to WCS
//! RD2   image_size       -- (width, height) in image pixels
//! BS    display_flags    -- bits: 0x01 show, 0x02 show_when_not_aligned,
//!                           0x04 use_clipping, 0x08 transparent
//! B     clipping
//! RC    brightness       -- 0..100
//! RC    contrast         -- 0..100
//! RC    fade             -- 0..100
//! (R2010+) B clip_mode    -- 0 = outside, 1 = inside
//! BS    clip_boundary_type  -- 1 = rectangle, 2 = polygon
//! BL    num_clip_verts
//! (if clip_boundary_type == 1)
//!   RD2  corner_1
//!   RD2  corner_2
//! (else)
//!   RD2 × num_clip_verts
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub insertion_point: Point3D,
    pub u_vector: Vec3D,
    pub v_vector: Vec3D,
    pub image_size: Point2D,
    pub display_flags: i16,
    pub clipping: bool,
    pub brightness: u8,
    pub contrast: u8,
    pub fade: u8,
    pub clip_mode: bool,
    pub clip_boundary: ClipBoundary,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClipBoundary {
    Rectangle {
        lower_left: Point2D,
        upper_right: Point2D,
    },
    Polygon(Vec<Point2D>),
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Image> {
    let _class_version = c.read_bl()?;
    let insertion_point = read_bd3(c)?;
    let u_vector = read_bd3(c)?;
    let v_vector = read_bd3(c)?;
    let image_size = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let display_flags = c.read_bs()?;
    let clipping = c.read_b()?;
    let brightness = c.read_rc()?;
    let contrast = c.read_rc()?;
    let fade = c.read_rc()?;
    let clip_mode = if version.is_r2010_plus() {
        c.read_b()?
    } else {
        false
    };
    let clip_boundary_type = c.read_bs()?;
    let num_clip_verts = c.read_bl()? as usize;
    if num_clip_verts > 1_000_000 {
        return Err(Error::SectionMap(format!(
            "IMAGE clip verts {num_clip_verts} exceeds 1M cap"
        )));
    }
    let clip_boundary = match clip_boundary_type {
        1 => {
            let c1 = Point2D { x: c.read_rd()?, y: c.read_rd()? };
            let c2 = Point2D { x: c.read_rd()?, y: c.read_rd()? };
            ClipBoundary::Rectangle { lower_left: c1, upper_right: c2 }
        }
        2 => {
            let mut pts = Vec::with_capacity(num_clip_verts);
            for _ in 0..num_clip_verts {
                pts.push(Point2D {
                    x: c.read_rd()?,
                    y: c.read_rd()?,
                });
            }
            ClipBoundary::Polygon(pts)
        }
        other => {
            return Err(Error::SectionMap(format!(
                "IMAGE clip boundary type {other} not in {{1, 2}}"
            )));
        }
    };
    Ok(Image {
        insertion_point,
        u_vector,
        v_vector,
        image_size,
        display_flags,
        clipping,
        brightness,
        contrast,
        fade,
        clip_mode,
        clip_boundary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_image_with_rect_clip() {
        let mut w = BitWriter::new();
        w.write_bl(0);
        w.write_bd(10.0); w.write_bd(20.0); w.write_bd(0.0);
        w.write_bd(1.0); w.write_bd(0.0); w.write_bd(0.0);
        w.write_bd(0.0); w.write_bd(1.0); w.write_bd(0.0);
        w.write_rd(1920.0); w.write_rd(1080.0);
        w.write_bs(0x07); // show + show-not-aligned + clip
        w.write_b(true); // clipping
        w.write_rc(50); w.write_rc(50); w.write_rc(0);
        w.write_bs(1); // rectangle
        w.write_bl(2); // num verts
        w.write_rd(0.0); w.write_rd(0.0);
        w.write_rd(1920.0); w.write_rd(1080.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let i = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(i.image_size, Point2D { x: 1920.0, y: 1080.0 });
        assert!(matches!(i.clip_boundary, ClipBoundary::Rectangle { .. }));
    }

    #[test]
    fn roundtrip_image_with_poly_clip() {
        let mut w = BitWriter::new();
        w.write_bl(0);
        w.write_bd(0.0); w.write_bd(0.0); w.write_bd(0.0);
        w.write_bd(1.0); w.write_bd(0.0); w.write_bd(0.0);
        w.write_bd(0.0); w.write_bd(1.0); w.write_bd(0.0);
        w.write_rd(100.0); w.write_rd(100.0);
        w.write_bs(0x01);
        w.write_b(true);
        w.write_rc(50); w.write_rc(50); w.write_rc(0);
        w.write_bs(2); // polygon
        w.write_bl(3);
        for (x, y) in [(0.0f64, 0.0), (100.0, 0.0), (50.0, 100.0)] {
            w.write_rd(x); w.write_rd(y);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let i = decode(&mut c, Version::R2000).unwrap();
        match i.clip_boundary {
            ClipBoundary::Polygon(pts) => assert_eq!(pts.len(), 3),
            _ => panic!("expected polygon"),
        }
    }
}
