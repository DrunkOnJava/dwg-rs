//! TABLECONTENT (§20.4.97) — the cells of a table and what they hold.
//!
//! The class is `AcDbTableContent`, DXF name `TABLECONTENT`, a custom class
//! whose object type code is assigned per file through `AcDb:Classes`. It
//! replaces the table data structures of AutoCAD 2005 and is reached from a
//! table entity through its extension dictionary. The class inherits from
//! `AcDbFormattedTableData`, `AcDbLinkedTableData` and `AcDbLinkedData`, and
//! §20.4.97 prints all four as one data stream:
//!
//! ```text
//! TV    name                              DXF 1        AcDbLinkedData
//! TV    description                       DXF 300
//! BL    number of columns                 DXF 90       AcDbLinkedTableData
//! per column:
//!   TV    name                            DXF 300
//!   BL    custom data                     DXF 91
//!   ..    custom data collection
//!   ..    cell style (the column's overrides)
//!   BL    cell style id                   DXF 90
//!   BD    width                           DXF 40
//! BL    number of rows                    DXF 91
//! per row:
//!   BL    number of cells                 DXF 90
//!   per cell:
//!     BL    state flags                   DXF 90
//!     TV    tooltip                       DXF 300
//!     BL    custom data                   DXF 91
//!     ..    custom data collection
//!     BL    has linked data               DXF 92
//!     if linked: H (handle stream), BL 93, BL 94, BL 96
//!     BL    number of contents            DXF 95
//!     per content:
//!       BL    type                        DXF 90   1 value, 2 field, 4 block
//!       ..    a value, or H of a field or block (handle stream)
//!       BL    number of attributes        DXF 91
//!       per attribute: H (handle stream), TV 301, BL 92
//!       BS    content format overrides    DXF 170
//!       ..    the content format, when the flag is not 0
//!     ..    cell style (the cell's overrides)
//!     BL    cell style id                 DXF 90
//!     BL    unknown flag                  DXF 91
//!     if the flag is not 0: BL, BD, BD, BL geometry flags, H, and when the
//!       geometry flags are not 0 the content geometry (TABLEGEOMETRY's)
//!   BL    custom data                     DXF 91
//!   ..    custom data collection
//!   ..    cell style (the row's overrides)
//!   BL    cell style id                   DXF 90
//!   BD    height                          DXF 40
//! BL    number of field references        H each, in the handle stream
//! ..    cell style (the table's overrides)  AcDbFormattedTableData
//! BL    number of merged cell ranges      DXF 90
//! per range: BL top row, BL left column, BL bottom row, BL right column
//! H     table style                       AcDbTableContent, handle stream
//! ```
//!
//! A custom data collection is a `BL` count and per item a `TV` name and a
//! value; a value (§20.4.99) is, from R2007, `BL` flags, `BL` data type, the
//! data, `BL` unit type, `TV` format string and `TV` value string, and before
//! R2007 only the data type and the data. The data types are 0 unknown (`BL`),
//! 1 long (`BL`), 2 double (`BD`), 4 string (a `TV` before R2007, from R2007 a
//! `BL` byte count and that many bytes of UTF-16LE in the data stream), 8 date,
//! 16 point and 32 3D point (a `BL` size, then the bytes, a `2RD` or a `3RD`),
//! 64 object id (a handle), 512 general (a `BL` byte count, then the bytes).
//! Data types 128 and 256 the spec does not describe and are refused. From
//! R2007 a value whose flags have bit 1 set, the spec's "kGeneral", still has
//! its data type but no data: the corpus's empty cells carry flags 7, a data
//! type of 0 and nothing between the type and the unit type.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::objects::table_cell_style::{
    TableCellStyle, TableContentFormat, read_cell_style, read_content_format,
};
use crate::string_stream::StringReader;
use crate::version::Version;

/// Most columns, rows, cells, items or ranges a record may claim.
const MAX_COUNT: u32 = 1 << 20;

