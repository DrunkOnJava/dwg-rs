//! XLINE entity (§19.4.58) — infinite line through a point.
//!
//! Same stream shape as RAY (a point + a direction vector). The only
//! semantic difference is that XLINE extends in both directions
//! from `point`; RAY is one-sided.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct XLine {
    pub point: Point3D,
    pub direction: Vec3D,
}

/// Decodes the `XLine` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>) -> Result<XLine> {
    let point = read_bd3(c)?;
    let direction = read_bd3(c)?;
    Ok(XLine { point, direction })
}
