//! DIMENSION (linear) — L4-17, ODA §19.4.18.
//!
//! Linear dimension subtype. Caller parses the common DIMENSION
//! preamble (see [`crate::entities::dimension::DimensionCommon`]) and
//! then invokes [`decode`] to read the subtype-specific suffix.
//!
//! # Stream shape (suffix only)
//!
//! ```text
//! BD3  extension_line_1_origin   -- "def point 13"
//! BD3  extension_line_2_origin   -- "def point 14"
//! BD3  dim_line_definition_point -- "def point 10"
//! BD   rotation_radians          -- rotation of the dimension line
//! BD   oblique_angle_radians     -- obliquing angle on the extension lines
//! ```
//!
//! Field names follow ODA spec v5.4.1 §19.4.18 semantic terminology
//! ("extension line 1/2 origin", "dim line definition point") rather
//! than the bare numeric group codes used by DXF. The
//! [`DimensionLinearSuffix`] form stays addressable by meaning.

use crate::bitcursor::BitCursor;
use crate::entities::dimension::DimensionCommon;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::version::Version;

/// The linear DIMENSION subtype suffix — the data that follows the
/// shared preamble.
///
/// See [§19.4.18][oda]. Combine with a `DimensionCommon` to form the
/// full entity.
///
/// [oda]: https://www.opendesign.com/files/guestdownloads/OpenDesign_Specification_for_.dwg_files.pdf
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionLinearSuffix {
    pub extension_line_1_origin: Point3D,
    pub extension_line_2_origin: Point3D,
    pub dim_line_definition_point: Point3D,
    pub rotation_radians: f64,
    pub oblique_angle_radians: f64,
}

/// Full linear DIMENSION — shared preamble plus linear-specific
/// suffix.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionLinear {
    pub common: DimensionCommon,
    pub suffix: DimensionLinearSuffix,
}

/// Decode the linear-dimension suffix from a bit stream.
///
/// The caller is expected to have already consumed the common
/// DIMENSION preamble (see
/// [`crate::entities::dimension::read_common`]) and to supply the
/// parsed [`DimensionCommon`] here. The `version` argument is
/// retained for symmetry with the rest of the entity-decoder API and
/// to support format variations in future DWG releases; the L4-17
/// suffix itself is version-invariant.
pub fn decode(
    c: &mut BitCursor<'_>,
    common: &DimensionCommon,
    _version: Version,
) -> Result<DimensionLinear> {
    let extension_line_1_origin = read_bd3(c)?;
    let extension_line_2_origin = read_bd3(c)?;
    let dim_line_definition_point = read_bd3(c)?;
    let rotation_radians = c.read_bd()?;
    let oblique_angle_radians = c.read_bd()?;
    Ok(DimensionLinear {
        common: common.clone(),
        suffix: DimensionLinearSuffix {
            extension_line_1_origin,
            extension_line_2_origin,
            dim_line_definition_point,
            rotation_radians,
            oblique_angle_radians,
        },
    })
}
