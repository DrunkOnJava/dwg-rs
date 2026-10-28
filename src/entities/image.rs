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
//! (if clip_boundary_type == 1)
//!   RD2  corner_1        -- no vertex count precedes these
//!   RD2  corner_2
//! (else)
//!   BL   num_clip_verts
//!   RD2 × num_clip_verts
//! ```
//!
//! # Measured — the rectangle form carries no vertex count
//!
//! `sample_AC1032.dwg` (R2018) holds one IMAGE (handle `0x662`) and one
//! WIPEOUT (handle `0x44D`), which shares this layout. The IMAGE is a
//! 994 × 965 raster with `clip_boundary_type = 1`; its clip corners
//! `(-0.5, -0.5)` and `(993.5, 964.5)` occupy bits 1663..1919 of a
//! record whose data stream ends at bit 1920. Reading a `BL` count
//! between the type and the corners consumes the first 34 bits of the
//! `-0.5`, which both mis-decodes the rectangle and overruns the record.
//! The WIPEOUT has `clip_boundary_type = 2`, and there the `BL` count
//! *is* present: `4`, followed by four `RD2` vertices spanning bits
//! 1673..2185 of a record ending at 2186.

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

/// Decodes the `Image` payload that follows the common entity header.
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
    // Measured: the rectangle form (type 1) writes its two corners
    // straight after the type — no count field. See the module docs.
    let num_clip_verts = if clip_boundary_type == 1 {
        2
    } else {
        c.read_bl()? as usize
    };
    // Real IMAGE clip boundaries are a handful of vertices; 100K is a
    // conservative upper bound. Also cross-check against remaining
    // payload bits (each vertex is 128 bits — two RDs).
    const IMAGE_MAX_CLIP_VERTS: usize = 100_000;
    if num_clip_verts > IMAGE_MAX_CLIP_VERTS || num_clip_verts * 128 > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "IMAGE clip verts {num_clip_verts} exceeds cap \
             ({IMAGE_MAX_CLIP_VERTS} or remaining_bits {})",
            c.remaining_bits()
        )));
    }
    let clip_boundary = match clip_boundary_type {
        1 => {
            let c1 = Point2D {
                x: c.read_rd()?,
                y: c.read_rd()?,
            };
            let c2 = Point2D {
                x: c.read_rd()?,
                y: c.read_rd()?,
            };
            ClipBoundary::Rectangle {
                lower_left: c1,
                upper_right: c2,
            }
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
