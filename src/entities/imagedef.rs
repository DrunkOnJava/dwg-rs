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
//! 2RD   image_size_pixels  -- (width, height) in image pixels
//! TV    file_path          -- absolute or relative path to the raster
//! B     is_loaded
//! RC    pixel_size_units   -- 0 = unitless, 1 = millimeter, 2 = centimeter,
//!                             3 = meter, 4 = kilometer, 5 = inch, 6 = foot,
//!                             7 = yard, 8 = mile
//! 2RD   pixel size         -- world units per pixel along U and V
//! ```
//!
//! # Measured: the two point fields are `2RD`, not `BD` pairs
//!
//! §20.4.81 types `Imgsize` and `Pixelsize` as `2RD` — raw doubles.
//! Read as `BD`s the single IMAGEDEF of `sample_AC1032.dwg` (handle
//! `0x8C4`) lands on a reserved `11` bit pattern; read as `2RD` it
//! closes exactly on the record's data-stream boundary.
//!
//! The `TV` path is version-aware: R2007+ encodes it as UTF-16LE,
//! earlier versions use 8-bit ASCII/MBCS — same rule as every other
//! TV field in the crate. From R2007 it is not merely re-encoded but
//! *relocated*, into the object's string stream (§19.1), so
//! `decode_object` is the path a modern file takes.
//!
//! IMAGEDEF is a non-entity object, so it carries the common *object*
//! data of §19.4.2 rather than the common entity preamble, and it is a
//! custom class — its type code is assigned per file through
//! `AcDb:Classes`.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::string_stream::StringReader;
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

/// Decodes the `ImageDef` payload that follows the common object header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<ImageDef> {
    read_body(c, &mut None, version)
}

/// Decode an IMAGEDEF straight from its raw object payload, taking the
/// file path from the R2007+ string stream when the file has one and
/// checking the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<ImageDef> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings, version)?;
    split.finish("IMAGEDEF")?;
    Ok(out)
}

/// Read the IMAGEDEF field list from whichever streams hold it.
fn read_body(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<ImageDef> {
    let class_version = c.read_bl()? as u32;
    let image_width = c.read_rd()?;
    let image_height = c.read_rd()?;
    let file_path = match strings.as_mut() {
        Some(reader) => reader.read_tv()?,
        None => read_tv(c, version)?,
    };
    let is_loaded = c.read_b()?;
    let pixel_size_units = c.read_rc()?;
    let pixel_width_size = c.read_rd()?;
    let pixel_height_size = c.read_rd()?;
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

/// Inline TV reader for the pre-R2007 layout. Unlike
/// [`crate::tables::read_tv`] it caps the claimed length before
/// allocating, because an IMAGEDEF path is the one `TV` in this crate
/// whose length comes straight from an untrusted file with no
/// surrounding field to bound it.
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
        Ok(crate::code_page::decode_active(&bytes))
    }
}
