//! RAPIDRTRENDERENVIRONMENT: the environment (an image-based lighting backdrop, by the
//! name its C++ class carries in `AcDb:Classes`, `AcDbIBLBackground`) the RapidRT renderer
//! lights a drawing with.
//!
//! ODA spec v5.4.1 prints no layout for it. The field list is derived from the bits of the
//! eight records of two R14 drawings and held to the record boundary alone: the corpus holds
//! no DXF export of either drawing, so no value is scored (CLEANROOM.md, 2026-10-28;
//! ARCHITECTURE.md §7bs).
//!
//! ```text
//! BL   version                 -- 2 in every record
//! B    enabled                 -- 1 in six records, 0 in two
//! TV   image                   -- a path to an HDR file, inline before R2007
//! BD   rotation                -- 0.0 in every record
//! B    display_image           -- 0 in three records, 1 in five
//! ```
//!
//! The record closes on its boundary on all eight, whatever the length of the path. The
//! two flags and the roles of the `BL` and the `BD` are read from the values and are
//! unverified; the names follow the groups of the IBL background in Autodesk's public DXF
//! Reference, recalled, for names only. The hard handle of a secondary background (if the
//! object holds one) lives in the handle stream and is not read here.

use crate::error::Result;
use crate::objects::modern;
use crate::objects::small_classes::open_and_finish;
use crate::version::Version;

/// RAPIDRTRENDERENVIRONMENT: an HDR backdrop for the RapidRT renderer. Held to its record
/// boundary alone; no DXF twin scores any value (ARCHITECTURE.md §7bs).
#[derive(Debug, Clone, PartialEq)]
pub struct RenderEnvironment {
    /// `BL`: 2 in every corpus record.
    pub version: u32,
    /// `B`: the first flag, 1 in six corpus records and 0 in two.
    pub enabled: bool,
    /// `TV`: the path of the HDR image.
    pub image: String,
    /// `BD`: 0.0 in every corpus record.
    pub rotation: f64,
    /// `B`: the last flag, 0 in three corpus records and 1 in five.
    pub display_image: bool,
}

/// Decode a RAPIDRTRENDERENVIRONMENT record from the layout of the module docs (derived from
/// AutoCAD's output, ARCHITECTURE.md §7bs), ending exactly on the data-stream boundary and,
/// from R2007, with the string stream consumed.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<RenderEnvironment> {
    open_and_finish(
        "RAPIDRTRENDERENVIRONMENT",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            let version_word = c.read_bl_u()?;
            let enabled = c.read_b()?;
            let image = modern::read_tv(c, strings, version)?;
            let rotation = c.read_bd()?;
            let display_image = c.read_b()?;
            Ok(RenderEnvironment {
                version: version_word,
                enabled,
                image,
                rotation,
                display_image,
            })
        },
    )
}
