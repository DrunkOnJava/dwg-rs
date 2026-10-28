//! DIMENSION (radius) and DIMENSION (diameter) — L4-19, ODA §19.4.22.
//!
//! Radius and diameter dimensions share the same stream layout — only
//! their object-type code (0x19 = radius, 0x1A = diameter) differs.
//! Both are decoded here with a common suffix form and two entry
//! points: [`decode_radius`] and [`decode_diameter`].
//!
//! # Stream shape (suffix only)
//!
//! ```text
//! BD3  center_point      -- "def point 10"
//! BD3  first_arc_point   -- "def point 15" (a point on the arc,
//!                           defining the measured chord)
//! BD   leader_length
//! ```
//!
//! The semantic difference between radius and diameter is in how the
//! measurement is drawn (arrow pointing inward vs spanning the
//! circle), not in how the geometry is stored. Parsers therefore
//! decode identically; downstream renderers may branch on kind.

use crate::bitcursor::BitCursor;
use crate::entities::dimension::DimensionCommon;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::version::Version;

/// Suffix for a radius or diameter dimension — they share the same
/// byte layout per §19.4.22.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionRadialSuffix {
    pub center_point: Point3D,
    pub first_arc_point: Point3D,
    pub leader_length: f64,
}

/// Radius DIMENSION (object-type code 0x19).
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionRadial {
    pub common: DimensionCommon,
    pub suffix: DimensionRadialSuffix,
}

/// Diameter DIMENSION (object-type code 0x1A).
///
/// Held in a distinct struct so callers can match on type at compile
/// time even though the bit-level decode is identical to radius.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionDiameter {
    pub common: DimensionCommon,
    pub suffix: DimensionRadialSuffix,
}

/// Decode the radius-dimension suffix.
pub fn decode_radius(
    c: &mut BitCursor<'_>,
    common: &DimensionCommon,
    _version: Version,
) -> Result<DimensionRadial> {
    let suffix = read_suffix(c)?;
    Ok(DimensionRadial {
        common: common.clone(),
        suffix,
    })
}

/// Decode the diameter-dimension suffix. Identical bit-level layout
/// to [`decode_radius`] — this helper exists so consumers can keep
/// the two dimensions on separate types.
pub fn decode_diameter(
    c: &mut BitCursor<'_>,
    common: &DimensionCommon,
    _version: Version,
) -> Result<DimensionDiameter> {
    let suffix = read_suffix(c)?;
    Ok(DimensionDiameter {
        common: common.clone(),
        suffix,
    })
}

fn read_suffix(c: &mut BitCursor<'_>) -> Result<DimensionRadialSuffix> {
    let center_point = read_bd3(c)?;
    let first_arc_point = read_bd3(c)?;
    let leader_length = c.read_bd()?;
    Ok(DimensionRadialSuffix {
        center_point,
        first_arc_point,
        leader_length,
    })
}