/// The data of a value (§20.4.99).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum TableValueData {
    /// From R2007, a value whose flags have bit 1 set carries no data.
    #[default]
    Absent,
    /// Data type 0: a `BL`.
    Unknown(u32),
    /// Data type 1: a `BL`.
    Long(u32),
    /// Data type 2: a `BD`.
    Double(f64),
    /// Data type 4: a `TV`.
    String(String),
    /// Data type 8: a `BL` size then that many bytes.
    Date(Vec<u8>),
    /// Data type 16: a `BL` size then a `2RD`.
    Point2d([f64; 2]),
    /// Data type 32: a `BL` size then a `3RD`.
    Point3d([f64; 3]),
    /// Data type 64: an object id, a handle of the handle stream.
    ObjectId,
    /// Data type 512: a `BL` byte count then the bytes.
    General(Vec<u8>),
}

/// A value (§20.4.99).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableValue {
    /// `BL` flags, DXF 93; R2007+ only.
    pub flags: Option<u32>,
    /// `BL` data type, DXF 90.
    pub data_type: u32,
    /// The data.
    pub data: TableValueData,
    /// `BL` unit type, DXF 94; R2007+ only.
    pub unit_type: Option<u32>,
    /// `TV` format string, DXF 300; R2007+ only.
    pub format_string: Option<String>,
    /// `TV` value string, DXF 302; R2007+ only.
    pub value_string: Option<String>,
}

/// One item of a custom data collection (§20.4.100).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCustomDataItem {
    /// `TV` item name, DXF 300.
    pub name: String,
    /// The item's value.
    pub value: TableValue,
}

/// The `BL` custom data and the collection that follows it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCustomData {
    /// `BL` custom data, DXF 91.
    pub flags: u32,
    /// The collection's items.
    pub items: Vec<TableCustomDataItem>,
}

/// A column of the table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableContentColumn {
    /// `TV` name, DXF 300.
    pub name: String,
    /// The column's custom data.
    pub custom_data: TableCustomData,
    /// The column's cell style overrides.
    pub cell_style: TableCellStyle,
    /// `BL` cell style id, DXF 90; 0 when none.
    pub cell_style_id: u32,
    /// `BD` width, DXF 40.
    pub width: f64,
}

/// The linked data of a cell that has some.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableLinkedData {
    /// `BL` row count, DXF 93.
    pub rows: u32,
    /// `BL` column count, DXF 94.
    pub columns: u32,
    /// `BL` unknown, DXF 96.
    pub unknown: u32,
}

/// What a cell content holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum TableCellContentKind {
    /// Type 0.
    #[default]
    Unknown,
    /// Type 1: a value.
    Value(TableValue),
    /// Type 2: a field, a handle of the handle stream.
    Field,
    /// Type 4: a block, a handle of the handle stream.
    Block,
}

/// An attribute of a block content.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableContentAttribute {
    /// `TV` attribute value, DXF 301.
    pub value: String,
    /// `BL` index, DXF 92; starts at 1.
    pub index: u32,
}

/// One content of a cell.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCellContent {
    /// `BL` content type, DXF 90.
    pub content_type: u32,
    /// What the content holds.
    pub kind: TableCellContentKind,
    /// The attributes.
    pub attributes: Vec<TableContentAttribute>,
    /// `BS` content format overrides flag, DXF 170.
    pub format_override_flag: i16,
    /// The content format overrides, when the flag is not 0.
    pub format: Option<TableContentFormat>,
}

/// The geometry a cell carries when its unknown flag is not 0.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCellExtra {
    /// `BL` unknown, DXF 91.
    pub unknown_a: u32,
    /// `BD` unknown, DXF 40.
    pub unknown_b: f64,
    /// `BD` unknown, DXF 41.
    pub unknown_c: f64,
    /// `BL` geometry data flags.
    pub geometry_flags: u32,
    /// The content geometry, when the geometry flags are not 0 (§20.4.98).
    pub geometry: Option<TableCellGeometry>,
}

