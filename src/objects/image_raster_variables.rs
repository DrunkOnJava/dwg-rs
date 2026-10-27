//! RASTERVARIABLES (§20.4.91) — the drawing-wide settings of raster images.
//!
//! The class is `AcDbRasterVariables`, DXF name `RASTERVARIABLES`, a custom
//! class whose object type code is assigned per file through `AcDb:Classes`.
//! The record is the common object data and four fields:
//!
//! ```text
//! BL  class version         DXF 90
//! BS  display image frame   DXF 70   IMAGEFRAME
//! BS  display quality       DXF 71   IMAGEQUALITY
//! BS  units                 DXF 72
//! ```
//!
//! A drawing holds one when it holds an IMAGE; each of the corpus's closes
//! on its data-stream boundary from R14 to R2018.

use crate::error::Result;
use crate::objects::modern;
use crate::version::Version;

/// A decoded RASTERVARIABLES.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RasterVariables {
    /// `BL` class version, DXF 90.
    pub class_version: u32,
    /// `BS` display image frame, DXF 70.
    pub display_frame: i16,
    /// `BS` display quality, DXF 71.
    pub display_quality: i16,
    /// `BS` units, DXF 72.
    pub units: i16,
}

/// Decode a RASTERVARIABLES from its raw object payload (§20.4.91),
/// checking its `BL` and three `BS` end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<RasterVariables> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = RasterVariables {
        class_version: split.data.read_bl_u()?,
        display_frame: split.data.read_bs()?,
        display_quality: split.data.read_bs()?,
        units: split.data.read_bs()?,
    };
    split.finish("RASTERVARIABLES")?;
    Ok(out)
}
