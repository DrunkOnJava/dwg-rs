//! DIMENSION (angular, 3-point) — L4-20, ODA §19.4.21.
//!
//! Three-point angular dimension: a vertex and two extension points
//! define the angle. The dimension line follows an arc centered at
//! the vertex.
//!
//! # Stream shape (suffix only)
//!
//! ```text
//! BD3  center                   -- vertex of the angle ("def point 10")
//! BD3  first_extension_point    -- one ray of the angle ("def point 13")
//! BD3  second_extension_point   -- other ray of the angle ("def point 14")
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::dimension::DimensionCommon;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::version::Version;

/// Angular-3-point DIMENSION suffix (§19.4.21).
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionAngular3PSuffix {
    pub center: Point3D,
    pub first_extension_point: Point3D,
    pub second_extension_point: Point3D,
}

/// Full angular-3-point DIMENSION.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionAngular3P {
    pub common: DimensionCommon,
    pub suffix: DimensionAngular3PSuffix,
}

/// Decode the angular-3-point dimension suffix.
pub fn decode(
    c: &mut BitCursor<'_>,
    common: &DimensionCommon,
    _version: Version,
) -> Result<DimensionAngular3P> {
    let center = read_bd3(c)?;
    let first_extension_point = read_bd3(c)?;
    let second_extension_point = read_bd3(c)?;
    Ok(DimensionAngular3P {
        common: common.clone(),
        suffix: DimensionAngular3PSuffix {
            center,
            first_extension_point,
            second_extension_point,
        },
    })
}
