//! TABLESTYLE (§20.4.101.1) — the style of a TABLE entity, in the layout
//! §20.4.101.1 titles "until R21".
//!
//! `TABLESTYLE` is a custom class (`AcDbTableStyle`), so its object type
//! code is assigned per file through `AcDb:Classes` and the dispatcher
//! resolves it by DXF class name. Every drawing of the public corpus holds
//! exactly one, `Standard`, and AutoCAD's DXF export of each (R2000 and
//! later) carries the same object.
//!
//! # Field list
//!
//! §20.4.101.1 prints the record as a header and three row styles, in the
//! order data, title, header:
//!
//! ```text
//! TV    description                  DXF 3
//! BS    flow direction               70    0 = down, 1 = up
//! BS    bit flags                    71
//! BD    horizontal cell margin       40
//! BD    vertical cell margin         41
//! B     suppress title               280
//! B     suppress header              281
//! 3 x   row style
//!         H   text style             7     handle stream, not read here
//!         BD  text height            140
//!         BS  text alignment         170
//!         TC  text colour            62
//!         TC  fill colour            63
//!         B   background enabled     283
//!         6 x border (top, horizontal inside, bottom, left, vertical inside, right)
//!               BS  line weight      274-279
//!               B   visible          284-289
//!               TC  colour           64-69
//!         R2007+:  BL data type      90
//!                  BL data unit type 91
//!                  TV format string  1
//! ```
//!
//! The data, title and header row styles are, in AutoCAD's own words in the
//! DXF twin, the `_DATA`, `_TITLE` and `_HEADER` cell styles.
//!
//! # Measured, where §20.4.101.1 is incomplete
//!
//! Two readings are not in the spec's table and come from the corpus's bits
//! (ARCHITECTURE.md, "TABLESTYLE until R2007"):
//!
//! - **The data type, unit type and format string are R2007+ together.** The
//!   spec's table gates `Data type` and `Data unit type` on "R2007+" and
//!   lists `Format string` after them with no gate of its own. `arc_2004.dwg`
//!   handle 101 spends 220 bits on the header and 3 x 543 on the rows,
//!   which is its whole 1849-bit budget with no format string; `arc_2007.dwg`
//!   handle 101 spends 138 + 3 x 579, the 36 further bits per row being
//!   `BL 512`, `BL 0` and a `TV` that costs no data-stream bits.
//! - **Every colour is a `TC` on R2000 too.** `arc_2000.dwg` handle 101 has
//!   the same 1849-bit budget as R2004's, which only the 44-bit `TC` form
//!   (`BS`, `BL`, `RC`) can fill; read as the one-`BS` `CMC` of R2000 the
//!   fields after the first colour land on nonsense (`BS 2054`, `BD` with
//!   the reserved `11` tag). The colours therefore always read as `TC`,
//!   whatever the release.
//!
//! # Releases
//!
//! R14 to R2007 follow the layout above and close on every corpus record.
//! R2010 and later store §20.4.101.2's different layout, decoded by
//! `decode_object_modern` into [`AcadTableStyleModern`]: the table's own
//! cell style and the cell styles of the title, header and data rows, in the
//! shape `crate::objects::table_cell_style` reads.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::color::{self, ObjectColor};
use crate::objects::modern;
use crate::objects::table_cell_style::{NamedCellStyle, read_named_cell_style};
use crate::string_stream::StringReader;
use crate::version::Version;

/// One of a row style's six borders.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableStyleBorder {
    /// `BS` line weight, DXF 274 to 279; -2 is AutoCAD's "by block".
    pub line_weight: i16,
    /// `B` visible, DXF 284 to 289.
    pub visible: bool,
    /// `TC` border colour, DXF 64 to 69.
    pub color: ObjectColor,
}

/// A row style: the data, title or header row of the table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableStyleRow {
    /// `BD` text height, DXF 140.
    pub text_height: f64,
    /// `BS` text alignment, DXF 170: 1 top left through 9 bottom right.
    pub text_alignment: i16,
    /// `TC` text colour, DXF 62.
    pub text_color: ObjectColor,
    /// `TC` fill colour, DXF 63.
    pub fill_color: ObjectColor,
    /// `B` background colour enabled, DXF 283.
    pub background_enabled: bool,
    /// The six borders: top, horizontal inside, bottom, left, vertical
    /// inside, right.
    pub borders: [TableStyleBorder; 6],
    /// `BL` data type, DXF 90; R2007+ only.
    pub data_type: Option<i32>,
    /// `BL` data unit type, DXF 91; R2007+ only.
    pub data_unit_type: Option<i32>,
    /// `TV` format string, DXF 1; R2007+ only.
    pub format_string: Option<String>,
}

