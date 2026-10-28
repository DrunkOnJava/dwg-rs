//! REVOLVEDSURFACE entity — ODA Open Design Specification v5.4.1
//! §19.4.79 (L4-38 in the entity inventory).
//!
//! A revolved surface is a 2D profile rotated around an axis by a
//! signed sweep angle. Like the other SURFACE variants it caches the
//! resulting geometry as an ACIS SAT blob and stores the parametric
//! axis + angle so callers can re-sweep without parsing SAT.
//!
//! # Stream shape
//!
//! ```text
//! <SAT blob>              -- crate::entities::modeler::decode_sat_blob
//! BD3 axis_origin         -- a point on the axis, WCS
//! BD3 axis_direction      -- axis unit vector (WCS); sense encodes rotation dir
//! BD  sweep_angle         -- radians; sign follows right-hand rule
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::modeler::{SatBlob, decode_sat_blob};
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct RevolvedSurface {
    /// Opaque ACIS SAT body — may be empty.
    pub sat: SatBlob,
    /// A world-space point on the axis of revolution.
    pub axis_origin: Point3D,
    /// Axis direction in WCS (usually, but not required to be, a
    /// unit vector — callers should normalize if they need one).
    pub axis_direction: Vec3D,
    /// Signed sweep angle in radians. Right-hand rule relative to
    /// `axis_direction`. 2π for a closed solid of revolution.
    pub sweep_angle: f64,
}

/// Decodes the `RevolvedSurface` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>) -> Result<RevolvedSurface> {
    let sat = decode_sat_blob(c)?;
    let axis_origin = read_bd3(c)?;
    let axis_direction = read_bd3(c)?;
    let sweep_angle = c.read_bd()?;
    Ok(RevolvedSurface {
        sat,
        axis_origin,
        axis_direction,
        sweep_angle,
    })
}
