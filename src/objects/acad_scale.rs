//! ACAD_SCALE object (spec §19.6.8 — L6-15) — single entry in the
//! per-drawing scale list.
//!
//! The drawing's named-object dictionary carries an `ACAD_SCALELIST`
//! sub-dictionary; each value in that sub-dictionary points at one
//! of these `ACAD_SCALE` records. A scale is a named ratio of paper
//! units to drawing units — `1:1`, `1/4" = 1'-0"`, `1:50`, etc.
//!
//! `SCALE` is a custom class (`AcDbScale`), so its object type code is
//! assigned per file through `AcDb:Classes`; the dispatcher resolves it
//! by DXF class name.
//!
//! # Stream shape — measured
//!
//! ```text
//! BS    version
//! TV    scale_name
//! BD    paper_units
//! BD    drawing_units
//! B     is_unit_scale
//! ```
//!
//! An earlier cut of this decoder read `TV, BD, BD, BS` — no leading
//! `BS`, and a `BS flag` where the wire has a single `B`. The bytes say
//! otherwise, in both layouts:
//!
//! - `arc_2004.dwg` (AC1018), the `1:1` record (handle 67). Its `RL`
//!   object-data-size leaves 49 bits after the common object data, and
//!   they are spent exactly as `BS = 0` (2 bits), `TV` of length 4
//!   whose bytes are `31 3A 31 00` — `"1:1"` and its NUL — (46 bits),
//!   `BD = 1.0` (2 bits), `BD = 1.0` (2 bits), `B = 1` (1 bit). The
//!   `B` reading is corroborated semantically: `1:1` is precisely the
//!   record AutoCAD marks as the unit scale.
//! - `arc_2013.dwg` (AC1027), the `1:2` record (handle 68). With the
//!   name in the string stream the budget is 71 bits: `BS = 0`,
//!   `BD = 1.0`, `BD` as a full 66-bit double whose eight little-endian
//!   bytes are `00 00 00 00 00 00 00 40` — 2.0 — and `B = 0`.
//!   Its `1:1` sibling (handle 67) spends 7 bits on the same fields
//!   with both `BD`s taking the 1.0 short form and `B = 1`.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct AcadScale {
    /// Leading `BS`; 0 in every record observed.
    pub version: i16,
    pub scale_name: String,
    pub paper_units: f64,
    pub drawing_units: f64,
    /// True for the record AutoCAD treats as the 1:1 unit scale.
    pub is_unit_scale: bool,
}

impl AcadScale {
    /// Numeric scale ratio = `paper_units / drawing_units`.
    /// Returns `None` if `drawing_units == 0.0` (malformed record).
    pub fn ratio(&self) -> Option<f64> {
        if self.drawing_units == 0.0 {
            None
        } else {
            Some(self.paper_units / self.drawing_units)
        }
    }
}

/// Read the ACAD_SCALE field list from whichever streams hold it.
fn read_body(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<AcadScale> {
    let scale_version = c.read_bs()?;
    let scale_name = modern::read_tv(c, strings, version)?;
    let paper_units = c.read_bd()?;
    let drawing_units = c.read_bd()?;
    let is_unit_scale = c.read_b()?;
    Ok(AcadScale {
        version: scale_version,
        scale_name,
        paper_units,
        drawing_units,
        is_unit_scale,
    })
}

/// Decodes the `AcadScale` payload that follows the common object header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AcadScale> {
    read_body(c, &mut None, version)
}

/// Decode an ACAD_SCALE straight from its raw object payload, taking the
/// scale name from the R2007+ string stream when the file has one and
/// checking the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadScale> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings, version)?;
    split.finish("SCALE")?;
    Ok(out)
}
