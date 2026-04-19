//! DIMENSION entity family (§19.4.10 through §19.4.15).
//!
//! DIMENSION has six subtypes, each with its own DWG object-type
//! code:
//!
//! | Subtype         | Code | Module symbol            |
//! |-----------------|------|--------------------------|
//! | Ordinate        | 21   | [`OrdinateDimension`]    |
//! | Linear          | 22   | [`LinearDimension`]      |
//! | Aligned         | 23   | [`AlignedDimension`]     |
//! | Angular-3-point | 24   | [`Angular3PtDimension`]  |
//! | Angular-2-line  | 25   | [`Angular2LineDimension`]|
//! | Radius          | 26   | [`RadiusDimension`]      |
//! | Diameter        | 27   | [`DiameterDimension`]    |
//!
//! All dimension subtypes share a large common preamble:
//!
//! ```text
//! RC    version_flag       -- R2010+, 0 or 1
//! BD3   extrusion
//! RD2   text_midpoint      -- screen coord of the dimension's text
//! BD    elevation
//! RC    flags              -- TYPE nibble + bits for suppress_1/2,
//!                             unknown user text, extension-line 1/2
//!                             suppress
//! TV    user_text
//! BD    text_rotation
//! BD    horiz_dir          -- rotation of dimension line
//! BD3   ins_scale          -- X/Y/Z scale
//! BD    ins_rotation
//! BS    attachment
//! BS    lineSpacing_style
//! BD    lineSpacing_factor
//! BD    actual_measurement -- computed dimension value
//! (R2007+)
//!   B   unknown_bit
//!   B   flip_arrow_1
//!   B   flip_arrow_2
//! RD2   12_pt              -- historical "12-pt" — the definition point
//! ```
//!
//! Each subtype then writes its own suffix. This module reads the
//! shared preamble then dispatches on [`DimensionKind`] to finish.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Point3D, Vec3D, read_bd3};
use crate::error::Result;
use crate::tables::read_tv;
use crate::version::Version;

/// Shared dimension preamble (§19.4.10.1).
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionCommon {
    pub version_flag: u8,
    pub extrusion: Vec3D,
    pub text_midpoint: Point2D,
    pub elevation: f64,
    pub flags: u8,
    pub user_text: String,
    pub text_rotation: f64,
    pub horiz_dir: f64,
    pub ins_scale: Point3D,
    pub ins_rotation: f64,
    pub attachment: i16,
    pub line_spacing_style: i16,
    pub line_spacing_factor: f64,
    pub actual_measurement: f64,
    pub def_point_12: Point2D,
    pub flip_arrow_1: bool,
    pub flip_arrow_2: bool,
}

pub fn read_common(c: &mut BitCursor<'_>, version: Version) -> Result<DimensionCommon> {
    let version_flag = if version.is_r2010_plus() {
        c.read_rc()?
    } else {
        0
    };
    let extrusion = read_bd3(c)?;
    let text_midpoint = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let elevation = c.read_bd()?;
    let flags = c.read_rc()?;
    let user_text = read_tv(c, version)?;
    let text_rotation = c.read_bd()?;
    let horiz_dir = c.read_bd()?;
    let ins_scale = read_bd3(c)?;
    let ins_rotation = c.read_bd()?;
    let attachment = c.read_bs()?;
    let line_spacing_style = c.read_bs()?;
    let line_spacing_factor = c.read_bd()?;
    let actual_measurement = c.read_bd()?;
    let (flip_arrow_1, flip_arrow_2) = if version.is_r2007_plus() {
        let _unknown = c.read_b()?;
        (c.read_b()?, c.read_b()?)
    } else {
        (false, false)
    };
    let def_point_12 = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    Ok(DimensionCommon {
        version_flag,
        extrusion,
        text_midpoint,
        elevation,
        flags,
        user_text,
        text_rotation,
        horiz_dir,
        ins_scale,
        ins_rotation,
        attachment,
        line_spacing_style,
        line_spacing_factor,
        actual_measurement,
        def_point_12,
        flip_arrow_1,
        flip_arrow_2,
    })
}