/// The cell content geometry of §20.4.98.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCellGeometry {
    /// `3BD` distance to top left.
    pub distance_to_top_left: [f64; 3],
    /// `3BD` distance to center.
    pub distance_to_center: [f64; 3],
    /// `BD` content width.
    pub content_width: f64,
    /// `BD` content height.
    pub content_height: f64,
    /// `BD` width.
    pub width: f64,
    /// `BD` height.
    pub height: f64,
    /// `BL` unknown flags.
    pub unknown_flags: u32,
}

/// A cell of the table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableContentCell {
    /// `BL` state flags, DXF 90.
    pub state_flags: u32,
    /// `TV` tooltip, DXF 300.
    pub tooltip: String,
    /// The cell's custom data.
    pub custom_data: TableCustomData,
    /// `BL` has linked data, DXF 92, with the data when it is 1.
    pub linked_data: Option<TableLinkedData>,
    /// The cell's contents.
    pub contents: Vec<TableCellContent>,
    /// The cell's cell style overrides.
    pub cell_style: TableCellStyle,
    /// `BL` cell style id, DXF 90; 0 when none.
    pub cell_style_id: u32,
    /// `BL` unknown flag, DXF 91, as `Some` of what follows it when not 0.
    pub extra: Option<TableCellExtra>,
}

/// A row of the table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableContentRow {
    /// The row's cells.
    pub cells: Vec<TableContentCell>,
    /// The row's custom data.
    pub custom_data: TableCustomData,
    /// The row's cell style overrides.
    pub cell_style: TableCellStyle,
    /// `BL` cell style id, DXF 90; 0 when none.
    pub cell_style_id: u32,
    /// `BD` height, DXF 40.
    pub height: f64,
}

/// A merged range of cells.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableMergedRange {
    /// `BL` top row index, DXF 91.
    pub top_row: u32,
    /// `BL` left column index, DXF 92.
    pub left_column: u32,
    /// `BL` bottom row index, DXF 93.
    pub bottom_row: u32,
    /// `BL` right column index, DXF 94.
    pub right_column: u32,
}

/// A decoded TABLECONTENT.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableContent {
    /// `TV` name, DXF 1.
    pub name: String,
    /// `TV` description, DXF 300.
    pub description: String,
    /// The columns.
    pub columns: Vec<TableContentColumn>,
    /// The rows.
    pub rows: Vec<TableContentRow>,
    /// `BL` number of cell contents with a field reference.
    pub field_reference_count: u32,
    /// The table's cell style overrides.
    pub cell_style: TableCellStyle,
    /// The merged cell ranges.
    pub merged_ranges: Vec<TableMergedRange>,
}

type Strings<'a, 'b> = &'a mut Option<StringReader<'b>>;

fn count(c: &mut BitCursor<'_>, what: &str) -> Result<usize> {
    let n = c.read_bl_u()?;
    if n > MAX_COUNT || n as usize > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "TABLECONTENT claims {n} {what} in {} remaining bits",
            c.remaining_bits()
        )));
    }
    Ok(n as usize)
}

/// A string value from R2007: a `BL` byte count and that many bytes of
/// UTF-16LE, in the data stream (the `TV`s of the same value are in the string
/// stream), the count including the terminating zero.
fn utf16_value(c: &mut BitCursor<'_>) -> Result<String> {
    let raw = bytes(c)?;
    let mut units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    if units.last() == Some(&0) {
        units.pop();
    }
    String::from_utf16(&units)
        .map_err(|_| Error::SectionMap("TABLECONTENT string value is not valid UTF-16".into()))
}

fn bytes(c: &mut BitCursor<'_>) -> Result<Vec<u8>> {
    let n = c.read_bl_u()? as usize;
    if n > c.remaining_bits() / 8 {
        return Err(Error::SectionMap(format!(
            "TABLECONTENT value claims {n} bytes in {} remaining bits",
            c.remaining_bits()
        )));
    }
    (0..n).map(|_| c.read_rc()).collect()
}

