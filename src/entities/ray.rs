//! RAY entity (§19.4.48) — infinite half-line.
//!
//! # Stream shape
//!
//! ```text
//! BD3 point            -- start point
//! BD3 vector           -- direction vector (unnormalized in stream,
//!                         but conventionally unit-length)
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Ray {
    pub start: Point3D,
    pub direction: Vec3D,
}

/// Decodes the `Ray` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Ray> {
    let start = read_bd3(c)?;
    let direction = read_bd3(c)?;
    Ok(Ray { start, direction })
}
