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
    fn roundtrip_angular_3p_right_angle() {
        // Right angle at the origin between +x and +y axes.
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // center
        w.write_bd(10.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // first extension
        w.write_bd(0.0);
        w.write_bd(10.0);
        w.write_bd(0.0); // second extension
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.center.x, 0.0);
        assert_eq!(d.suffix.first_extension_point.x, 10.0);
        assert_eq!(d.suffix.second_extension_point.y, 10.0);
    }

    #[test]
    fn roundtrip_angular_3p_offset_center() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(5.0);
        w.write_bd(5.0);
        w.write_bd(0.0);
        w.write_bd(15.0);
        w.write_bd(5.0);
        w.write_bd(0.0);
        w.write_bd(5.0);
        w.write_bd(15.0);
        w.write_bd(0.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.center.x, 5.0);
        assert_eq!(d.suffix.center.y, 5.0);
        assert_eq!(d.suffix.first_extension_point.x, 15.0);
    }

    #[test]
    fn roundtrip_angular_3p_r2010() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2010);
        w.write_bd(1.0);
        w.write_bd(2.0);
        w.write_bd(3.0);
        w.write_bd(4.0);
        w.write_bd(5.0);
        w.write_bd(6.0);
        w.write_bd(7.0);
        w.write_bd(8.0);
        w.write_bd(9.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2010).unwrap();
        let d = decode(&mut c, &common, Version::R2010).unwrap();
        assert_eq!(d.suffix.center.z, 3.0);
        assert_eq!(d.suffix.first_extension_point.y, 5.0);
        assert_eq!(d.suffix.second_extension_point.x, 7.0);
    }
}
