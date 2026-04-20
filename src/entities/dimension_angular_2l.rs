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

    fn write_point(w: &mut BitWriter, p: (f64, f64, f64)) {
        w.write_bd(p.0);
        w.write_bd(p.1);
        w.write_bd(p.2);
    }

    #[test]
    fn roundtrip_angular_2l_perpendicular_lines() {
        // Line 1: (0,0) → (10,0)
        // Line 2: (0,0) → (0,10)  → perpendicular at the origin
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        write_point(&mut w, (0.0, 0.0, 0.0));
        write_point(&mut w, (10.0, 0.0, 0.0));
        write_point(&mut w, (0.0, 0.0, 0.0));
        write_point(&mut w, (0.0, 10.0, 0.0));
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.extension_line_1a.x, 0.0);
        assert_eq!(d.suffix.extension_line_1b.x, 10.0);
        assert_eq!(d.suffix.extension_line_2b.y, 10.0);
    }

    #[test]
    fn roundtrip_angular_2l_arbitrary() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        write_point(&mut w, (1.0, 2.0, 3.0));
        write_point(&mut w, (4.0, 5.0, 6.0));
        write_point(&mut w, (7.0, 8.0, 9.0));
        write_point(&mut w, (10.0, 11.0, 12.0));
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.extension_line_1a.x, 1.0);
        assert_eq!(d.suffix.extension_line_2a.y, 8.0);
        assert_eq!(d.suffix.extension_line_2b.z, 12.0);
    }

    #[test]
    fn roundtrip_angular_2l_r2007_plus() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2007);
        write_point(&mut w, (0.1, 0.2, 0.3));
        write_point(&mut w, (0.4, 0.5, 0.6));
        write_point(&mut w, (0.7, 0.8, 0.9));
        write_point(&mut w, (1.0, 1.1, 1.2));
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2007).unwrap();
        let d = decode(&mut c, &common, Version::R2007).unwrap();
        assert_eq!(d.suffix.extension_line_1b.y, 0.5);
    }
}
