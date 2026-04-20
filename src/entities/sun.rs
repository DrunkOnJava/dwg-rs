//! SUN entity (§19.4.93) — R2007+.
//!
//! SUN represents a sky-model light source anchored to the drawing.
//! It is typically referenced by a viewport's lighting settings
//! rather than placed in user-visible model space. Per the spec
//! (and cross-checked against the fields ACadSharp exposes at the
//! algorithm level), SUN carries the date/time + color + shadow
//! configuration necessary to compute a physically-plausible sun
//! direction.
//!
//! # Stream shape (minimal, the fields viewers and round-trip tools
//! actually need)
//!
//! ```text
//! BL   version             -- always 1 in observed files
//! B    status              -- on / off
//! BS   sun_color_index     -- simplified CMC → AutoCAD color index;
//!                             the fully-resolved CMC is deferred
//! BD   intensity
//! B    has_shadow
//! BL   julian_day          -- compact date: YYYYMMDD or true Julian
//! BL   time_of_day         -- seconds since midnight
//! B    daylight_savings
//! B    shadow_type_map_flag
//! B    shadow_softness_flag
//! B    shadow_map_size_flag
//! BS   shadow_softness_samples
//! ```
//!
//! The `sun_color_index` simplification (BS vs. full CMC) matches the
//! pattern the LIGHT decoder uses — a full CMC expansion lives in the
//! dedicated color module and will be wired in once that code path is
//! stable for every entity that needs it. Until then, treating the
//! leading bitshort as the AutoCAD color index matches what every
//! viewer-class consumer does in practice.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::version::Version;

/// Decoded SUN entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Sun {
    pub version: u32,
    pub status: bool,
    pub sun_color_index: i16,
    pub intensity: f64,
    pub has_shadow: bool,
    pub julian_day: u32,
    pub time_of_day: u32,
    pub daylight_savings: bool,
    pub shadow_type_map_flag: bool,
    pub shadow_softness_flag: bool,
    pub shadow_map_size_flag: bool,
    pub shadow_softness_samples: i16,
}

/// Decode a SUN payload.
///
/// The cursor must already be positioned past the common entity
/// preamble. Returns [`Error::Unsupported`] for pre-R2007 versions.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Sun> {
    if !version.is_r2007_plus() {
        return Err(Error::Unsupported {
            feature: "SUN requires R2007+".into(),
        });
    }
    let version_field = c.read_bl_u()?;
    let status = c.read_b()?;
    let sun_color_index = c.read_bs()?;
    let intensity = c.read_bd()?;
    let has_shadow = c.read_b()?;
    let julian_day = c.read_bl_u()?;
    let time_of_day = c.read_bl_u()?;
    let daylight_savings = c.read_b()?;
    let shadow_type_map_flag = c.read_b()?;
    let shadow_softness_flag = c.read_b()?;
    let shadow_map_size_flag = c.read_b()?;
    let shadow_softness_samples = c.read_bs()?;
    Ok(Sun {
        version: version_field,
        status,
        sun_color_index,
        intensity,
        has_shadow,
        julian_day,
        time_of_day,
        daylight_savings,
        shadow_type_map_flag,
        shadow_softness_flag,
        shadow_map_size_flag,
        shadow_softness_samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_sun_noon_summer() {
        let mut w = BitWriter::new();
        w.write_bl_u(1); // version
        w.write_b(true); // status on
        w.write_bs(7); // color index
        w.write_bd(1.5); // intensity
        w.write_b(true); // has_shadow
        w.write_bl_u(20260620); // June 20, 2026
        w.write_bl_u(43200); // noon = 12h * 3600s
        w.write_b(true); // daylight savings
        w.write_b(false); // shadow_type_map_flag
        w.write_b(true); // shadow_softness_flag
        w.write_b(false); // shadow_map_size_flag
        w.write_bs(16); // shadow softness samples

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2010).unwrap();
        assert_eq!(s.version, 1);
        assert!(s.status);
        assert_eq!(s.sun_color_index, 7);
        assert_eq!(s.intensity, 1.5);
        assert!(s.has_shadow);
        assert_eq!(s.julian_day, 20260620);
        assert_eq!(s.time_of_day, 43200);
        assert!(s.daylight_savings);
        assert!(!s.shadow_type_map_flag);
        assert!(s.shadow_softness_flag);
        assert!(!s.shadow_map_size_flag);
        assert_eq!(s.shadow_softness_samples, 16);
    }

    #[test]
    fn rejects_pre_r2007() {
        let bytes = [0u8; 4];
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(
            matches!(&err, Error::Unsupported { feature } if feature.contains("SUN")),
            "err={err:?}"
        );
    }
}