fn read_value(c: &mut BitCursor<'_>, strings: Strings, version: Version) -> Result<TableValue> {
    let modern_value = version.is_r2007_plus();
    let flags = if modern_value {
        Some(c.read_bl_u()?)
    } else {
        None
    };
    let data_type = c.read_bl_u()?;
    let has_data = flags.is_none_or(|f| f & 1 == 0);
    let data = match data_type {
        _ if !has_data => TableValueData::Absent,
        0 => TableValueData::Unknown(c.read_bl_u()?),
        1 => TableValueData::Long(c.read_bl_u()?),
        2 => TableValueData::Double(c.read_bd()?),
        4 if modern_value => TableValueData::String(utf16_value(c)?),
        4 => TableValueData::String(modern::read_tv(c, strings, version)?),
        8 => TableValueData::Date(bytes(c)?),
        16 => {
            c.read_bl_u()?;
            TableValueData::Point2d([c.read_rd()?, c.read_rd()?])
        }
        32 => {
            c.read_bl_u()?;
            TableValueData::Point3d([c.read_rd()?, c.read_rd()?, c.read_rd()?])
        }
        64 => TableValueData::ObjectId,
        512 => TableValueData::General(bytes(c)?),
        other => {
            return Err(Error::Unsupported {
                feature: format!("TABLECONTENT value of data type {other}"),
            });
        }
    };
    let (unit_type, format_string, value_string) = if modern_value {
        (
            Some(c.read_bl_u()?),
            Some(modern::read_tv(c, strings, version)?),
            Some(modern::read_tv(c, strings, version)?),
        )
    } else {
        (None, None, None)
    };
    Ok(TableValue {
        flags,
        data_type,
        data,
        unit_type,
        format_string,
        value_string,
    })
}

fn read_custom_data(
    c: &mut BitCursor<'_>,
    strings: Strings,
    version: Version,
) -> Result<TableCustomData> {
    let flags = c.read_bl_u()?;
    let n = count(c, "custom data items")?;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        items.push(TableCustomDataItem {
            name: modern::read_tv(c, strings, version)?,
            value: read_value(c, strings, version)?,
        });
    }
    Ok(TableCustomData { flags, items })
}

fn read_column(
    c: &mut BitCursor<'_>,
    strings: Strings,
    version: Version,
) -> Result<TableContentColumn> {
    Ok(TableContentColumn {
        name: modern::read_tv(c, strings, version)?,
        custom_data: read_custom_data(c, strings, version)?,
        cell_style: read_cell_style(c, strings, version)?,
        cell_style_id: c.read_bl_u()?,
        width: c.read_bd()?,
    })
}

fn read_3bd(c: &mut BitCursor<'_>) -> Result<[f64; 3]> {
    Ok([c.read_bd()?, c.read_bd()?, c.read_bd()?])
}

fn read_geometry(c: &mut BitCursor<'_>) -> Result<TableCellGeometry> {
    Ok(TableCellGeometry {
        distance_to_top_left: read_3bd(c)?,
        distance_to_center: read_3bd(c)?,
        content_width: c.read_bd()?,
        content_height: c.read_bd()?,
        width: c.read_bd()?,
        height: c.read_bd()?,
        unknown_flags: c.read_bl_u()?,
    })
}

fn read_content(
    c: &mut BitCursor<'_>,
    strings: Strings,
    version: Version,
) -> Result<TableCellContent> {
    let content_type = c.read_bl_u()?;
    let kind = match content_type {
        0 => TableCellContentKind::Unknown,
        1 => TableCellContentKind::Value(read_value(c, strings, version)?),
        2 => TableCellContentKind::Field,
        4 => TableCellContentKind::Block,
        other => {
            return Err(Error::Unsupported {
                feature: format!("TABLECONTENT cell content of type {other}"),
            });
        }
    };
    let n = count(c, "attributes")?;
    let mut attributes = Vec::with_capacity(n);
    for _ in 0..n {
        attributes.push(TableContentAttribute {
            value: modern::read_tv(c, strings, version)?,
            index: c.read_bl_u()?,
        });
    }
    let format_override_flag = c.read_bs()?;
    let format = if format_override_flag != 0 {
        Some(read_content_format(c, strings, version)?)
    } else {
        None
    };
    Ok(TableCellContent {
        content_type,
        kind,
        attributes,
        format_override_flag,
        format,
    })
}

