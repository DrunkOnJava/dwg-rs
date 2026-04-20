//! GEODATA entity (§19.4.95) — R2010+.
//!
//! GEODATA attaches a geographic coordinate reference system (CRS) to
//! a model-space block. Combined with the design-to-reference
//! transform, a viewer can map any WCS point to an absolute Earth
//! coordinate (lat/lon or projected) for integration with GIS data.
//!
//! # Stream shape
//!
//! ```text
//! BL   version
//! H    block_handle                 -- owning model-space block
//!                                      (deferred — raw handle only)
//! BD3  origin                        -- in WCS
//! BD3  design_to_ref_x_axis          -- model-space basis vector
//! BD3  design_to_ref_y_axis          -- model-space basis vector
//! BD3  reference_point               -- in CRS
//! BD3  reference_north_dir           -- CRS-space "north" vector
//! BD   design_to_ref_x_scale
//! BD   design_to_ref_y_scale
//! TV   coordinate_system_def_string  -- ESRI WKT or PROJ string
//! TV   geo_rss_tag                   -- optional RSS feed tag
//! TV   observation_from_tag
//! TV   observation_to_tag
//! TV   observation_coverage_tag
//! ```
//!
//! The block handle is captured but not resolved; callers that need
//! the owning block join it against the full handle map.

use crate::bitcursor::{BitCursor, Handle};
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

/// Decoded GEODATA entity.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoData {
    pub version: u32,
    /// Deferred — raw handle reference to the owning block record.
    pub block_handle: Handle,
    pub origin: Point3D,
    pub design_to_ref_x_axis: Vec3D,
    pub design_to_ref_y_axis: Vec3D,
    pub reference_point: Point3D,
    pub reference_north_dir: Vec3D,
    pub design_to_ref_x_scale: f64,
    pub design_to_ref_y_scale: f64,
    pub coordinate_system_def: String,
    pub geo_rss_tag: String,
    pub observation_from_tag: String,
    pub observation_to_tag: String,
    pub observation_coverage_tag: String,
}

/// Decode a GEODATA payload.
///
/// The cursor must already be positioned past the common entity
/// preamble. Returns [`Error::Unsupported`] for pre-R2010 versions.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<GeoData> {
    if !version.is_r2010_plus() {
        return Err(Error::Unsupported {
            feature: "GEODATA requires R2010+".into(),
        });
    }
    let version_field = c.read_bl_u()?;
    let block_handle = c.read_handle()?;
    let origin = read_bd3(c)?;
    let design_to_ref_x_axis = read_bd3(c)?;
    let design_to_ref_y_axis = read_bd3(c)?;
    let reference_point = read_bd3(c)?;
    let reference_north_dir = read_bd3(c)?;
    let design_to_ref_x_scale = c.read_bd()?;
    let design_to_ref_y_scale = c.read_bd()?;
    let coordinate_system_def = read_tv(c, version)?;
    let geo_rss_tag = read_tv(c, version)?;
    let observation_from_tag = read_tv(c, version)?;
    let observation_to_tag = read_tv(c, version)?;
    let observation_coverage_tag = read_tv(c, version)?;
    Ok(GeoData {
        version: version_field,
        block_handle,
        origin,
        design_to_ref_x_axis,
        design_to_ref_y_axis,
        reference_point,
        reference_north_dir,
        design_to_ref_x_scale,
        design_to_ref_y_scale,
        coordinate_system_def,
        geo_rss_tag,
        observation_from_tag,
        observation_to_tag,
        observation_coverage_tag,
    })
}

/// GEODATA is R2010+ only. R2010, R2013, R2018 all use the UTF-16LE
/// bitstream encoding for TV fields per [`Version::uses_utf16_text`].
fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if version.uses_utf16_text() {
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            let lo = c.read_rc()? as u16;
            let hi = c.read_rc()? as u16;
            units.push((hi << 8) | lo);
        }
        if units.last() == Some(&0) {
            units.pop();
        }
        String::from_utf16(&units)
            .map_err(|_| Error::SectionMap("GEODATA string is not valid UTF-16".into()))
    } else {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(c.read_rc()?);
        }
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn write_utf16_tv(w: &mut BitWriter, s: &str) {
        let units: Vec<u16> = s.encode_utf16().collect();
        let len = units.len() + 1;
        w.write_bs_u(len as u16);
        for u in &units {
            w.write_rc((u & 0xFF) as u8);
            w.write_rc((u >> 8) as u8);
        }
        w.write_rc(0);
        w.write_rc(0);
    }

    #[test]
    fn roundtrip_geodata_wgs84() {
        let mut w = BitWriter::new();
        w.write_bl_u(1); // version
        w.write_handle(5, 0x123); // block handle
        // origin (0,0,0)
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // x axis (1,0,0)
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // y axis (0,1,0)
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        // reference_point (lon=-122.42, lat=37.77, 0)
        w.write_bd(-122.42);
        w.write_bd(37.77);
        w.write_bd(0.0);
        // reference_north (0, 1, 0)
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // x scale
        w.write_bd(1.0); // y scale
        write_utf16_tv(&mut w, "WGS84");
        write_utf16_tv(&mut w, ""); // empty RSS tag
        write_utf16_tv(&mut w, "OBS_FROM");
        write_utf16_tv(&mut w, "OBS_TO");
        write_utf16_tv(&mut w, "OBS_COV");

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let g = decode(&mut c, Version::R2018).unwrap();
        assert_eq!(g.version, 1);
        assert_eq!(g.block_handle.code, 5);
        assert_eq!(g.block_handle.value, 0x123);
        assert_eq!(
            g.reference_point,
            Point3D {
                x: -122.42,
                y: 37.77,
                z: 0.0
            }
        );
        assert_eq!(g.design_to_ref_x_scale, 1.0);
        assert_eq!(g.coordinate_system_def, "WGS84");
        assert_eq!(g.geo_rss_tag, "");
        assert_eq!(g.observation_from_tag, "OBS_FROM");
        assert_eq!(g.observation_to_tag, "OBS_TO");
        assert_eq!(g.observation_coverage_tag, "OBS_COV");
    }

    #[test]
    fn rejects_pre_r2010() {
        let bytes = [0u8; 4];
        let mut c = BitCursor::new(&bytes);
        // R2007 is rejected — GEODATA arrived in R2010.
        let err = decode(&mut c, Version::R2007).unwrap_err();
        assert!(
            matches!(&err, Error::Unsupported { feature } if feature.contains("GEODATA")),
            "err={err:?}"
        );
        let err = decode(&mut c, Version::R2004).unwrap_err();
        assert!(
            matches!(&err, Error::Unsupported { feature } if feature.contains("GEODATA")),
            "err={err:?}"
        );
    }
}
