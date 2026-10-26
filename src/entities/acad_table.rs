//! ACAD_TABLE (§20.4.96) — a table entity, in the layout R2010 and later
//! write (§20.4.96.2).
//!
//! `ACAD_TABLE` is a custom class whose object type code is assigned per file
//! through `AcDb:Classes`. The entity inherits from INSERT, and in R2010 and
//! later its record is the INSERT fields, a few unknowns, the table content
//! of TABLECONTENT (§20.4.97) without its common object data, and the table's
//! break data:
//!
//! ```text
//! ..    INSERT fields: 3BD insertion point, BB data flags and the scale,
//!       BD rotation, 3BD extrusion, B has ATTRIBs, BL owned object count
//!       (only when there are ATTRIBs)
//! RC    unknown, 0
//! H     unknown, soft pointer       handle stream, not read here
//! BL    unknown, 0
//! B     unknown, R2010 only
//! BL    unknown, R2013 and later
//! ..    the table content (§20.4.97)
//! BS    unknown, 38 by default
//! 3BD   horizontal direction        DXF 11, 21, 31
//! BL    has break data              0 or 1
//! if 1:
//!   BL    option flags              1 enable breaks, 2 repeat top labels,
//!                                   4 repeat bottom labels, 8 allow manual
//!                                   positions, 16 allow manual heights
//!   BL    flow direction            1 right, 2 vertical, 4 left
//!   BD    break spacing
//!   BL    unknown flags
//!   BL    unknown flags
//!   BL    number of manual positions
//!   per manual position: 3BD position, BD height, BL flags
//! BL    number of break row ranges  at least 1
//! per row range: 3BD position, BL start row index, BL end row index
//! ```
//!
//! The spec's table for R2010+ does not repeat the INSERT fields it opens
//! with; the corpus's R2018 table (`sample_AC1032.dwg`, handle `0x528`) shows
//! them: the insertion point `(774.87, 48.54, 0)`, data flags `11` (unit
//! scale), rotation 0 and extrusion `(0, 0, 1)`, then the `RC`.
//!
//! R14 to R2007 write the older layout of §20.4.96.1 instead, read by
//! [`decode_legacy`] into [`AcadTableLegacy`]: the INSERT fields (three `BD`
//! scales on R14), a flag, the horizontal direction, the column and row counts,
//! the widths and heights, then per cell its type, edge flags, merge and
//! autofit flags, merged width and height, rotation, text or block data, its
//! property overrides (a flag word whose bits say which of 18 properties
//! follow) and, from R2007, a value; then the table's own overrides and three
//! groups of 18 border overrides (colour, line weight, visibility).
//!
//! Two readings are measured where §20.4.96.1 is silent. Every colour is the
//! 44-bit `TC` form on every release, as in TABLESTYLE: read as the one-`BS`
//! `CMC` of R14 and R2000 the first cell with a colour override leaves the
//! boundary (`sample_AC1015.dwg` handle `0x528`, cell 13, flags `0x6666F`). And
//! from R2007 a text cell stores no `TV` of its own: its text is the value
//! that follows, so reading one shifts every later string of the R2007 string
//! stream by a place, which the data-stream boundary cannot see and AutoCAD's
//! DXF export does (a cell's format and value strings).

use crate::bitcursor::BitCursor;
use crate::common_entity::CommonEntityData;
use crate::entities::Point3D;
use crate::entities::insert::Insert;
use crate::error::{Error, Result};
use crate::objects::color::{self, ObjectColor};
use crate::objects::modern;
use crate::objects::table_content::{self, Strings, TableContent, TableValue};
use crate::version::Version;

/// Most manual positions or row ranges a record may claim.
const MAX_COUNT: u32 = 1 << 20;

/// A manual break position.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableBreakPosition {
    /// `3BD` position.
    pub position: [f64; 3],
    /// `BD` height.
    pub height: f64,
    /// `BL` flags, meaning unknown.
    pub flags: u32,
}

