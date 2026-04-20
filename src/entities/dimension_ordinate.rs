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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::entities::dimension::read_common;

    fn write_common(w: &mut BitWriter, version: Version) {
        if version.is_r2010_plus() {
            w.write_rc(0);
        }
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_rd(5.0);
        w.write_rd(5.0);
        w.write_bd(0.0);
        w.write_rc(0x00);
        w.write_bs_u(0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bs(0);
        w.write_bs(0);
        w.write_bd(1.0);
        w.write_bd(10.0);
        if version.is_r2007_plus() {
            w.write_b(false);
            w.write_b(false);
            w.write_b(false);
        }
        w.write_rd(0.0);
        w.write_rd(0.0);
    }

    #[test]
    fn roundtrip_ordinate_y_type() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(3.0);
        w.write_bd(4.0);
        w.write_bd(0.0); // feature
        w.write_bd(3.0);
        w.write_bd(10.0);
        w.write_bd(0.0); // leader
        w.write_rc(0x00); // Y-type flag
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.feature_location.x, 3.0);
        assert_eq!(d.suffix.feature_location.y, 4.0);
        assert_eq!(d.suffix.leader_endpoint.y, 10.0);
        assert_eq!(d.suffix.flags, 0x00);
        assert_eq!(d.suffix.axis(), OrdinateAxis::Y);
    }

    #[test]
    fn roundtrip_ordinate_x_type() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(5.0);
        w.write_bd(2.0);
        w.write_bd(0.0);
        w.write_bd(12.0);
        w.write_bd(2.0);
        w.write_bd(0.0);
        w.write_rc(0x01); // X-type flag
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.feature_location.x, 5.0);
        assert_eq!(d.suffix.leader_endpoint.x, 12.0);
        assert_eq!(d.suffix.flags, 0x01);
        assert_eq!(d.suffix.axis(), OrdinateAxis::X);
    }

    #[test]
    fn ordinate_flag_reserved_bits_preserved() {
        // Ensure higher-order flag bits survive round-trip — they
        // are ODA-reserved, so we preserve them even though we don't
        // interpret them.
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_rc(0xF1); // x-type, plus high reserved bits
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.flags, 0xF1);
        assert_eq!(d.suffix.axis(), OrdinateAxis::X);
    }
}
