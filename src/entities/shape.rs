//! SHAPE entity (§20.4.37, type 0x21) — one glyph of a shape file, placed
//! in the drawing.
//!
//! A SHAPE carries the number of its shape in the shape file of its text
//! style, not the shape's name; the DXF writes the name (group 2), which
//! this crate cannot recover without reading the shape file (§20.4.56), so
//! that group is not compared. The field list is the one §20.4.37 prints,
//! with no release-dependent branch:
//!
//! ```text
//! 3BD   insertion point    DXF 10, 20, 30
//! BD    scale              DXF 40, default 1
//! BD    rotation           DXF 50, radians, default 0
//! BD    width factor       DXF 41, default 1
//! BD    oblique            DXF 51, radians, default 0
//! BD    thickness          DXF 39
//! BS    shape number       DXF 2 (by name)
//! 3BD   extrusion          DXF 210, 220, 230
//! H     SHAPEFILE          hard pointer, in the handle stream
//! ```
//!
//! The one SHAPE of each release of the public corpus (ACadSharp's
//! `sample_AC10xx`) closes on its data-stream boundary from R14 to R2018.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::Result;

/// A decoded SHAPE.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// `3BD` insertion point, DXF 10, 20, 30.
    pub insertion_point: Point3D,
    /// `BD` scale, DXF 40.
    pub scale: f64,
    /// `BD` rotation in radians, DXF 50.
    pub rotation: f64,
    /// `BD` width factor, DXF 41.
    pub width_factor: f64,
    /// `BD` oblique angle in radians, DXF 51.
    pub oblique: f64,
    /// `BD` thickness, DXF 39.
    pub thickness: f64,
    /// `BS` index of the shape in the shape file.
    pub shape_number: i16,
    /// `3BD` extrusion, DXF 210, 220, 230.
    pub extrusion: Vec3D,
}

/// Decode a SHAPE's field list (§20.4.37) past the common entity data.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Shape> {
    Ok(Shape {
        insertion_point: read_bd3(c)?,
        scale: c.read_bd()?,
        rotation: c.read_bd()?,
        width_factor: c.read_bd()?,
        oblique: c.read_bd()?,
        thickness: c.read_bd()?,
        shape_number: c.read_bs()?,
        extrusion: read_bd3(c)?,
    })
}
