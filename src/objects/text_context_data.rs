//! ACDB_TEXTOBJECTCONTEXTDATA_CLASS (`AcDbTextObjectContextData`): the
//! annotation-scale state of a text object.
//!
//! ODA spec v5.4.1 prints no layout for it. The field list is derived from
//! AutoCAD's own output under the method in `CLEANROOM.md` (2026-10-26): the data
//! bits of the seven corpus records (R2010, R2013 and R2018) against the DXF object
//! of the same handle. ARCHITECTURE.md §7bm holds the evidence.
//!
//! ```text
//! BS  version                   (DXF group 70 of AcDbObjectContextData: 4)
//! B   default flag              (group 290: 1)
//! BS  flag                      (group 70 of AcDbTextObjectContextData: 0)
//! BD  rotation                  (group 50)
//! RD  groups 10, 20, 11, 21     (two 2D points)
//! ```
//!
//! The handle of the annotation scale (group 340) lives in the handle stream and is
//! not decoded. A release before R2010 is refused with
//! [`crate::error::Error::Unsupported`]: the corpus holds the class's objects on
//! R2010, R2013 and R2018 only.

use crate::error::{Error, Result};
use crate::objects::small_classes::open_and_finish;
use crate::version::Version;

/// A decoded ACDB_TEXTOBJECTCONTEXTDATA_CLASS record.
#[derive(Debug, Clone, PartialEq)]
pub struct TextObjectContextData {
    /// `BS`, DXF group 70 of the object-context subclass (4).
    pub dxf_70_version: i16,
    /// `B`, DXF group 290.
    pub dxf_290: bool,
    /// `BS`, DXF group 70 of the text subclass (0 in the corpus).
    pub dxf_70_flag: i16,
    /// `BD`, DXF group 50. The corpus shows it as 0 in both DXF twins and as
    /// 0.5235987755982988 (30 degrees as radians) in a record with no twin, so its
    /// unit in the DXF is unverified by value.
    pub dxf_50: f64,
    /// `RD`, DXF group 10.
    pub dxf_10: f64,
    /// `RD`, DXF group 20.
    pub dxf_20: f64,
    /// `RD`, DXF group 11 (0.0 in every record: the order of groups 11 and 21 is
    /// unverified).
    pub dxf_11: f64,
    /// `RD`, DXF group 21.
    pub dxf_21: f64,
}

/// Decode an ACDB_TEXTOBJECTCONTEXTDATA_CLASS record straight from its raw object
/// payload, from the layout of the module docs (derived from AutoCAD's output,
/// ARCHITECTURE.md §7bm), ending exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<TextObjectContextData> {
    if !version.is_r2010_plus() {
        return Err(Error::Unsupported {
            feature: "ACDB_TEXTOBJECTCONTEXTDATA_CLASS before R2010".into(),
        });
    }
    open_and_finish(
        "ACDB_TEXTOBJECTCONTEXTDATA_CLASS",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, _| {
            Ok(TextObjectContextData {
                dxf_70_version: c.read_bs()?,
                dxf_290: c.read_b()?,
                dxf_70_flag: c.read_bs()?,
                dxf_50: c.read_bd()?,
                dxf_10: c.read_rd()?,
                dxf_20: c.read_rd()?,
                dxf_11: c.read_rd()?,
                dxf_21: c.read_rd()?,
            })
        },
    )
}
