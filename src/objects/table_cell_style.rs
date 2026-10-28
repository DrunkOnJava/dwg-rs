//! Cell styles (§20.4.101.3 and §20.4.101.4) — the reader TABLESTYLE's
//! R2010 layout and CELLSTYLEMAP share.
//!
//! §20.4.101.4 prints a cell style as a type, a data flag and, when the flag
//! says data is present, the style's property overrides, content format,
//! margins and borders:
//!
//! ```text
//! BL    cell style type                  DXF 90   1 cell, 2 row, 3 column,
//!                                                 4 formatted table data, 5 table
//! BS    data flags                       DXF 170  0 no data, 1 data present
//! if data present:
//!   BL  property override flags          DXF 91
//!   BL  merge flags                      DXF 92
//!   TC  background colour                DXF 62
//!   BL  content layout flags             DXF 93   1 flow, 2 stacked horizontal,
//!                                                 4 stacked vertical
//!   ..  content format (below)
//!   BS  margin override flags            DXF 171
//!   if the margin overrides are present:
//!     6 x BD margins                     DXF 40   vertical, horizontal, bottom,
//!                                                 right, horizontal spacing,
//!                                                 vertical spacing
//!   BL  number of borders                DXF 94   0 to 6
//!   per border:
//!     BL  edge flags                     DXF 95   1 top, 2 right, 4 bottom,
//!                                                 8 left, 0x10 inside vertical,
//!                                                 0x20 inside horizontal
//!     if the edge flags are not zero:
//!       BL  border property overrides    DXF 90
//!       BL  border type                  DXF 91   1 single, 2 double
//!       TC  colour                       DXF 62
//!       BL  line weight                  DXF 92
//!       H   line type                    DXF 340  handle stream, not read here
//!       BL  invisibility                 DXF 93   1 invisible
//!       BD  double line spacing          DXF 40
//! ```
//!
//! and §20.4.101.3 prints the content format:
//!
//! ```text
//! BL  property override flags   DXF 90
//! BL  property flags            DXF 91
//! BL  value data type           DXF 92
//! BL  value unit type           DXF 93
//! TV  value format string       DXF 300
//! BD  rotation                  DXF 40
//! BD  block scale               DXF 140
//! BL  cell alignment            DXF 94   1 top left through 9 bottom right
//! TC  content colour            DXF 62
//! H   text style                DXF 340  handle stream, not read here
//! BD  text height               DXF 144
//! ```
//!
//! Every `TC` is the 44-bit `BS`, `BL`, `RC` form on every release the corpus
//! holds, R2000 included, as it is in TABLESTYLE's earlier layout.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::color::{self, ObjectColor};
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

/// Most borders a cell style holds: top, right, bottom, left, inside
/// vertical and inside horizontal.
const MAX_BORDERS: u32 = 6;

/// A content format (§20.4.101.3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableContentFormat {
    /// `BL` property override flags, DXF 90.
    pub property_override_flags: u32,
    /// `BL` property flags, DXF 91; only the auto-scale bit 0x100 is used.
    pub property_flags: u32,
    /// `BL` value data type, DXF 92.
    pub value_data_type: u32,
    /// `BL` value unit type, DXF 93.
    pub value_unit_type: u32,
    /// `TV` value format string, DXF 300.
    pub value_format: String,
    /// `BD` rotation, DXF 40.
    pub rotation: f64,
    /// `BD` block scale, DXF 140.
    pub block_scale: f64,
    /// `BL` cell alignment, DXF 94.
    pub cell_alignment: u32,
    /// `TC` content colour, DXF 62.
    pub content_color: ObjectColor,
    /// `BD` text height, DXF 144.
    pub text_height: f64,
}

/// One border of a cell style (§20.4.101.4).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCellBorder {
    /// `BL` edge flags, DXF 95.
    pub edge_flags: u32,
    /// The border's properties; `None` when the edge flags are zero.
    pub properties: Option<TableBorderProperties>,
}

/// The properties of a border whose edge flags are not zero.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableBorderProperties {
    /// `BL` border property override flags, DXF 90.
    pub override_flags: u32,
    /// `BL` border type, DXF 91.
    pub border_type: u32,
    /// `TC` colour, DXF 62.
    pub color: ObjectColor,
    /// `BL` line weight, DXF 92.
    pub line_weight: i32,
    /// `BL` invisibility, DXF 93.
    pub invisible: u32,
    /// `BD` double line spacing, DXF 40.
    pub double_line_spacing: f64,
}

