//! CELLSTYLEMAP (§20.4.102) — the cell styles of a TABLESTYLE.
//!
//! The class is `AcDbCellStyleMap`, DXF name `CELLSTYLEMAP`, a custom class
//! whose object type code is assigned per file through `AcDb:Classes`. It was
//! introduced with AutoCAD 2008 as a separate container for the cell styles
//! of a table style, and the table style reaches it through the
//! `ACAD_ROUNDTRIP_2008_TABLESTYLE_CELLSTYLEMAP` entry of its extension
//! dictionary.
//!
//! §20.4.102 prints the data stream as `BL` number of cell styles, then per
//! style a cell style (§20.4.101.4, read by
//! `crate::objects::table_cell_style`), `BL` id, `BL` class and `TV` name.
//! Each corpus drawing from R2000 to R2007 holds one, with the three cell
//! styles `_TITLE` (id 1), `_HEADER` (2) and `_DATA` (3); an R2010 or later
//! drawing does not hold one (its TABLESTYLE carries the styles itself) and
//! AutoCAD writes the object afresh on every save, so the DWG's and the DXF's
//! never share a handle.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::objects::table_cell_style::{NamedCellStyle, read_named_cell_style};
use crate::string_stream::StringReader;
use crate::version::Version;

/// Most cell styles a CELLSTYLEMAP claims.
const MAX_CELL_STYLES: u32 = 4096;

/// A decoded CELLSTYLEMAP.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AcadCellStyleMap {
    /// The cell styles, with their ids, classes and names.
    pub cell_styles: Vec<NamedCellStyle>,
}

fn read_body(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<AcadCellStyleMap> {
    let count = c.read_bl_u()?;
    if count > MAX_CELL_STYLES || count as usize > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "CELLSTYLEMAP claims {count} cell styles in {} remaining bits",
            c.remaining_bits()
        )));
    }
    let mut cell_styles = Vec::with_capacity(count as usize);
    for _ in 0..count {
        cell_styles.push(read_named_cell_style(c, strings, version)?);
    }
    Ok(AcadCellStyleMap { cell_styles })
}

/// Decode a CELLSTYLEMAP (§20.4.102) straight from its raw object payload,
/// taking every `TV` from the string stream when the file has one and
/// checking the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadCellStyleMap> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings, version)?;
    split.finish("CELLSTYLEMAP")?;
    Ok(out)
}
