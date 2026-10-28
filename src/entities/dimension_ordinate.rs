//! DIMENSION (ordinate) — L4-21, ODA §19.4.23.
//!
//! Ordinate dimensions measure the X or Y offset of a feature from
//! a known origin. One flag byte distinguishes X-type (bit 0 set)
//! from Y-type (bit 0 clear).
//!
//! # Stream shape (suffix only)
//!
//! ```text
//! BD3  feature_location   -- the point being dimensioned ("def point 13")
//! BD3  leader_endpoint    -- where the leader terminates  ("def point 14")
//! RC   flags              -- bit 0 = x-type, 0 = y-type
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::dimension::DimensionCommon;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::version::Version;

/// Ordinate-dimension orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrdinateAxis {
    /// Y-type: the dimension value is the feature's Y-offset from
    /// the origin. Flag bit 0 = 0.
    Y,
    /// X-type: the dimension value is the feature's X-offset from
    /// the origin. Flag bit 0 = 1.
    X,
}

/// Ordinate DIMENSION suffix (§19.4.23).
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionOrdinateSuffix {
    pub feature_location: Point3D,
    pub leader_endpoint: Point3D,
    /// Raw flag byte as stored in the DWG — kept for round-trip
    /// fidelity. Semantic view is in [`Self::axis`].
    pub flags: u8,
}

impl DimensionOrdinateSuffix {
    /// Interpret the flag byte as an axis indicator.
    ///
    /// Per ODA v5.4.1 §19.4.23, bit 0 set ⇒ X-type, clear ⇒ Y-type.
    /// Other bits are reserved and preserved in [`Self::flags`].
    pub fn axis(&self) -> OrdinateAxis {
        if self.flags & 0x01 != 0 {
            OrdinateAxis::X
        } else {
            OrdinateAxis::Y
        }
    }
}

/// Full ordinate DIMENSION.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionOrdinate {
    pub common: DimensionCommon,
    pub suffix: DimensionOrdinateSuffix,
}

/// Decode the ordinate-dimension suffix.
pub fn decode(
    c: &mut BitCursor<'_>,
    common: &DimensionCommon,
    _version: Version,
) -> Result<DimensionOrdinate> {
    let feature_location = read_bd3(c)?;
    let leader_endpoint = read_bd3(c)?;
    let flags = c.read_rc()?;
    Ok(DimensionOrdinate {
        common: common.clone(),
        suffix: DimensionOrdinateSuffix {
            feature_location,
            leader_endpoint,
            flags,
        },
    })
}