fn read_cell(
    c: &mut BitCursor<'_>,
    strings: Strings,
    version: Version,
) -> Result<TableContentCell> {
    let state_flags = c.read_bl_u()?;
    let tooltip = modern::read_tv(c, strings, version)?;
    let custom_data = read_custom_data(c, strings, version)?;
    let linked_data = match c.read_bl_u()? {
        0 => None,
        1 => Some(TableLinkedData {
            rows: c.read_bl_u()?,
            columns: c.read_bl_u()?,
            unknown: c.read_bl_u()?,
        }),
        other => {
            return Err(Error::SectionMap(format!(
                "TABLECONTENT cell has linked data flag {other}, not 0 or 1"
            )));
        }
    };
    let n = count(c, "cell contents")?;
    let mut contents = Vec::with_capacity(n);
    for _ in 0..n {
        contents.push(read_content(c, strings, version)?);
    }
    let cell_style = read_cell_style(c, strings, version)?;
    let cell_style_id = c.read_bl_u()?;
    let extra = if c.read_bl_u()? != 0 {
        let unknown_a = c.read_bl_u()?;
        let unknown_b = c.read_bd()?;
        let unknown_c = c.read_bd()?;
        let geometry_flags = c.read_bl_u()?;
        let geometry = if geometry_flags != 0 {
            Some(read_geometry(c)?)
        } else {
            None
        };
        Some(TableCellExtra {
            unknown_a,
            unknown_b,
            unknown_c,
            geometry_flags,
            geometry,
        })
    } else {
        None
    };
    Ok(TableContentCell {
        state_flags,
        tooltip,
        custom_data,
        linked_data,
        contents,
        cell_style,
        cell_style_id,
        extra,
    })
}

fn read_row(c: &mut BitCursor<'_>, strings: Strings, version: Version) -> Result<TableContentRow> {
    let n = count(c, "cells")?;
    let mut cells = Vec::with_capacity(n);
    for _ in 0..n {
        cells.push(read_cell(c, strings, version)?);
    }
    Ok(TableContentRow {
        cells,
        custom_data: read_custom_data(c, strings, version)?,
        cell_style: read_cell_style(c, strings, version)?,
        cell_style_id: c.read_bl_u()?,
        height: c.read_bd()?,
    })
}

fn read_body(c: &mut BitCursor<'_>, strings: Strings, version: Version) -> Result<TableContent> {
    let name = modern::read_tv(c, strings, version)?;
    let description = modern::read_tv(c, strings, version)?;
    let n = count(c, "columns")?;
    let mut columns = Vec::with_capacity(n);
    for _ in 0..n {
        columns.push(read_column(c, strings, version)?);
    }
    let n = count(c, "rows")?;
    let mut rows = Vec::with_capacity(n);
    for _ in 0..n {
        rows.push(read_row(c, strings, version)?);
    }
    let field_reference_count = c.read_bl_u()?;
    let cell_style = read_cell_style(c, strings, version)?;
    let n = count(c, "merged cell ranges")?;
    let mut merged_ranges = Vec::with_capacity(n);
    for _ in 0..n {
        merged_ranges.push(TableMergedRange {
            top_row: c.read_bl_u()?,
            left_column: c.read_bl_u()?,
            bottom_row: c.read_bl_u()?,
            right_column: c.read_bl_u()?,
        });
    }
    Ok(TableContent {
        name,
        description,
        columns,
        rows,
        field_reference_count,
        cell_style,
        merged_ranges,
    })
}

/// Decode a TABLECONTENT (§20.4.97) straight from its raw object payload,
/// taking every `TV` from the string stream when the file has one and
/// checking the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<TableContent> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings, version)?;
    split.finish("TABLECONTENT")?;
    Ok(out)
}