/// The break data of a table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableBreakData {
    /// `BL` option flags.
    pub option_flags: u32,
    /// `BL` flow direction.
    pub flow_direction: u32,
    /// `BD` break spacing.
    pub break_spacing: f64,
    /// `BL` unknown flags.
    pub unknown_a: u32,
    /// `BL` unknown flags.
    pub unknown_b: u32,
    /// The manual positions.
    pub manual_positions: Vec<TableBreakPosition>,
}

/// A break row range.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableBreakRowRange {
    /// `3BD` position.
    pub position: [f64; 3],
    /// `BL` start row index.
    pub start_row: u32,
    /// `BL` end row index.
    pub end_row: u32,
}

/// A decoded ACAD_TABLE in the R2010 and later layout.
#[derive(Debug, Clone, PartialEq)]
pub struct AcadTable {
    /// The INSERT fields the entity opens with.
    pub insert: Insert,
    /// `RC` unknown, 0 on every corpus record.
    pub unknown_byte: u8,
    /// `BL` unknown, 0 on every corpus record.
    pub unknown_a: u32,
    /// `B` unknown, R2010 only.
    pub unknown_flag: Option<bool>,
    /// `BL` unknown, R2013 and later.
    pub unknown_b: Option<u32>,
    /// The table content.
    pub content: TableContent,
    /// `BS` unknown, 38 by default.
    pub unknown_short: i16,
    /// `3BD` horizontal direction, DXF 11, 21, 31.
    pub horizontal_direction: [f64; 3],
    /// The break data, when the table has some.
    pub break_data: Option<TableBreakData>,
    /// The break row ranges.
    pub break_row_ranges: Vec<TableBreakRowRange>,
}

fn read_3bd(c: &mut BitCursor<'_>) -> Result<[f64; 3]> {
    Ok([c.read_bd()?, c.read_bd()?, c.read_bd()?])
}

fn count(c: &mut BitCursor<'_>, what: &str) -> Result<usize> {
    let n = c.read_bl_u()?;
    if n > MAX_COUNT || n as usize > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "ACAD_TABLE claims {n} {what} in {} remaining bits",
            c.remaining_bits()
        )));
    }
    Ok(n as usize)
}

/// Decode an ACAD_TABLE entity's field list (§20.4.96.2) past the common
/// entity data. Releases before R2010 use §20.4.96.1's layout, read by
/// [`decode_legacy`].
pub fn decode(
    c: &mut BitCursor<'_>,
    version: Version,
    _common: &CommonEntityData,
    strings: Strings,
) -> Result<AcadTable> {
    let insert = read_head(c, version)?;
    let unknown_byte = c.read_rc()?;
    let unknown_a = c.read_bl_u()?;
    let unknown_flag = if matches!(version, Version::R2010) {
        Some(c.read_b()?)
    } else {
        None
    };
    let unknown_b = if matches!(version, Version::R2013 | Version::R2018) {
        Some(c.read_bl_u()?)
    } else {
        None
    };
    let content = table_content::read_content_body(c, strings, version)?;
    let unknown_short = c.read_bs()?;
    let horizontal_direction = read_3bd(c)?;
    let break_data = match c.read_bl_u()? {
        0 => None,
        1 => {
            let option_flags = c.read_bl_u()?;
            let flow_direction = c.read_bl_u()?;
            let break_spacing = c.read_bd()?;
            let unknown_a = c.read_bl_u()?;
            let unknown_b = c.read_bl_u()?;
            let n = count(c, "manual positions")?;
            let mut manual_positions = Vec::with_capacity(n);
            for _ in 0..n {
                manual_positions.push(TableBreakPosition {
                    position: read_3bd(c)?,
                    height: c.read_bd()?,
                    flags: c.read_bl_u()?,
                });
            }
            Some(TableBreakData {
                option_flags,
                flow_direction,
                break_spacing,
                unknown_a,
                unknown_b,
                manual_positions,
            })
        }
        other => {
            return Err(Error::SectionMap(format!(
                "ACAD_TABLE has break data flag {other}, not 0 or 1"
            )));
        }
    };
    let n = count(c, "break row ranges")?;
    let mut break_row_ranges = Vec::with_capacity(n);
    for _ in 0..n {
        break_row_ranges.push(TableBreakRowRange {
            position: read_3bd(c)?,
            start_row: c.read_bl_u()?,
            end_row: c.read_bl_u()?,
        });
    }
    Ok(AcadTable {
        insert,
        unknown_byte,
        unknown_a,
        unknown_flag,
        unknown_b,
        content,
        unknown_short,
        horizontal_direction,
        break_data,
        break_row_ranges,
    })
}

