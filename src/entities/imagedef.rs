//! IMAGEDEF object (§19.5.26) — raster-image definition.
//!
//! IMAGEDEF is the object-side half of a raster IMAGE entity. One
//! IMAGE entity references one IMAGEDEF via handle; IMAGEDEF stores
//! the file path and pixel-size metadata for the actual raster on
//! disk. The object lives in the named-object dictionary under
//! `ACAD_IMAGE_DICT`.
//!
//! # Stream shape
//!
//! ```text
//! BL    class_version      -- always 0 in observed files
//! BD2   image_size_pixels  -- (width, height) in image pixels
//! TV    file_path          -- absolute or relative path to the raster
//! B     is_loaded
//! RC    pixel_size_units   -- 0 = unitless, 1 = millimeter, 2 = centimeter,
//!                             3 = meter, 4 = kilometer, 5 = inch, 6 = foot,
//!                             7 = yard, 8 = mile
//! BD    pixel_width_size   -- world units per pixel along U
//! BD    pixel_height_size  -- world units per pixel along V
//! ```
//!
//! The `TV` path is version-aware: R2007+ encodes it as UTF-16LE,
//! earlier versions use 8-bit ASCII/MBCS — same rule as every other
//! TV field in the crate.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::version::Version;

/// Decoded IMAGEDEF — path + pixel metadata for a raster IMAGE.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageDef {
    pub class_version: u32,
    /// (width, height) in image pixels.
    pub image_size_pixels: (f64, f64),
    /// Absolute or relative filesystem path to the raster file.
    pub file_path: String,
    pub is_loaded: bool,
    /// Per spec §19.5.26: 0 = unitless, 1 = mm, 2 = cm, 3 = m,
    /// 4 = km, 5 = in, 6 = ft, 7 = yd, 8 = mi.
    pub pixel_size_units: u8,
    pub pixel_width_size: f64,
    pub pixel_height_size: f64,
}

/// Defensive cap on the TV (file path) length. Real IMAGEDEFs point at
/// filesystem paths — well under 4 KiB even on the worst HiDPI-asset
/// pipeline. 64 KiB is already adversarial territory.
const IMAGEDEF_MAX_PATH_UNITS: usize = 65_536;

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<ImageDef> {
    let class_version = c.read_bl()? as u32;
    let image_width = c.read_bd()?;
    let image_height = c.read_bd()?;
    let file_path = read_tv(c, version)?;
    let is_loaded = c.read_b()?;
    let pixel_size_units = c.read_rc()?;
    let pixel_width_size = c.read_bd()?;
    let pixel_height_size = c.read_bd()?;
    Ok(ImageDef {
        class_version,
        image_size_pixels: (image_width, image_height),
        file_path,
        is_loaded,
        pixel_size_units,
        pixel_width_size,
        pixel_height_size,
    })
}

/// Local TV reader — the `tables::read_tv` helper is `pub(crate)` and
/// duplicating its logic inline here avoids widening its visibility
/// just for this module. The behaviour is identical: version-aware
/// UTF-16 / 8-bit selection with a trailing-NUL strip.
fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if len > IMAGEDEF_MAX_PATH_UNITS || len > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "IMAGEDEF file path length {len} exceeds cap \
             ({IMAGEDEF_MAX_PATH_UNITS} or remaining_bits {})",
            c.remaining_bits()
        )));
    }
    if version.is_r2007_plus() {
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
            .map_err(|_| Error::SectionMap("IMAGEDEF file_path is not valid UTF-16".into()))
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

    fn write_tv_8bit(w: &mut BitWriter, s: &str) {
        w.write_bs_u((s.len() + 1) as u16); // +1 for trailing NUL
        for b in s.as_bytes() {
            w.write_rc(*b);
        }
        w.write_rc(0);
    }

    #[test]
    fn roundtrip_minimal_imagedef() {
        let mut w = BitWriter::new();
        w.write_bl(0); // class_version
        w.write_bd(1920.0);
        w.write_bd(1080.0);
        write_tv_8bit(&mut w, "C:\\drawings\\bg.png");
        w.write_b(true); // is_loaded
        w.write_rc(1); // mm
        w.write_bd(0.5);
        w.write_bd(0.5);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let i = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(i.class_version, 0);
        assert_eq!(i.image_size_pixels, (1920.0, 1080.0));
        assert_eq!(i.file_path, "C:\\drawings\\bg.png");
        assert!(i.is_loaded);
        assert_eq!(i.pixel_size_units, 1);
        assert_eq!(i.pixel_width_size, 0.5);
        assert_eq!(i.pixel_height_size, 0.5);
    }

    #[test]
    fn roundtrip_empty_path() {
        let mut w = BitWriter::new();
        w.write_bl(0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bs_u(0); // empty TV
        w.write_b(false);
        w.write_rc(0); // unitless
        w.write_bd(1.0);
        w.write_bd(1.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let i = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(i.file_path, "");
        assert!(!i.is_loaded);
        assert_eq!(i.pixel_size_units, 0);
    }

    #[test]
    fn rejects_oversized_tv_claim() {
        // Claim a path length larger than the cap — must reject before
        // any RC reads (defensive-allocation).
        let mut w = BitWriter::new();
        w.write_bl(0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // Claim IMAGEDEF_MAX_PATH_UNITS + 1 — beyond the cap.
        // Because write_bs_u is u16 we can't encode past 65535; use the
        // cap itself (65536) by writing 0 in the BS_U and... actually
        // bs_u is limited to u16 so max is 65535. The cap catches
        // 65536+; use remaining_bits branch for this test instead.
        //
        // Drop a large claim (within u16) but with no payload behind
        // it — remaining_bits will be smaller than the claim.
        w.write_bs_u(60_000);
        // no payload
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("IMAGEDEF")));
    }
}
