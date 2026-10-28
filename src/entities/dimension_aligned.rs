//! DIMENSION (aligned) — L4-18, ODA §19.4.19.
//!
//! Aligned dimension subtype. The dimension line runs parallel to
//! the segment between the two extension-line origins, so no
//! `rotation` is stored (contrast with linear dimensions per
//! §19.4.18). Only the obliquing angle is kept.
//!
//! # Stream shape (suffix only)
//!
//! ```text
//! BD3  extension_line_1_origin   -- "def point 13"
//! BD3  extension_line_2_origin   -- "def point 14"
//! BD3  dim_line_definition_point -- "def point 10"
//! BD   oblique_angle_radians
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::dimension::DimensionCommon;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::version::Version;

/// Aligned DIMENSION suffix (§19.4.19).
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionAlignedSuffix {
    pub extension_line_1_origin: Point3D,
    pub extension_line_2_origin: Point3D,
    pub dim_line_definition_point: Point3D,
    pub oblique_angle_radians: f64,
}

/// Full aligned DIMENSION — shared preamble plus aligned-specific
/// suffix.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionAligned {
    pub common: DimensionCommon,
    pub suffix: DimensionAlignedSuffix,
}

/// Decode the aligned-dimension suffix from a bit stream.
///
/// Caller supplies the already-parsed [`DimensionCommon`]. The
/// `_version` parameter is kept for signature symmetry; this
/// subtype's suffix has no version gates.
pub fn decode(
    c: &mut BitCursor<'_>,
    common: &DimensionCommon,
    _version: Version,
) -> Result<DimensionAligned> {
    let extension_line_1_origin = read_bd3(c)?;
    let extension_line_2_origin = read_bd3(c)?;
    let dim_line_definition_point = read_bd3(c)?;
    let oblique_angle_radians = c.read_bd()?;
    Ok(DimensionAligned {
        common: common.clone(),
        suffix: DimensionAlignedSuffix {
            extension_line_1_origin,
            extension_line_2_origin,
            dim_line_definition_point,
            oblique_angle_radians,
        },
    })
}