/// The value of one property override of a legacy table.
#[derive(Debug, Clone, PartialEq)]
pub enum TableOverrideValue {
    /// A `B`.
    Bool(bool),
    /// A `BS`.
    Short(i16),
    /// A `BD`.
    Real(f64),
    /// A `TC` colour, the 44-bit form on every release.
    Color(ObjectColor),
}

/// A set of property overrides: the flag word and, for each bit of it that
/// holds data in the data stream, the bit and its value.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableOverrides {
    /// The flag word, `BL`.
    pub flags: u32,
    /// The overridden properties in stream order, each with the flag bit that
    /// introduced it. Bits that name a handle hold no data stream bits and do
    /// not appear.
    pub values: Vec<(u32, TableOverrideValue)>,
}

/// One cell of a legacy table (§20.4.96.1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LegacyTableCell {
    /// `BS` cell type, DXF 171: 1 text, 2 block.
    pub cell_type: i16,
    /// `RC` cell edge flags, DXF 172.
    pub edge_flags: u8,
    /// `B` merged, DXF 173.
    pub merged: bool,
    /// `B` autofit, DXF 174.
    pub autofit: bool,
    /// `BL` merged width, DXF 175.
    pub merged_width: u32,
    /// `BL` merged height, DXF 176.
    pub merged_height: u32,
    /// `BD` rotation, DXF 145.
    pub rotation: f64,
    /// `TV` text of a text cell, DXF 1; none from R2007, where the value
    /// holds the text.
    pub text: Option<String>,
    /// `BD` block scale of a block cell, DXF 144.
    pub block_scale: Option<f64>,
    /// The attribute definitions of a block cell: `BS` index and `TV` text.
    pub attribute_definitions: Vec<(i16, String)>,
    /// `BL` cell flag override, DXF 177, and the overrides, when the cell
    /// has any.
    pub overrides: Option<(u8, TableOverrides)>,
    /// `BL` unknown and the value that follows it, from R2007.
    pub value: Option<(u32, TableValue)>,
}

/// A decoded ACAD_TABLE in the layout of §20.4.96.1, R14 to R2007.
#[derive(Debug, Clone, PartialEq)]
pub struct AcadTableLegacy {
    /// The INSERT fields the entity opens with.
    pub insert: Insert,
    /// `BS` flag for table value, DXF 90.
    pub table_flags: i16,
    /// `3BD` horizontal direction, DXF 11, 21, 31.
    pub horizontal_direction: [f64; 3],
    /// `BD` column widths, DXF 142.
    pub column_widths: Vec<f64>,
    /// `BD` row heights, DXF 141.
    pub row_heights: Vec<f64>,
    /// The cells, row by row.
    pub cells: Vec<LegacyTableCell>,
    /// The table overrides, DXF 93, when there are any.
    pub table_overrides: Option<TableOverrides>,
    /// The border colour overrides, DXF 94, when there are any.
    pub border_color_overrides: Option<TableOverrides>,
    /// The border line weight overrides, DXF 95, when there are any.
    pub border_line_weight_overrides: Option<TableOverrides>,
    /// The border visibility overrides, DXF 96, when there are any.
    pub border_visibility_overrides: Option<TableOverrides>,
}

#[derive(Clone, Copy)]
enum Kind {
    Bool,
    Bs,
    Real,
    Color,
    Handle,
}

use Kind::{Bool, Bs, Color, Handle, Real};

