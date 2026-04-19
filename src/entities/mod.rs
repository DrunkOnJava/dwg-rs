//! Entity decoders — spec §19.4.x.
//!
//! Each submodule parses one entity type's type-specific payload. The
//! caller is expected to have already consumed the object header
//! (type code, size, handle) and the common entity preamble
//! ([`crate::common_entity`]) before invoking a decoder here.
//!
//! # Coverage
//!
//! | Entity         | Module            | Spec section |
//! |----------------|-------------------|--------------|
//! | LINE           | [`line`]          | §19.4.20     |
//! | POINT          | [`point`]         | §19.4.27     |
//! | CIRCLE         | [`circle`]        | §19.4.8      |
//! | ARC            | [`arc`]           | §19.4.2      |
//! | ELLIPSE        | [`ellipse`]       | §19.4.17     |
//! | LWPOLYLINE     | [`lwpolyline`]    | §19.4.25     |
//! | TEXT           | [`text`]          | §19.4.46     |
//! | INSERT         | [`insert`]        | §19.4.34     |
//! | RAY            | [`ray`]           | §19.4.48     |
//! | XLINE          | [`xline`]         | §19.4.58     |
//! | SOLID          | [`solid`]         | §19.4.43     |
//! | 3DFACE         | [`three_d_face`]  | §19.4.32     |
//! | VIEWPORT (stub)| [`viewport`]      | §19.4.60     |
//!
//! # Shared types
//!
//! - [`Point2D`]: 2-element f64 coordinate pair.
//! - [`Point3D`]: 3-element f64 coordinate triple.
//! - [`Vec3D`]: 3D direction vector (same shape as `Point3D`, distinct
//!   semantically).
//! - [`Extrusion`]: the "extrusion / normal vector" shortcut of
//!   spec §2.11 (BE). Either defaults to `(0,0,1)` or reads 3 BDs.

use crate::bitcursor::BitCursor;
use crate::error::Result;

pub mod arc;
pub mod attdef;
pub mod attrib;
pub mod block;
pub mod circle;
pub mod dimension;
pub mod ellipse;
pub mod endblk;
pub mod hatch;
pub mod image;
pub mod insert;
pub mod leader;
pub mod line;
pub mod lwpolyline;
pub mod mleader;
pub mod mtext;
pub mod point;
pub mod polyline;
pub mod ray;
pub mod solid;
pub mod spline;
pub mod text;
pub mod three_d_face;
pub mod trace;
pub mod vertex;
pub mod viewport;
pub mod xline;

/// A 2D point: (x, y) in WCS (World Coordinate System).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

/// A 3D point: (x, y, z) in WCS.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A 3D vector — same shape as [`Point3D`], used to differentiate
/// direction/normal semantics in decoded entity shapes.
pub type Vec3D = Point3D;

/// Read a 3D point encoded as three BDs.
pub fn read_bd3(c: &mut BitCursor<'_>) -> Result<Point3D> {
    let x = c.read_bd()?;
    let y = c.read_bd()?;
    let z = c.read_bd()?;
    Ok(Point3D { x, y, z })
}

/// Read a 2D point encoded as two BDs.
pub fn read_bd2(c: &mut BitCursor<'_>) -> Result<Point2D> {
    let x = c.read_bd()?;
    let y = c.read_bd()?;
    Ok(Point2D { x, y })
}

/// Read a 3D point encoded as three RDs (raw doubles).
pub fn read_rd3(c: &mut BitCursor<'_>) -> Result<Point3D> {
    let x = c.read_rd()?;
    let y = c.read_rd()?;
    let z = c.read_rd()?;
    Ok(Point3D { x, y, z })
}

/// Read a 2D point encoded as two RDs.
pub fn read_rd2(c: &mut BitCursor<'_>) -> Result<Point2D> {
    let x = c.read_rd()?;
    let y = c.read_rd()?;
    Ok(Point2D { x, y })
}

/// Read a BE (BitExtrusion) per spec §2.11.
///
/// # Encoding
///
/// A single bit flag:
/// - `1` ⇒ extrusion = `(0, 0, 1)` (the common case — XY plane)
/// - `0` ⇒ three BDs follow for (x, y, z)
pub fn read_be(c: &mut BitCursor<'_>) -> Result<Vec3D> {
    if c.read_b()? {
        Ok(Vec3D {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        })
    } else {
        read_bd3(c)
    }
}

/// Read a BT (BitThickness) per spec §2.12 — defaults to 0.0 if the
/// one-bit flag is set; otherwise reads a BD.
pub fn read_bt(c: &mut BitCursor<'_>) -> Result<f64> {
    if c.read_b()? {
        Ok(0.0)
    } else {
        c.read_bd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn be_default_roundtrip() {
        let mut w = BitWriter::new();
        w.write_b(true); // default
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let e = read_be(&mut c).unwrap();
        assert_eq!(e, Vec3D { x: 0.0, y: 0.0, z: 1.0 });
    }

    #[test]
    fn be_explicit_roundtrip() {
        let mut w = BitWriter::new();
        w.write_b(false);
        w.write_bd(1.5);
        w.write_bd(2.5);
        w.write_bd(3.5);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let e = read_be(&mut c).unwrap();
        assert_eq!(e, Vec3D { x: 1.5, y: 2.5, z: 3.5 });
    }

    #[test]
    fn bt_default_roundtrip() {
        let mut w = BitWriter::new();
        w.write_b(true);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let t = read_bt(&mut c).unwrap();
        assert_eq!(t, 0.0);
    }

    #[test]
    fn bt_explicit_roundtrip() {
        let mut w = BitWriter::new();
        w.write_b(false);
        w.write_bd(4.25);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let t = read_bt(&mut c).unwrap();
        assert_eq!(t, 4.25);
    }
}
