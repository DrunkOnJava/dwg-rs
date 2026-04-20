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

    fn write_radial_suffix(w: &mut BitWriter, cp: (f64, f64, f64), fap: (f64, f64, f64), ll: f64) {
        w.write_bd(cp.0);
        w.write_bd(cp.1);
        w.write_bd(cp.2);
        w.write_bd(fap.0);
        w.write_bd(fap.1);
        w.write_bd(fap.2);
        w.write_bd(ll);
    }

    #[test]
    fn roundtrip_radius_r2000() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        write_radial_suffix(&mut w, (0.0, 0.0, 0.0), (5.0, 0.0, 0.0), 2.5);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode_radius(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.center_point.x, 0.0);
        assert_eq!(d.suffix.first_arc_point.x, 5.0);
        assert_eq!(d.suffix.leader_length, 2.5);
    }

    #[test]
    fn roundtrip_diameter_r2000() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        write_radial_suffix(&mut w, (10.0, 10.0, 0.0), (20.0, 10.0, 0.0), 4.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode_diameter(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.center_point.x, 10.0);
        assert_eq!(d.suffix.first_arc_point.x, 20.0);
        assert_eq!(d.suffix.leader_length, 4.0);
    }

    #[test]
    fn radius_and_diameter_share_layout() {
        // Two identical streams should decode to identical suffixes.
        let mut w1 = BitWriter::new();
        write_common(&mut w1, Version::R2000);
        write_radial_suffix(&mut w1, (1.0, 2.0, 0.0), (4.0, 2.0, 0.0), 0.5);
        let bytes1 = w1.into_bytes();

        let mut w2 = BitWriter::new();
        write_common(&mut w2, Version::R2000);
        write_radial_suffix(&mut w2, (1.0, 2.0, 0.0), (4.0, 2.0, 0.0), 0.5);
        let bytes2 = w2.into_bytes();

        let mut c1 = BitCursor::new(&bytes1);
        let common1 = read_common(&mut c1, Version::R2000).unwrap();
        let r = decode_radius(&mut c1, &common1, Version::R2000).unwrap();

        let mut c2 = BitCursor::new(&bytes2);
        let common2 = read_common(&mut c2, Version::R2000).unwrap();
        let d = decode_diameter(&mut c2, &common2, Version::R2000).unwrap();

        assert_eq!(r.suffix, d.suffix);
    }
}
