//! ACSH_HISTORY_CLASS and ACSH_BOX_CLASS: the solid-history records AutoCAD
//! writes for a primitive solid.
//!
//! ODA spec v5.4.1 prints no layout for either. The field lists are derived from
//! AutoCAD's own output under the method in `CLEANROOM.md` (2026-10-26): the data
//! bits of the 21 corpus records (`sample_base` saved as R14 to R2018) against
//! the DXF object of the same handle. ARCHITECTURE.md §7bi holds the evidence.
//!
//! ```text
//! ACSH_HISTORY_CLASS   BL group 90 (33), BL group 91 (329), BL group 92 (1),
//!                      B group 280, B group 281
//! ACSH_BOX_CLASS       AcDbEvalExpr (as in `objects::dynamic_block`),
//!                      BL group 90, BL group 91,
//!                      BD x16 transform (groups 40 to 55),
//!                      colour (R2004+: BS, BL colour word, RC flags and the TVs
//!                      they name; before: one BS, 256 = ByLayer, group 62),
//!                      BL group 92,
//!                      BL group 90, BL group 91 of the box, BD x3 (40, 41, 42)
//! ```
//!
//! The DXF writes the last six of the 16 transform values (groups 50 to 55) in
//! degrees; the decoded values are as stored, so the probe converts. Handles
//! (groups 360 and 347) live in the handle stream and are not decoded. A colour
//! flag above 3, and an integer wider than the short forms the corpus holds, are
//! refused with [`crate::error::Error::Unsupported`]. ACSH_EXTRUSION_CLASS, which
//! carries a binary blob and two more matrices, is not decoded.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::objects::dynamic_block::{EvalExpr, read_eval};
use crate::objects::modern;
use crate::objects::small_classes::{open_and_finish, unsupported};
use crate::string_stream::StringReader;
use crate::version::Version;

/// ACSH_HISTORY_CLASS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShHistory {
    /// `BL`, DXF group 90 (33).
    pub dxf_90: u32,
    /// `BL`, DXF group 91 (329).
    pub dxf_91: u32,
    /// `BL`, DXF group 92 (1).
    pub dxf_92: u32,
    /// `B`, DXF group 280.
    pub dxf_280: bool,
    /// `B`, DXF group 281.
    pub dxf_281: bool,
}

/// The colour of a history node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShColor {
    /// `BS`: before R2004 the whole colour (256 = ByLayer, DXF group 62), from
    /// R2004 the leading `BS` of the colour, 0 in the corpus.
    pub index: i16,
    /// `BL` colour word from R2004 (0xC0000000 in the corpus, where the DXF
    /// writes group 62 = 256).
    pub word: Option<u32>,
    /// `TV` colour name when flag bit 0 is set, from R2004.
    pub name: Option<String>,
    /// `TV` colour book when flag bit 1 is set, from R2004.
    pub book: Option<String>,
}

/// ACSH_BOX_CLASS.
#[derive(Debug, Clone, PartialEq)]
pub struct ShBox {
    pub eval: EvalExpr,
    /// `BL`, DXF group 90 of the history node (33).
    pub node_90: u32,
    /// `BL`, DXF group 91 of the history node (329).
    pub node_91: u32,
    /// `BD` x16, DXF groups 40 to 55 (the last six in degrees in the DXF).
    pub transform: [f64; 16],
    pub color: ShColor,
    /// `BL`, DXF group 92 (3).
    pub node_92: u32,
    /// `BL`, DXF group 90 of the box (33).
    pub box_90: u32,
    /// `BL`, DXF group 91 of the box (329).
    pub box_91: u32,
    /// `BD` x3, DXF groups 40, 41 and 42 of the box.
    pub size: [f64; 3],
}

pub(crate) fn read_color(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<ShColor> {
    let index = c.read_bs()?;
    if !version.is_r2004_plus() {
        return Ok(ShColor {
            index,
            word: None,
            name: None,
            book: None,
        });
    }
    let word = c.read_bl_u()?;
    let flags = c.read_rc()?;
    if flags > 3 {
        return Err(unsupported("ACSH colour flag"));
    }
    let name = if flags & 1 != 0 {
        Some(modern::read_tv(c, strings, version)?)
    } else {
        None
    };
    let book = if flags & 2 != 0 {
        Some(modern::read_tv(c, strings, version)?)
    } else {
        None
    };
    Ok(ShColor {
        index,
        word: Some(word),
        name,
        book,
    })
}

/// Decode an ACSH_HISTORY_CLASS record: three `BL` and two `B` (DXF groups 90, 91,
/// 92, 280, 281), derived from AutoCAD's output (ARCHITECTURE.md §7bi), ending on
/// the data-stream boundary.
pub(crate) fn decode_history(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<ShHistory> {
    open_and_finish(
        "ACSH_HISTORY_CLASS",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, _| {
            Ok(ShHistory {
                dxf_90: c.read_bl_u()?,
                dxf_91: c.read_bl_u()?,
                dxf_92: c.read_bl_u()?,
                dxf_280: c.read_b()?,
                dxf_281: c.read_b()?,
            })
        },
    )
}

/// Decode an ACSH_BOX_CLASS record from the layout of the module docs (derived
/// from AutoCAD's output, ARCHITECTURE.md §7bi), ending exactly on the
/// data-stream boundary and, from R2007, with the string stream consumed.
pub(crate) fn decode_box(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<ShBox> {
    open_and_finish(
        "ACSH_BOX_CLASS",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            let eval = read_eval(c)?;
            let node_90 = c.read_bl_u()?;
            let node_91 = c.read_bl_u()?;
            let mut transform = [0.0; 16];
            for v in &mut transform {
                *v = c.read_bd()?;
            }
            let color = read_color(c, strings, version)?;
            let node_92 = c.read_bl_u()?;
            let box_90 = c.read_bl_u()?;
            let box_91 = c.read_bl_u()?;
            let size = [c.read_bd()?, c.read_bd()?, c.read_bd()?];
            Ok(ShBox {
                eval,
                node_90,
                node_91,
                transform,
                color,
                node_92,
                box_90,
                box_91,
                size,
            })
        },
    )
}