/// §20.4.96.1's cell overrides, by flag bit, in stream order.
const CELL_OVERRIDES: [(u32, Kind); 18] = [
    (0x1, Bs),
    (0x2, Bool),
    (0x4, Color),
    (0x8, Color),
    (0x10, Handle),
    (0x20, Real),
    (0x40, Color),
    (0x400, Bs),
    (0x4000, Bs),
    (0x80, Color),
    (0x800, Bs),
    (0x8000, Bs),
    (0x100, Color),
    (0x1000, Bs),
    (0x10000, Bs),
    (0x200, Color),
    (0x2000, Bs),
    (0x20000, Bs),
];

/// §20.4.96.1's table overrides, by flag bit, in stream order. Bit 0x2, the
/// header suppression, is "always true (do not read any data for this)".
const TABLE_OVERRIDES: [(u32, Kind); 22] = [
    (0x1, Bool),
    (0x4, Bs),
    (0x8, Real),
    (0x10, Real),
    (0x20, Color),
    (0x40, Color),
    (0x80, Color),
    (0x100, Bool),
    (0x200, Bool),
    (0x400, Bool),
    (0x800, Color),
    (0x1000, Color),
    (0x2000, Color),
    (0x4000, Bs),
    (0x8000, Bs),
    (0x10000, Bs),
    (0x20000, Handle),
    (0x40000, Handle),
    (0x80000, Handle),
    (0x100000, Real),
    (0x200000, Real),
    (0x400000, Real),
];

const fn border_overrides(kind: Kind) -> [(u32, Kind); 18] {
    let mut out = [(0u32, kind); 18];
    let mut i = 0;
    while i < 18 {
        out[i] = (1u32 << i, kind);
        i += 1;
    }
    out
}

fn read_overrides(
    c: &mut BitCursor<'_>,
    strings: Strings,
    version: Version,
    flags: u32,
    layout: &[(u32, Kind)],
) -> Result<Vec<(u32, TableOverrideValue)>> {
    let mut values = Vec::new();
    for &(bit, kind) in layout {
        if flags & bit == 0 {
            continue;
        }
        let value = match kind {
            Kind::Bool => TableOverrideValue::Bool(c.read_b()?),
            Kind::Bs => TableOverrideValue::Short(c.read_bs()?),
            Kind::Real => TableOverrideValue::Real(c.read_bd()?),
            Kind::Color => TableOverrideValue::Color(color::read_tc(c, strings, version)?),
            Kind::Handle => continue,
        };
        values.push((bit, value));
    }
    Ok(values)
}

/// A `B` that says whether a flag word and its overrides follow.
fn read_optional_overrides(
    c: &mut BitCursor<'_>,
    strings: Strings,
    version: Version,
    layout: &[(u32, Kind)],
) -> Result<Option<TableOverrides>> {
    if !c.read_b()? {
        return Ok(None);
    }
    let flags = c.read_bl_u()?;
    let values = read_overrides(c, strings, version, flags, layout)?;
    Ok(Some(TableOverrides { flags, values }))
}

fn read_legacy_cell(
    c: &mut BitCursor<'_>,
    strings: Strings,
    version: Version,
) -> Result<LegacyTableCell> {
    let cell_type = c.read_bs()?;
    let edge_flags = c.read_rc()?;
    let merged = c.read_b()?;
    let autofit = c.read_b()?;
    let merged_width = c.read_bl_u()?;
    let merged_height = c.read_bl_u()?;
    let rotation = c.read_bd()?;
    let mut text = None;
    let mut block_scale = None;
    let mut attribute_definitions = Vec::new();
    match cell_type {
        1 if version.is_r2007_plus() => {}
        1 => text = Some(modern::read_tv(c, strings, version)?),
        2 => {
            block_scale = Some(c.read_bd()?);
            if c.read_b()? {
                let n = count(c, "attribute definitions")?;
                for _ in 0..n {
                    attribute_definitions
                        .push((c.read_bs()?, modern::read_tv(c, strings, version)?));
                }
            }
        }
        other => {
            return Err(Error::Unsupported {
                feature: format!("ACAD_TABLE cell of type {other}"),
            });
        }
    }
    let overrides = if c.read_b()? {
        let flags = c.read_bl_u()?;
        let virtual_edges = c.read_rc()?;
        let values = read_overrides(c, strings, version, flags, &CELL_OVERRIDES)?;
        Some((virtual_edges, TableOverrides { flags, values }))
    } else {
        None
    };
    let value = if version.is_r2007_plus() {
        Some((
            c.read_bl_u()?,
            table_content::read_value(c, strings, version)?,
        ))
    } else {
        None
    };
    Ok(LegacyTableCell {
        cell_type,
        edge_flags,
        merged,
        autofit,
        merged_width,
        merged_height,
        rotation,
        text,
        block_scale,
        attribute_definitions,
        overrides,
        value,
    })
}