/// The data of a cell style whose data flag is 1.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCellStyleData {
    /// `BL` property override flags, DXF 91.
    pub property_override_flags: u32,
    /// `BL` merge flags, DXF 92.
    pub merge_flags: u32,
    /// `TC` background colour, DXF 62.
    pub background_color: ObjectColor,
    /// `BL` content layout flags, DXF 93.
    pub content_layout_flags: u32,
    /// The content format.
    pub content_format: TableContentFormat,
    /// `BS` margin override flags, DXF 171.
    pub margin_override_flags: i16,
    /// The six margins, DXF 40, when the margin override flags say so.
    pub margins: Option<[f64; 6]>,
    /// The borders.
    pub borders: Vec<TableCellBorder>,
}

/// A cell style (§20.4.101.4).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCellStyle {
    /// `BL` cell style type, DXF 90.
    pub cell_style_type: u32,
    /// The style's data; `None` when the data flag, DXF 170, is 0.
    pub data: Option<TableCellStyleData>,
}

/// A cell style with the id, class and name that follow it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NamedCellStyle {
    /// The cell style.
    pub style: TableCellStyle,
    /// `BL` cell style id, DXF 90: 1 title, 2 header, 3 data, 4 table, 101
    /// and up custom.
    pub id: u32,
    /// `BL` cell style class, DXF 91: 1 data, 2 label.
    pub class: u32,
    /// `TV` cell style name, DXF 300.
    pub name: String,
}

fn read_content_format(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<TableContentFormat> {
    Ok(TableContentFormat {
        property_override_flags: c.read_bl_u()?,
        property_flags: c.read_bl_u()?,
        value_data_type: c.read_bl_u()?,
        value_unit_type: c.read_bl_u()?,
        value_format: modern::read_tv(c, strings, version)?,
        rotation: c.read_bd()?,
        block_scale: c.read_bd()?,
        cell_alignment: c.read_bl_u()?,
        content_color: color::read_tc(c, strings, version)?,
        text_height: c.read_bd()?,
    })
}

fn read_border(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<TableCellBorder> {
    let edge_flags = c.read_bl_u()?;
    if edge_flags == 0 {
        return Ok(TableCellBorder {
            edge_flags,
            properties: None,
        });
    }
    let properties = TableBorderProperties {
        override_flags: c.read_bl_u()?,
        border_type: c.read_bl_u()?,
        color: color::read_tc(c, strings, version)?,
        line_weight: c.read_bl()?,
        invisible: c.read_bl_u()?,
        double_line_spacing: c.read_bd()?,
    };
    Ok(TableCellBorder {
        edge_flags,
        properties: Some(properties),
    })
}

fn read_data(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<TableCellStyleData> {
    let property_override_flags = c.read_bl_u()?;
    let merge_flags = c.read_bl_u()?;
    let background_color = color::read_tc(c, strings, version)?;
    let content_layout_flags = c.read_bl_u()?;
    let content_format = read_content_format(c, strings, version)?;
    let margin_override_flags = c.read_bs()?;
    let margins = if margin_override_flags & 1 != 0 {
        let mut m = [0.0; 6];
        for v in &mut m {
            *v = c.read_bd()?;
        }
        Some(m)
    } else {
        None
    };
    let count = c.read_bl_u()?;
    if count > MAX_BORDERS {
        return Err(Error::SectionMap(format!(
            "cell style claims {count} borders, at most {MAX_BORDERS}"
        )));
    }
    let mut borders = Vec::with_capacity(count as usize);
    for _ in 0..count {
        borders.push(read_border(c, strings, version)?);
    }
    Ok(TableCellStyleData {
        property_override_flags,
        merge_flags,
        background_color,
        content_layout_flags,
        content_format,
        margin_override_flags,
        margins,
        borders,
    })
}

/// Read one cell style (§20.4.101.4).
pub(crate) fn read_cell_style(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<TableCellStyle> {
    let cell_style_type = c.read_bl_u()?;
    let data = match c.read_bs()? {
        0 => None,
        1 => Some(read_data(c, strings, version)?),
        other => {
            return Err(Error::SectionMap(format!(
                "cell style data flag is {other}, not 0 or 1"
            )));
        }
    };
    Ok(TableCellStyle {
        cell_style_type,
        data,
    })
}

/// Read a cell style and the `BL` id, `BL` class and `TV` name after it.
pub(crate) fn read_named_cell_style(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<NamedCellStyle> {
    Ok(NamedCellStyle {
        style: read_cell_style(c, strings, version)?,
        id: c.read_bl_u()?,
        class: c.read_bl_u()?,
        name: modern::read_tv(c, strings, version)?,
    })
}
