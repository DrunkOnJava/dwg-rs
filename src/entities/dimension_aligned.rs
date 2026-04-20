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
    fn roundtrip_aligned_r2000_no_oblique() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(5.0);
        w.write_bd(5.0);
        w.write_bd(0.0);
        w.write_bd(2.5);
        w.write_bd(2.5);
        w.write_bd(0.0);
        w.write_bd(0.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.extension_line_2_origin.x, 5.0);
        assert_eq!(d.suffix.extension_line_2_origin.y, 5.0);
        assert_eq!(d.suffix.oblique_angle_radians, 0.0);
    }

    #[test]
    fn roundtrip_aligned_with_oblique() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(7.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(4.0);
        w.write_bd(3.0);
        w.write_bd(0.0);
        w.write_bd(std::f64::consts::FRAC_PI_3);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert!((d.suffix.oblique_angle_radians - std::f64::consts::FRAC_PI_3).abs() < 1e-12);
        assert_eq!(d.suffix.dim_line_definition_point.y, 3.0);
    }

    #[test]
    fn roundtrip_aligned_r2007_plus() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2007);
        w.write_bd(2.0);
        w.write_bd(2.0);
        w.write_bd(0.0);
        w.write_bd(12.0);
        w.write_bd(2.0);
        w.write_bd(0.0);
        w.write_bd(7.0);
        w.write_bd(5.0);
        w.write_bd(0.0);
        w.write_bd(0.25);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2007).unwrap();
        let d = decode(&mut c, &common, Version::R2007).unwrap();
        assert_eq!(d.suffix.extension_line_1_origin.x, 2.0);
        assert_eq!(d.suffix.oblique_angle_radians, 0.25);
    }
}