/// The INSERT fields every table opens with, as §20.4.96.1 prints them: three
/// `BD` scales on R13 and R14, `BB` data flags and the scale from R2000.
fn read_head(c: &mut BitCursor<'_>, version: Version) -> Result<Insert> {
    let insertion_point = read_3bd(c)?;
    let scale = if matches!(version, Version::R14) {
        [c.read_bd()?, c.read_bd()?, c.read_bd()?]
    } else {
        match c.read_bb()? {
            0b11 => [1.0, 1.0, 1.0],
            0b01 => [1.0, c.read_dd(1.0)?, c.read_dd(1.0)?],
            0b10 => {
                let x = c.read_rd()?;
                [x, x, x]
            }
            _ => {
                let x = c.read_rd()?;
                [x, c.read_dd(x)?, c.read_dd(x)?]
            }
        }
    };
    let rotation = c.read_bd()?;
    let extrusion = read_3bd(c)?;
    let has_attribs = c.read_b()?;
    let owned_object_count = if has_attribs && version.is_r2004_plus() {
        c.read_bl_u()?
    } else {
        0
    };
    Ok(Insert {
        insertion_point: point(insertion_point),
        scale: point(scale),
        rotation,
        extrusion: point(extrusion),
        has_attribs,
        owned_object_count,
    })
}

fn point(v: [f64; 3]) -> Point3D {
    Point3D {
        x: v[0],
        y: v[1],
        z: v[2],
    }
}

/// Decode an ACAD_TABLE entity's field list in the layout of §20.4.96.1
/// (R14 to R2007) past the common entity data.
pub fn decode_legacy(
    c: &mut BitCursor<'_>,
    version: Version,
    _common: &CommonEntityData,
    strings: Strings,
) -> Result<AcadTableLegacy> {
    let insert = read_head(c, version)?;
    let table_flags = c.read_bs()?;
    let horizontal_direction = read_3bd(c)?;
    let columns = count(c, "columns")?;
    let rows = count(c, "rows")?;
    let mut column_widths = Vec::with_capacity(columns);
    for _ in 0..columns {
        column_widths.push(c.read_bd()?);
    }
    let mut row_heights = Vec::with_capacity(rows);
    for _ in 0..rows {
        row_heights.push(c.read_bd()?);
    }
    let total = columns * rows;
    if total > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "ACAD_TABLE claims {total} cells in {} remaining bits",
            c.remaining_bits()
        )));
    }
    let mut cells = Vec::with_capacity(total);
    for _ in 0..total {
        cells.push(read_legacy_cell(c, strings, version)?);
    }
    let table_overrides = read_optional_overrides(c, strings, version, &TABLE_OVERRIDES)?;
    let border_color_overrides =
        read_optional_overrides(c, strings, version, &border_overrides(Color))?;
    let border_line_weight_overrides =
        read_optional_overrides(c, strings, version, &border_overrides(Bs))?;
    let border_visibility_overrides =
        read_optional_overrides(c, strings, version, &border_overrides(Bs))?;
    Ok(AcadTableLegacy {
        insert,
        table_flags,
        horizontal_direction,
        column_widths,
        row_heights,
        cells,
        table_overrides,
        border_color_overrides,
        border_line_weight_overrides,
        border_visibility_overrides,
    })
}
