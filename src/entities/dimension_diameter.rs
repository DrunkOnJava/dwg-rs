//! DIMENSION (diameter) — L4-19 variant, ODA §19.4.22.
//!
//! Diameter dimensions share the bit layout of radius dimensions
//! (see [`crate::entities::dimension_radial`]) — only the object-type
//! code (0x1A vs 0x19) differs. This module re-exports the diameter
//! type and its dedicated decoder from `dimension_radial` so the
//! family can be navigated by spec-name without duplicating bit-level
//! decode logic.
//!
//! # Stream shape (suffix only)
//!
//! ```text
//! BD3  center_point      -- "def point 10"
//! BD3  first_arc_point   -- "def point 15"
//! BD   leader_length
//! ```

pub use crate::entities::dimension_radial::{
    DimensionDiameter, DimensionRadialSuffix, decode_diameter as decode,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitcursor::BitCursor;
    use crate::bitwriter::BitWriter;
    use crate::entities::dimension::read_common;
    use crate::version::Version;

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
    fn decode_via_reexport() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(12.5);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(6.25);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d: DimensionDiameter = decode(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.first_arc_point.x, 12.5);
        assert_eq!(d.suffix.leader_length, 6.25);
    }

    #[test]
    fn suffix_shape_matches_radial() {
        // Sanity-check the re-export — the public type is the one
        // declared in dimension_radial, not a duplicate.
        let _s: DimensionRadialSuffix = DimensionRadialSuffix {
            center_point: crate::entities::Point3D::default(),
            first_arc_point: crate::entities::Point3D::default(),
            leader_length: 0.0,
        };
    }
}
