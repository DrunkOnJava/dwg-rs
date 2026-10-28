//! DIMENSION (angular, 2-line) — L4-20, ODA §19.4.20.
//!
//! Two-line angular dimension: the angle is defined by two line
//! segments. Four points are stored — two per line — and the final
//! point additionally defines the dimension arc.
//!
//! # Stream shape (suffix only)
//!
//! ```text
//! BD3  extension_line_1a  -- first line start  ("def point 13")
//! BD3  extension_line_1b  -- first line end    ("def point 14")
//! BD3  extension_line_2a  -- second line start ("def point 15")
//! BD3  extension_line_2b  -- second line end   ("def point 10"),
//!                            also defines the arc location
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::dimension::DimensionCommon;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::version::Version;

/// Angular-2-line DIMENSION suffix (§19.4.20).
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionAngular2LSuffix {
    pub extension_line_1a: Point3D,
    pub extension_line_1b: Point3D,
    pub extension_line_2a: Point3D,
    pub extension_line_2b: Point3D,
}

/// Full angular-2-line DIMENSION.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionAngular2L {
    pub common: DimensionCommon,
    pub suffix: DimensionAngular2LSuffix,
}

/// Decode the angular-2-line dimension suffix.
pub fn decode(
    c: &mut BitCursor<'_>,
    common: &DimensionCommon,
    _version: Version,
) -> Result<DimensionAngular2L> {
    let extension_line_1a = read_bd3(c)?;
    let extension_line_1b = read_bd3(c)?;
    let extension_line_2a = read_bd3(c)?;
    let extension_line_2b = read_bd3(c)?;
    Ok(DimensionAngular2L {
        common: common.clone(),
        suffix: DimensionAngular2LSuffix {
            extension_line_1a,
            extension_line_1b,
            extension_line_2a,
            extension_line_2b,
        },
    })
}
