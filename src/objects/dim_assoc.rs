//! DIMASSOC (`AcDbDimAssoc`): the associativity record of a dimension.
//!
//! ODA spec v5.4.1 prints no layout for it. The field list is derived from
//! AutoCAD's own output under the method in `CLEANROOM.md` (2026-10-26): the
//! data bits of the 21 corpus records (`sample_base` saved as R14 to R2018)
//! against the DXF object of the same handle. ARCHITECTURE.md §7bi holds the
//! evidence.
//!
//! ```text
//! BL  flags                         (group 90: 2 or 3)
//! RC  and B  after it               (groups 70 and 71, both 0)
//! for each bit set in the low nibble of the flags:
//!   TV  reference class name        (group 1, "AcDbOsnapPointRef")
//!   RC  osnap mode                  (group 72: 3, 13, 1)
//!   BS  unknown                     (1 in every record)
//!   BS  group 73                    (2)
//!   BL  group 91                    (0)
//!   BD  unknown                     (0.0)
//!   BD  group 40                    (0.0, 0.9999999999999998)
//!   BD  groups 10 and 20            (0.0 and 0.0, 5.0 and 5.0)
//!   BS  group 75                    (0)
//!   B   unknown                     (0)
//! ```
//!
//! The dimension handle (group 330) and each reference's object handle (group
//! 331) live in the handle stream and are not decoded. A reference class other
//! than `AcDbOsnapPointRef`, flag bits above the low nibble, and an integer wider
//! than the short `BS` forms are refused with
//! [`crate::error::Error::Unsupported`] rather than read on a guess.

use crate::error::Result;
use crate::objects::modern;
use crate::objects::small_classes::{open_and_finish, read_small, unsupported};
use crate::version::Version;

const OSNAP_POINT_REF: &str = "AcDbOsnapPointRef";

/// One reference of a DIMASSOC.
#[derive(Debug, Clone, PartialEq)]
pub struct OsnapPointRef {
    /// `RC` osnap mode, DXF group 72.
    pub dxf_72: u8,
    /// `BS` between group 72 and group 73, 1 in every record: unverified by
    /// value (the DXF shows a handle, group 331, at its place).
    pub unknown_a: u8,
    /// `BS`, DXF group 73.
    pub dxf_73: u8,
    /// `BL`, DXF group 91.
    pub dxf_91: u8,
    /// `BD` before group 40, 0.0 in every record: unverified by value.
    pub unknown_b: f64,
    /// `BD`, DXF group 40.
    pub dxf_40: f64,
    /// `BD`, DXF group 10 (the two coordinates are equal in the corpus, so
    /// their order is unverified).
    pub dxf_10: f64,
    /// `BD`, DXF group 20.
    pub dxf_20: f64,
    /// `BS`, DXF group 75.
    pub dxf_75: u8,
    /// `B` after group 75, 0 in every record: unverified by value.
    pub unknown_trailing: bool,
}

/// A decoded DIMASSOC record.
#[derive(Debug, Clone, PartialEq)]
pub struct DimAssoc {
    /// `BL` flags, DXF group 90; one reference per bit set in the low nibble.
    pub dxf_90: u32,
    /// `RC` after the flags, DXF groups 70 and 71 in an order the corpus
    /// cannot tell apart (both 0).
    pub dxf_70_71_byte: u8,
    /// `B` after that `RC`.
    pub dxf_70_71_bit: bool,
    pub references: Vec<OsnapPointRef>,
}

/// Decode a DIMASSOC straight from its raw object payload, from the layout of
/// the module docs (derived from AutoCAD's output, ARCHITECTURE.md §7bi), ending
/// exactly on the data-stream boundary and, from R2007, with the string stream
/// consumed.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<DimAssoc> {
    open_and_finish(
        "DIMASSOC",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            let dxf_90 = c.read_bl_u()?;
            if dxf_90 > 0xF {
                return Err(unsupported("DIMASSOC flags above the low nibble"));
            }
            let dxf_70_71_byte = c.read_rc()?;
            let dxf_70_71_bit = c.read_b()?;
            let mut references = Vec::new();
            for _ in 0..dxf_90.count_ones() {
                let class = modern::read_tv(c, strings, version)?;
                if class != OSNAP_POINT_REF {
                    return Err(unsupported("DIMASSOC reference class"));
                }
                let dxf_72 = c.read_rc()?;
                let unknown_a = read_small(c, "DIMASSOC reference field")?;
                let dxf_73 = read_small(c, "DIMASSOC group 73")?;
                let dxf_91 = read_small(c, "DIMASSOC group 91")?;
                let unknown_b = c.read_bd()?;
                let dxf_40 = c.read_bd()?;
                let dxf_10 = c.read_bd()?;
                let dxf_20 = c.read_bd()?;
                let dxf_75 = read_small(c, "DIMASSOC group 75")?;
                let unknown_trailing = c.read_b()?;
                references.push(OsnapPointRef {
                    dxf_72,
                    unknown_a,
                    dxf_73,
                    dxf_91,
                    unknown_b,
                    dxf_40,
                    dxf_10,
                    dxf_20,
                    dxf_75,
                    unknown_trailing,
                });
            }
            Ok(DimAssoc {
                dxf_90,
                dxf_70_71_byte,
                dxf_70_71_bit,
                references,
            })
        },
    )
}
