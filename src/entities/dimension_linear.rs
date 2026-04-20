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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::entities::dimension::read_common;

    /// Write a minimal common dimension preamble matching the test
    /// helper in `dimension.rs`.
    fn write_common(w: &mut BitWriter, version: Version) {
        if version.is_r2010_plus() {
            w.write_rc(0);
        }
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // extrusion
        w.write_rd(5.0);
        w.write_rd(5.0); // text midpoint
        w.write_bd(0.0); // elevation
        w.write_rc(0x00); // flags
        w.write_bs_u(0); // empty user text
        w.write_bd(0.0); // text rotation
        w.write_bd(0.0); // horiz dir
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(1.0); // ins scale
        w.write_bd(0.0); // ins rotation
        w.write_bs(0); // attachment
        w.write_bs(0); // line-spacing style
        w.write_bd(1.0); // line-spacing factor
        w.write_bd(10.0); // actual measurement
        if version.is_r2007_plus() {
            w.write_b(false);
            w.write_b(false);
            w.write_b(false);
        }
        w.write_rd(0.0);
        w.write_rd(0.0); // def point 12
    }

    #[test]
    fn roundtrip_linear_r2000_basic() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // ext line 1
        w.write_bd(10.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // ext line 2
        w.write_bd(5.0);
        w.write_bd(2.0);
        w.write_bd(0.0); // dim line def point
        w.write_bd(0.0); // rotation
        w.write_bd(0.0); // oblique
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert_eq!(d.suffix.extension_line_2_origin.x, 10.0);
        assert_eq!(d.suffix.dim_line_definition_point.y, 2.0);
        assert_eq!(d.suffix.rotation_radians, 0.0);
    }

    #[test]
    fn roundtrip_linear_r2000_rotated() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(1.0);
        w.write_bd(2.0);
        w.write_bd(3.0);
        w.write_bd(11.0);
        w.write_bd(2.0);
        w.write_bd(3.0);
        w.write_bd(6.0);
        w.write_bd(4.0);
        w.write_bd(3.0);
        w.write_bd(std::f64::consts::FRAC_PI_4);
        w.write_bd(std::f64::consts::FRAC_PI_6);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2000).unwrap();
        let d = decode(&mut c, &common, Version::R2000).unwrap();
        assert!((d.suffix.rotation_radians - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((d.suffix.oblique_angle_radians - std::f64::consts::FRAC_PI_6).abs() < 1e-12);
        assert_eq!(d.suffix.extension_line_1_origin.x, 1.0);
    }

    #[test]
    fn roundtrip_linear_r2010_version_gate() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2010);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.5);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let common = read_common(&mut c, Version::R2010).unwrap();
        let d = decode(&mut c, &common, Version::R2010).unwrap();
        assert_eq!(d.suffix.extension_line_2_origin.x, 1.0);
    }
}
