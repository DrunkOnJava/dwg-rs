//! TABLEGEOMETRY (§20.4.103) — the laid-out geometry of a table.
//!
//! The class is `AcDbTableGeometry`, DXF name `TABLEGEOMETRY`, a custom class
//! whose object type code is assigned per file through `AcDb:Classes`. It was
//! introduced with AutoCAD 2008 beside TABLECONTENT, "does not need to be
//! present in a DWG file", and records, per cell of a table, the space the
//! cell and its contents take.
//!
//! # Field list
//!
//! §20.4.103 prints the data stream as:
//!
//! ```text
//! BL    row count                    DXF 90
//! BL    column count                 DXF 91
//! BL    row x column count           DXF 92
//! per cell, row by row:
//!   BL    flags                      DXF 93
//!   BD    width with gap             DXF 40
//!   BD    height with gap            DXF 41
//!   H     unknown, soft pointer      DXF 330   handle stream, not read here
//!   BL    content count              DXF 94
//!   per content:
//!     3BD   distance to top left     DXF 10, 20, 30
//!     3BD   distance to center       DXF 11, 21, 31
//!     BD    content width            DXF 43
//!     BD    content height           DXF 44
//!     BD    width                    DXF 45
//!     BD    height                   DXF 46
//!     BD    unknown, 0               DXF 95
//! ```
//!
//! The last value is typed `BD` in the spec's table and carried under an
//! integer group, 95; a `BD` of 0 and a `BL` of 0 are the same two bits, `10`,
//! and every corpus record holds 0, so the width does not tell them apart. It
//! is read as `BL`, the DXF's type, and a non-zero value would show as a
//! record that does not close.
//!
//! A record whose third count is not the product of the first two is refused.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

/// Fewest bits of one cell: `BL`, two `BD` and `BL`, each two bits at least.
const MIN_CELL_BITS: usize = 8;

/// Fewest bits of one content: two `3BD`, four `BD` and a `BL`.
const MIN_CONTENT_BITS: usize = 22;

/// What one cell holds at one position.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableGeometryContent {
    /// `3BD` distance to top left, DXF 10, 20, 30.
    pub distance_to_top_left: [f64; 3],
    /// `3BD` distance to center, DXF 11, 21, 31.
    pub distance_to_center: [f64; 3],
    /// `BD` content width, DXF 43.
    pub content_width: f64,
    /// `BD` content height, DXF 44.
    pub content_height: f64,
    /// `BD` width, DXF 45.
    pub width: f64,
    /// `BD` height, DXF 46.
    pub height: f64,
    /// `BL` unknown, DXF 95, 0 on every corpus record.
    pub unknown: u32,
}

/// One cell of the table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableGeometryCell {
    /// `BL` flags, DXF 93.
    pub flags: u32,
    /// `BD` width with gap, DXF 40.
    pub width_with_gap: f64,
    /// `BD` height with gap, DXF 41.
    pub height_with_gap: f64,
    /// The cell's contents.
    pub contents: Vec<TableGeometryContent>,
}

/// A decoded TABLEGEOMETRY.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableGeometry {
    /// `BL` row count, DXF 90.
    pub rows: u32,
    /// `BL` column count, DXF 91.
    pub columns: u32,
    /// The cells, row by row.
    pub cells: Vec<TableGeometryCell>,
}

fn read_3bd(c: &mut BitCursor<'_>) -> Result<[f64; 3]> {
    Ok([c.read_bd()?, c.read_bd()?, c.read_bd()?])
}

fn read_content(c: &mut BitCursor<'_>) -> Result<TableGeometryContent> {
    Ok(TableGeometryContent {
        distance_to_top_left: read_3bd(c)?,
        distance_to_center: read_3bd(c)?,
        content_width: c.read_bd()?,
        content_height: c.read_bd()?,
        width: c.read_bd()?,
        height: c.read_bd()?,
        unknown: c.read_bl_u()?,
    })
}

fn read_cell(c: &mut BitCursor<'_>) -> Result<TableGeometryCell> {
    let flags = c.read_bl_u()?;
    let width_with_gap = c.read_bd()?;
    let height_with_gap = c.read_bd()?;
    let count = c.read_bl_u()? as usize;
    if count > c.remaining_bits() / MIN_CONTENT_BITS {
        return Err(Error::SectionMap(format!(
            "TABLEGEOMETRY cell claims {count} contents in {} remaining bits",
            c.remaining_bits()
        )));
    }
    let mut contents = Vec::with_capacity(count);
    for _ in 0..count {
        contents.push(read_content(c)?);
    }
    Ok(TableGeometryCell {
        flags,
        width_with_gap,
        height_with_gap,
        contents,
    })
}

fn read_body(
    c: &mut BitCursor<'_>,
    _strings: &mut Option<StringReader<'_>>,
) -> Result<TableGeometry> {
    let rows = c.read_bl_u()?;
    let columns = c.read_bl_u()?;
    let total = c.read_bl_u()?;
    if u64::from(rows) * u64::from(columns) != u64::from(total) {
        return Err(Error::SectionMap(format!(
            "TABLEGEOMETRY has {rows} rows and {columns} columns but {total} cells"
        )));
    }
    if total as usize > c.remaining_bits() / MIN_CELL_BITS {
        return Err(Error::SectionMap(format!(
            "TABLEGEOMETRY claims {total} cells in {} remaining bits",
            c.remaining_bits()
        )));
    }
    let mut cells = Vec::with_capacity(total as usize);
    for _ in 0..total {
        cells.push(read_cell(c)?);
    }
    Ok(TableGeometry {
        rows,
        columns,
        cells,
    })
}

/// Decode a TABLEGEOMETRY (§20.4.103) straight from its raw object payload,
/// checking the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<TableGeometry> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings)?;
    split.finish("TABLEGEOMETRY")?;
    Ok(out)
}
