//! SUN (`AcDbSun`): the sun of a drawing's lighting settings.
//!
//! ODA spec v5.4.1 prints no layout for it. The field list is derived from
//! AutoCAD's own output under the method in `CLEANROOM.md` (2026-10-26): the data
//! bits of the one SUN record of the corpus (`geoloc.dwg`, R2018) against the
//! `AcDbSun` object of the same handle in its DXF export. ARCHITECTURE.md §7y holds
//! the evidence. SUN is a non-entity object that AutoCAD hangs on a dictionary, so
//! it carries the common object data and reaches the dispatcher as a custom class.
//!
//! ```text
//! BL  version                  (group 90: 1)
//! B   status                   (group 290)
//! colour                       (group 421 is the low 24 bits of its BL word)
//!       BS, BL word, RC flags, then the TVs the flags name
//! BD  intensity                (group 40)
//! B   shadows                  (group 291)
//! BL  julian day               (group 91)
//! BL  time                     (group 92: 54000000)
//! B   daylight savings         (group 292)
//! BS  shadow type              (group 70)
//! BS  shadow map size          (group 71: 256)
//! RC  shadow softness          (group 280)
//! ```
//!
//! The owner handle (group 330) lives in the handle stream and is not decoded. A
//! release before R2007, a colour flag above 3 and an integer wider than the short
//! forms the corpus holds are refused with [`crate::error::Error::Unsupported`]
//! rather than read on a guess.

use crate::error::{Error, Result};
use crate::objects::sh_history::{ShColor, read_color};
use crate::objects::small_classes::open_and_finish;
use crate::version::Version;

/// A decoded SUN record.
#[derive(Debug, Clone, PartialEq)]
pub struct Sun {
    /// `BL`, DXF group 90 (1).
    pub dxf_90: u32,
    /// `B`, DXF group 290.
    pub dxf_290: bool,
    /// The sun colour: the low 24 bits of its word are DXF group 421. DXF group 63
    /// (7 in the corpus) is not a field of the data bits; the leading `BS` reads 0.
    pub color: ShColor,
    /// `BD`, DXF group 40.
    pub dxf_40: f64,
    /// `B`, DXF group 291.
    pub dxf_291: bool,
    /// `BL`, DXF group 91.
    pub dxf_91: u32,
    /// `BL`, DXF group 92.
    pub dxf_92: u32,
    /// `B`, DXF group 292.
    pub dxf_292: bool,
    /// `BS`, DXF group 70.
    pub dxf_70: i16,
    /// `BS`, DXF group 71.
    pub dxf_71: i16,
    /// `RC`, DXF group 280.
    pub dxf_280: u8,
}

/// Decode a SUN record straight from its raw object payload, from the layout of
/// the module docs (derived from AutoCAD's output, ARCHITECTURE.md §7y), ending
/// exactly on the data-stream boundary with the string stream consumed.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<Sun> {
    if !version.is_r2007_plus() {
        return Err(Error::Unsupported {
            feature: "SUN before R2007".into(),
        });
    }
    open_and_finish(
        "SUN",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            Ok(Sun {
                dxf_90: c.read_bl_u()?,
                dxf_290: c.read_b()?,
                color: read_color(c, strings, version)?,
                dxf_40: c.read_bd()?,
                dxf_291: c.read_b()?,
                dxf_91: c.read_bl_u()?,
                dxf_92: c.read_bl_u()?,
                dxf_292: c.read_b()?,
                dxf_70: c.read_bs()?,
                dxf_71: c.read_bs()?,
                dxf_280: c.read_rc()?,
            })
        },
    )
}
