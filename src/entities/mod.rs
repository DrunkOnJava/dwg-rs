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
//! | LINE           | [`self::line`]    | §19.4.20     |
//! | POINT          | [`point`]         | §19.4.27     |
//! | CIRCLE         | [`circle`]        | §19.4.8      |
//! | ARC            | [`arc`]           | §19.4.2      |
//! | ELLIPSE        | [`ellipse`]       | §19.4.17     |
//! | LWPOLYLINE     | [`lwpolyline`]    | §19.4.25     |
//! | MESH           | [`mesh`]          | §19.4.66     |
//! | MLINE          | [`mline`]         | §19.4.71     |
//! | POLYFACE_MESH  | [`polyface_mesh`] | §19.4.29     |
//! | POLYGON_MESH   | [`polygon_mesh`]  | §19.4.30     |
//! | IMAGE          | [`image`]         | §19.4.35     |
//! | IMAGEDEF       | [`imagedef`]      | §19.5.26     |
//! | PROXY ENTITY   | [`proxy_entity_passthrough`] | §19.4.91 |
//! | TEXT           | [`text`]          | §19.4.46     |
//! | INSERT         | [`insert`]        | §19.4.34     |
//! | RAY            | [`ray`]           | §19.4.48     |
//! | XLINE          | [`xline`]         | §19.4.58     |
//! | SOLID          | [`solid`]         | §19.4.43     |
//! | 3DFACE         | [`three_d_face`]  | §19.4.32     |
//! | 3DSOLID        | [`three_d_solid`] | §19.4.42     |
//! | REGION         | [`region`]        | §19.4.43     |
//! | BODY           | [`body`]          | §19.4.44     |
//! | VIEWPORT (stub)| [`viewport`]      | §19.4.60     |
//!
//! # Shared types
//!
//! - [`Point2D`]: 2-element f64 coordinate pair.
//! - [`Point3D`]: 3-element f64 coordinate triple.
//! - [`Vec3D`]: 3D direction vector (same shape as `Point3D`, distinct
//!   semantically).
//! - [`read_be`]: the "extrusion / normal vector" shortcut of
//!   spec §2.11 (BE). Either defaults to `(0,0,1)` or reads 3 BDs.

use crate::bitcursor::BitCursor;
use crate::error::Result;

pub mod arc;
pub mod attdef;
pub mod attrib;
pub mod block;
pub mod body;
pub mod camera;
pub mod circle;
pub mod dimension;
pub mod dimension_aligned;
pub mod dimension_angular_2l;
pub mod dimension_angular_3p;
pub mod dimension_diameter;
pub mod dimension_linear;
pub mod dimension_ordinate;
pub mod dimension_radial;
pub mod dispatch;
pub mod ellipse;
pub mod endblk;
pub mod extruded_surface;
pub mod geodata;
pub mod hatch;
pub mod helix;
pub mod image;
pub mod imagedef;
pub mod insert;
pub mod leader;
pub mod light;
pub mod line;
pub mod lofted_surface;
pub mod lwpolyline;
pub mod mesh;
pub mod mleader;
pub mod mline;
pub mod modeler;
pub mod mtext;
pub mod ole2_frame;
pub mod point;
pub mod polyface_mesh;
pub mod polygon_mesh;
pub mod polyline;
pub mod proxy_entity_passthrough;
pub mod ray;
pub mod region;
pub mod revolved_surface;
pub mod solid;
pub mod spline;
pub mod sun;
pub mod swept_surface;
pub mod text;
pub mod three_d_face;
pub mod three_d_solid;
pub mod tolerance;
pub mod trace;
pub mod underlay;
pub mod vertex;
pub mod viewport;
pub mod wipeout;
pub mod xline;

pub use dispatch::{
    DecodedEntity, DispatchSummary, decode_from_raw, decode_from_raw_with_class_map,
};

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
    if c.read_b()? { Ok(0.0) } else { c.read_bd() }
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
        assert_eq!(
            e,
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0
            }
        );
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
        assert_eq!(
            e,
            Vec3D {
                x: 1.5,
                y: 2.5,
                z: 3.5
            }
        );
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
