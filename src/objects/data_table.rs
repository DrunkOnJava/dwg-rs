//! DATATABLE (`AcDbDataTable`): a table of typed columns, as AutoCAD writes it
//! for the data a drawing's fields and links draw on.
//!
//! ODA spec v5.4.1 prints no layout for it. The field list is derived from
//! AutoCAD's own output under the method in `CLEANROOM.md` (2026-10-26): the data
//! bits of the six corpus records (R14, R2000 and R2004; two per release) against
//! the DXF object of the R2004 pair. ARCHITECTURE.md §7bm holds the evidence.
//!
//! ```text
//! BS  version                   (group 70: 2)
//! BL  column count              (group 90: 4)
//! BL  row count                 (group 91: 21 and 20)
//! TV  table name                (group 1, empty in the corpus)
//! for each column:
//!   BS  column type             (group 92)
//!   TV  column name             (group 2, empty in the corpus)
//!   row count cells of the type:
//!     1  BL  (group 93)
//!     2  BD  (group 40)
//!     6  an object handle (group 360): in the handle stream, so no data bits
//! ```
//!
//! The cells of a handle column and the handle stream they sit in are not decoded.
//! A column type other than 1, 2 and 6, which the corpus does not hold, is refused
//! with [`crate::error::Error::Unsupported`] rather than read on a guess, and every
//! count is capped by the bits left in the record.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::objects::small_classes::open_and_finish;
use crate::version::Version;

/// The cells of one column, by its type (DXF group 92).
#[derive(Debug, Clone, PartialEq)]
pub enum DataTableCells {
    /// Type 1: one `BL` per row, DXF group 93.
    Long(Vec<i32>),
    /// Type 2: one `BD` per row, DXF group 40.
    Double(Vec<f64>),
    /// Type 6: one handle per row (DXF group 360), in the handle stream and not decoded.
    Handle,
}

/// One column of a DATATABLE.
#[derive(Debug, Clone, PartialEq)]
pub struct DataTableColumn {
    /// `BS`, DXF group 92.
    pub dxf_92: i16,
    /// `TV`, DXF group 2.
    pub dxf_2: String,
    pub cells: DataTableCells,
}

/// A decoded DATATABLE record.
#[derive(Debug, Clone, PartialEq)]
pub struct DataTable {
    /// `BS`, DXF group 70 (2).
    pub dxf_70: i16,
    /// `BL`, DXF group 91.
    pub rows: u32,
    /// `TV`, DXF group 1.
    pub dxf_1: String,
    /// The columns, in order; their count is DXF group 90.
    pub columns: Vec<DataTableColumn>,
}

fn capped(count: u32, what: &str, bits_each: usize, c: &BitCursor<'_>) -> Result<usize> {
    let count = count as usize;
    if count > c.remaining_bits() / bits_each {
        return Err(Error::SectionMap(format!(
            "DATATABLE {what} {count} exceeds the {} bits left in the record",
            c.remaining_bits()
        )));
    }
    Ok(count)
}

/// Decode a DATATABLE record straight from its raw object payload, from the
/// layout of the module docs (derived from AutoCAD's output, ARCHITECTURE.md §7bm),
/// ending exactly on the data-stream boundary and, from R2007, with the string
/// stream consumed.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<DataTable> {
    open_and_finish(
        "DATATABLE",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            let dxf_70 = c.read_bs()?;
            let column_count = c.read_bl_u()?;
            let rows = c.read_bl_u()?;
            let dxf_1 = modern::read_tv(c, strings, version)?;
            let column_count = capped(column_count, "column count", 4, c)?;
            let mut columns = Vec::with_capacity(column_count.min(1024));
            for _ in 0..column_count {
                let dxf_92 = c.read_bs()?;
                let dxf_2 = modern::read_tv(c, strings, version)?;
                let cells = match dxf_92 {
                    1 => {
                        let n = capped(rows, "row count", 2, c)?;
                        let mut v = Vec::with_capacity(n.min(1024));
                        for _ in 0..n {
                            v.push(c.read_bl()?);
                        }
                        DataTableCells::Long(v)
                    }
                    2 => {
                        let n = capped(rows, "row count", 2, c)?;
                        let mut v = Vec::with_capacity(n.min(1024));
                        for _ in 0..n {
                            v.push(c.read_bd()?);
                        }
                        DataTableCells::Double(v)
                    }
                    6 => DataTableCells::Handle,
                    _ => {
                        return Err(Error::Unsupported {
                            feature: format!("DATATABLE column type {dxf_92}"),
                        });
                    }
                };
                columns.push(DataTableColumn {
                    dxf_92,
                    dxf_2,
                    cells,
                });
            }
            Ok(DataTable {
                dxf_70,
                rows,
                dxf_1,
                columns,
            })
        },
    )
}