/// A decoded TABLESTYLE in the §20.4.101.1 layout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AcadTableStyle {
    /// `TV` description, DXF 3.
    pub description: String,
    /// `BS` flow direction, DXF 70: 0 down, 1 up.
    pub flow_direction: i16,
    /// `BS` bit flags, DXF 71, "meaning unknown" in the spec.
    pub bit_flags: i16,
    /// `BD` horizontal cell margin, DXF 40.
    pub horizontal_margin: f64,
    /// `BD` vertical cell margin, DXF 41.
    pub vertical_margin: f64,
    /// `B` suppress title, DXF 280.
    pub suppress_title: bool,
    /// `B` suppress header, DXF 281.
    pub suppress_header: bool,
    /// The data row style (`_DATA`).
    pub data: TableStyleRow,
    /// The title row style (`_TITLE`).
    pub title: TableStyleRow,
    /// The header row style (`_HEADER`).
    pub header: TableStyleRow,
}

fn read_border(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<TableStyleBorder> {
    Ok(TableStyleBorder {
        line_weight: c.read_bs()?,
        visible: c.read_b()?,
        color: color::read_tc(c, strings, version)?,
    })
}

fn read_row(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<TableStyleRow> {
    let text_height = c.read_bd()?;
    let text_alignment = c.read_bs()?;
    let text_color = color::read_tc(c, strings, version)?;
    let fill_color = color::read_tc(c, strings, version)?;
    let background_enabled = c.read_b()?;
    let mut borders: [TableStyleBorder; 6] = Default::default();
    for border in &mut borders {
        *border = read_border(c, strings, version)?;
    }
    let (data_type, data_unit_type, format_string) = if version.is_r2007_plus() {
        (
            Some(c.read_bl()?),
            Some(c.read_bl()?),
            Some(modern::read_tv(c, strings, version)?),
        )
    } else {
        (None, None, None)
    };
    Ok(TableStyleRow {
        text_height,
        text_alignment,
        text_color,
        fill_color,
        background_enabled,
        borders,
        data_type,
        data_unit_type,
        format_string,
    })
}

fn read_body(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<AcadTableStyle> {
    Ok(AcadTableStyle {
        description: modern::read_tv(c, strings, version)?,
        flow_direction: c.read_bs()?,
        bit_flags: c.read_bs()?,
        horizontal_margin: c.read_bd()?,
        vertical_margin: c.read_bd()?,
        suppress_title: c.read_b()?,
        suppress_header: c.read_b()?,
        data: read_row(c, strings, version)?,
        title: read_row(c, strings, version)?,
        header: read_row(c, strings, version)?,
    })
}

/// Decode a TABLESTYLE straight from its raw object payload (§20.4.101.1),
/// taking the description and format strings from the R2007 string stream
/// when the file has one and checking the data fields end exactly on the
/// data-stream boundary. R2010 and later use §20.4.101.2's layout, read by
/// [`decode_object_modern`].
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadTableStyle> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings, version)?;
    split.finish("TABLESTYLE")?;
    Ok(out)
}

/// Most cell styles a TABLESTYLE of the R2010 layout claims.
const MAX_CELL_STYLES: u32 = 4096;

/// A cell style of the R2010 TABLESTYLE, with the `BL` that leads it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableStyleCellStyle {
    /// `BL` before the cell style; it equals the cell style id after it on
    /// every corpus record.
    pub leading_id: u32,
    /// The cell style with its id, class and name.
    pub style: NamedCellStyle,
}

/// A decoded TABLESTYLE in the R2010 layout of §20.4.101.2.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AcadTableStyleModern {
    /// `RC` unknown, 0 on every corpus record.
    pub unknown_byte: u8,
    /// `TV` description, DXF 3.
    pub description: String,
    /// `BL` unknown, 0 on every corpus record.
    pub unknown_a: u32,
    /// `BL` unknown, 101 on every corpus record: the first custom cell
    /// style id.
    pub unknown_b: u32,
    /// The cell style named "Table", id 4.
    pub table: NamedCellStyle,
    /// The cell styles of the title, header and data rows, in that order.
    pub cell_styles: Vec<TableStyleCellStyle>,
}

fn read_body_modern(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<AcadTableStyleModern> {
    let unknown_byte = c.read_rc()?;
    let description = modern::read_tv(c, strings, version)?;
    let unknown_a = c.read_bl_u()?;
    let unknown_b = c.read_bl_u()?;
    let table = read_named_cell_style(c, strings, version)?;
    let count = c.read_bl_u()?;
    if count > MAX_CELL_STYLES || count as usize > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "TABLESTYLE claims {count} cell styles in {} remaining bits",
            c.remaining_bits()
        )));
    }
    let mut cell_styles = Vec::with_capacity(count as usize);
    for _ in 0..count {
        cell_styles.push(TableStyleCellStyle {
            leading_id: c.read_bl_u()?,
            style: read_named_cell_style(c, strings, version)?,
        });
    }
    Ok(AcadTableStyleModern {
        unknown_byte,
        description,
        unknown_a,
        unknown_b,
        table,
        cell_styles,
    })
}

/// Decode a TABLESTYLE of the R2010 layout (§20.4.101.2) straight from its
/// raw object payload, taking every `TV` from the string stream and checking
/// that the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object_modern(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadTableStyleModern> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body_modern(&mut split.data, &mut split.strings, version)?;
    split.finish("TABLESTYLE")?;
    Ok(out)
}
