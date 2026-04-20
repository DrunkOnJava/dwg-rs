//! LIGHT entity (§19.4.94) — R2007+.
//!
//! LIGHT is a unified representation of four sub-types dispatched by
//! `light_type`:
//!
//! | `light_type` | Variant    | Position | Target |
//! |--------------|------------|----------|--------|
//! | 1            | distant    | n/a      | n/a    |
//! | 2            | point      | yes      | no     |
//! | 3            | spot       | yes      | yes    |
//! | 4            | web        | yes      | yes    |
//!
//! Only the position/target fields a variant actually uses are
//! encoded on the wire; the decoder omits the unused points from the
//! struct for clarity.
//!
//! # Stream shape
//!
//! ```text
//! BL   version
//! TV   name
//! BL   light_type          -- 1..=4 per the table above
//! B    is_on
//! BS   shadow_type
//! B    has_shadow_softness
//! (if light_type >= 2)
//!   BD3  position
//! (if light_type >= 3)
//!   BD3  target
//! BD   intensity
//! BS   color_index          -- simplified CMC (AutoCAD color index)
//! B    plot_glyph
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

/// Discriminator for LIGHT variant per §19.4.94.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightType {
    Distant,
    Point,
    Spot,
    Web,
    /// Reserved / future — surface the raw code rather than erroring
    /// so callers can see what they're dealing with.
    Unknown(u32),
}

impl LightType {
    pub fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Distant,
            2 => Self::Point,
            3 => Self::Spot,
            4 => Self::Web,
            other => Self::Unknown(other),
        }
    }

    pub fn has_position(self) -> bool {
        matches!(self, Self::Point | Self::Spot | Self::Web)
    }

    pub fn has_target(self) -> bool {
        matches!(self, Self::Spot | Self::Web)
    }
}

/// Decoded LIGHT entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    pub version: u32,
    pub name: String,
    pub light_type: LightType,
    pub is_on: bool,
    pub shadow_type: i16,
    pub has_shadow_softness: bool,
    /// Present for point / spot / web lights.
    pub position: Option<Point3D>,
    /// Present for spot / web lights.
    pub target: Option<Point3D>,
    pub intensity: f64,
    pub color_index: i16,
    pub plot_glyph: bool,
}

/// Decode a LIGHT payload.
///
/// The cursor must already be positioned past the common entity
/// preamble. Returns [`Error::Unsupported`] for pre-R2007 versions.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Light> {
    if !version.is_r2007_plus() {
        return Err(Error::Unsupported {
            feature: "LIGHT requires R2007+".into(),
        });
    }
    let version_field = c.read_bl_u()?;
    let name = read_tv(c, version)?;
    let light_type_code = c.read_bl_u()?;
    let light_type = LightType::from_code(light_type_code);
    let is_on = c.read_b()?;
    let shadow_type = c.read_bs()?;
    let has_shadow_softness = c.read_b()?;
    let position = if light_type.has_position() {
        Some(read_bd3(c)?)
    } else {
        None
    };
    let target = if light_type.has_target() {
        Some(read_bd3(c)?)
    } else {
        None
    };
    let intensity = c.read_bd()?;
    let color_index = c.read_bs()?;
    let plot_glyph = c.read_b()?;
    Ok(Light {
        version: version_field,
        name,
        light_type,
        is_on,
        shadow_type,
        has_shadow_softness,
        position,
        target,
        intensity,
        color_index,
        plot_glyph,
    })
}

/// LIGHT is R2007+ only, so TV is always UTF-16LE.
fn read_tv(c: &mut BitCursor<'_>, _version: Version) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    let mut units = Vec::with_capacity(len);
    for _ in 0..len {
        let lo = c.read_rc()? as u16;
        let hi = c.read_rc()? as u16;
        units.push((hi << 8) | lo);
    }
    if units.last() == Some(&0) {
        units.pop();
    }
    String::from_utf16(&units).map_err(|_| Error::SectionMap("LIGHT name is not valid UTF-16".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Helper — write a UTF-16LE TV string with trailing NUL.
    fn write_utf16_tv(w: &mut BitWriter, s: &str) {
        let units: Vec<u16> = s.encode_utf16().collect();
        let len = units.len() + 1; // +1 trailing NUL
        w.write_bs_u(len as u16);
        for u in &units {
            w.write_rc((u & 0xFF) as u8);
            w.write_rc((u >> 8) as u8);
        }
        w.write_rc(0); // NUL lo
        w.write_rc(0); // NUL hi
    }

    #[test]
    fn roundtrip_spot_light() {
        let mut w = BitWriter::new();
        w.write_bl_u(1); // version
        write_utf16_tv(&mut w, "Spot1");
        w.write_bl_u(3); // light_type = Spot
        w.write_b(true); // is_on
        w.write_bs(1); // shadow_type
        w.write_b(false); // has_shadow_softness
        // position (10, 20, 30)
        w.write_bd(10.0);
        w.write_bd(20.0);
        w.write_bd(30.0);
        // target (0, 0, 0)
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // intensity
        w.write_bs(3); // color index
        w.write_b(true); // plot_glyph

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2018).unwrap();
        assert_eq!(l.version, 1);
        assert_eq!(l.name, "Spot1");
        assert_eq!(l.light_type, LightType::Spot);
        assert!(l.is_on);
        assert_eq!(l.shadow_type, 1);
        assert!(!l.has_shadow_softness);
        assert_eq!(
            l.position,
            Some(Point3D {
                x: 10.0,
                y: 20.0,
                z: 30.0
            })
        );
        assert_eq!(
            l.target,
            Some(Point3D {
                x: 0.0,
                y: 0.0,
                z: 0.0
            })
        );
        assert_eq!(l.intensity, 1.0);
        assert_eq!(l.color_index, 3);
        assert!(l.plot_glyph);
    }

    #[test]
    fn roundtrip_distant_light_omits_position_and_target() {
        let mut w = BitWriter::new();
        w.write_bl_u(1);
        write_utf16_tv(&mut w, "Sunlike");
        w.write_bl_u(1); // Distant
        w.write_b(true);
        w.write_bs(0);
        w.write_b(false);
        w.write_bd(0.75); // intensity
        w.write_bs(252); // color
        w.write_b(false);

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2010).unwrap();
        assert_eq!(l.light_type, LightType::Distant);
        assert!(l.position.is_none());
        assert!(l.target.is_none());
        assert_eq!(l.intensity, 0.75);
    }

    #[test]
    fn rejects_pre_r2007() {
        let bytes = [0u8; 4];
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2004).unwrap_err();
        assert!(
            matches!(&err, Error::Unsupported { feature } if feature.contains("LIGHT")),
            "err={err:?}"
        );
    }
}