/// One of the six DIMENSION subtypes.
#[derive(Debug, Clone, PartialEq)]
pub enum Dimension {
    Ordinate(OrdinateDimension),
    Linear(LinearDimension),
    Aligned(AlignedDimension),
    Angular3Pt(Angular3PtDimension),
    Angular2Line(Angular2LineDimension),
    Radius(RadiusDimension),
    Diameter(DiameterDimension),
}

/// Enum of dimension subtypes for dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionKind {
    Ordinate,
    Linear,
    Aligned,
    Angular3Pt,
    Angular2Line,
    Radius,
    Diameter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrdinateDimension {
    pub common: DimensionCommon,
    pub def_point_10: Point3D,
    pub feature_location_13: Point3D,
    pub leader_endpoint_14: Point3D,
    pub flag_2: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinearDimension {
    pub common: DimensionCommon,
    pub def_point_13: Point3D,
    pub def_point_14: Point3D,
    pub def_point_10: Point3D,
    pub extension_line_rotation: f64,
    pub dim_rotation: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlignedDimension {
    pub common: DimensionCommon,
    pub def_point_13: Point3D,
    pub def_point_14: Point3D,
    pub def_point_10: Point3D,
    pub extension_line_rotation: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Angular3PtDimension {
    pub common: DimensionCommon,
    pub def_point_10: Point3D,
    pub def_point_13: Point3D,
    pub def_point_14: Point3D,
    pub def_point_15: Point3D,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Angular2LineDimension {
    pub common: DimensionCommon,
    pub def_point_13: Point3D,
    pub def_point_14: Point3D,
    pub def_point_15: Point3D,
    pub def_point_10: Point3D,
    pub def_point_16: Point2D,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadiusDimension {
    pub common: DimensionCommon,
    pub def_point_10: Point3D,
    pub def_point_15: Point3D,
    pub leader_length: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiameterDimension {
    pub common: DimensionCommon,
    pub def_point_15: Point3D,
    pub def_point_10: Point3D,
    pub leader_length: f64,
}

/// Decode a complete dimension of `kind` (caller already consumed
/// the common entity preamble).
pub fn decode(
    c: &mut BitCursor<'_>,
    version: Version,
    kind: DimensionKind,
) -> Result<Dimension> {
    let common = read_common(c, version)?;
    Ok(match kind {
        DimensionKind::Ordinate => {
            let def_point_10 = read_bd3(c)?;
            let feature_location_13 = read_bd3(c)?;
            let leader_endpoint_14 = read_bd3(c)?;
            let flag_2 = c.read_rc()?;
            Dimension::Ordinate(OrdinateDimension {
                common,
                def_point_10,
                feature_location_13,
                leader_endpoint_14,
                flag_2,
            })
        }
        DimensionKind::Linear => {
            let def_point_13 = read_bd3(c)?;
            let def_point_14 = read_bd3(c)?;
            let def_point_10 = read_bd3(c)?;
            let extension_line_rotation = c.read_bd()?;
            let dim_rotation = c.read_bd()?;
            Dimension::Linear(LinearDimension {
                common,
                def_point_13,
                def_point_14,
                def_point_10,
                extension_line_rotation,
                dim_rotation,
            })
        }
        DimensionKind::Aligned => {
            let def_point_13 = read_bd3(c)?;
            let def_point_14 = read_bd3(c)?;
            let def_point_10 = read_bd3(c)?;
            let extension_line_rotation = c.read_bd()?;
            Dimension::Aligned(AlignedDimension {
                common,
                def_point_13,
                def_point_14,
                def_point_10,
                extension_line_rotation,
            })
        }
        DimensionKind::Angular3Pt => {
            let def_point_10 = read_bd3(c)?;
            let def_point_13 = read_bd3(c)?;
            let def_point_14 = read_bd3(c)?;
            let def_point_15 = read_bd3(c)?;
            Dimension::Angular3Pt(Angular3PtDimension {
                common,
                def_point_10,
                def_point_13,
                def_point_14,
                def_point_15,
            })
        }
        DimensionKind::Angular2Line => {
            let def_point_13 = read_bd3(c)?;
            let def_point_14 = read_bd3(c)?;
            let def_point_15 = read_bd3(c)?;
            let def_point_10 = read_bd3(c)?;
            let def_point_16 = Point2D {
                x: c.read_rd()?,
                y: c.read_rd()?,
            };
            Dimension::Angular2Line(Angular2LineDimension {
                common,
                def_point_13,
                def_point_14,
                def_point_15,
                def_point_10,
                def_point_16,
            })
        }
        DimensionKind::Radius => {
            let def_point_10 = read_bd3(c)?;
            let def_point_15 = read_bd3(c)?;
            let leader_length = c.read_bd()?;
            Dimension::Radius(RadiusDimension {
                common,
                def_point_10,
                def_point_15,
                leader_length,
            })
        }
        DimensionKind::Diameter => {
            let def_point_15 = read_bd3(c)?;
            let def_point_10 = read_bd3(c)?;
            let leader_length = c.read_bd()?;
            Dimension::Diameter(DiameterDimension {
                common,
                def_point_15,
                def_point_10,
                leader_length,
            })
        }
    })
}

impl DimensionKind {
    /// Map a DWG object-type code to the dimension subtype. Returns
    /// None if the code is not a dimension type.
    pub fn from_object_type_code(code: u16) -> Option<Self> {
        match code {
            21 => Some(Self::Ordinate),
            22 => Some(Self::Linear),
            23 => Some(Self::Aligned),
            24 => Some(Self::Angular3Pt),
            25 => Some(Self::Angular2Line),
            26 => Some(Self::Radius),
            27 => Some(Self::Diameter),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Helper: write a minimal common dimension preamble.
    fn write_common(w: &mut BitWriter, version: Version) {
        if version.is_r2010_plus() {
            w.write_rc(0);
        }
        w.write_bd(0.0); w.write_bd(0.0); w.write_bd(1.0); // extrusion
        w.write_rd(5.0); w.write_rd(5.0); // text midpoint
        w.write_bd(0.0); // elevation
        w.write_rc(0x00); // flags
        w.write_bs_u(0); // empty user text
        w.write_bd(0.0); // text rotation
        w.write_bd(0.0); // horiz dir
        w.write_bd(1.0); w.write_bd(1.0); w.write_bd(1.0); // ins scale
        w.write_bd(0.0); // ins rotation
        w.write_bs(0); // attachment
        w.write_bs(0); // line-spacing style
        w.write_bd(1.0); // line-spacing factor
        w.write_bd(10.0); // actual measurement
        if version.is_r2007_plus() {
            w.write_b(false); // unknown
            w.write_b(false); // flip 1
            w.write_b(false); // flip 2
        }
        w.write_rd(0.0); w.write_rd(0.0); // def point 12
    }

    #[test]
    fn roundtrip_linear_dim_r2000() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(0.0); w.write_bd(0.0); w.write_bd(0.0); // pt13
        w.write_bd(10.0); w.write_bd(0.0); w.write_bd(0.0); // pt14
        w.write_bd(5.0); w.write_bd(2.0); w.write_bd(0.0); // pt10
        w.write_bd(0.0); // ext rotation
        w.write_bd(0.0); // dim rotation
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode(&mut c, Version::R2000, DimensionKind::Linear).unwrap();
        match d {
            Dimension::Linear(ld) => {
                assert_eq!(ld.common.actual_measurement, 10.0);
                assert_eq!(ld.def_point_14.x, 10.0);
            }
            _ => panic!("expected Linear"),
        }
    }

    #[test]
    fn roundtrip_radius_dim_r2000() {
        let mut w = BitWriter::new();
        write_common(&mut w, Version::R2000);
        w.write_bd(0.0); w.write_bd(0.0); w.write_bd(0.0); // pt10 (center)
        w.write_bd(5.0); w.write_bd(0.0); w.write_bd(0.0); // pt15 (chord)
        w.write_bd(2.5); // leader length
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode(&mut c, Version::R2000, DimensionKind::Radius).unwrap();
        match d {
            Dimension::Radius(rd) => {
                assert_eq!(rd.leader_length, 2.5);
                assert_eq!(rd.def_point_15.x, 5.0);
            }
            _ => panic!("expected Radius"),
        }
    }

    #[test]
    fn object_type_mapping() {
        assert_eq!(
            DimensionKind::from_object_type_code(22),
            Some(DimensionKind::Linear)
        );
        assert_eq!(
            DimensionKind::from_object_type_code(27),
            Some(DimensionKind::Diameter)
        );
        assert_eq!(DimensionKind::from_object_type_code(0x04), None);
    }
}

