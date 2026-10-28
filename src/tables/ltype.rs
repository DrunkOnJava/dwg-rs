//! LTYPE table entry (ODA Open Design Specification v5.4.1 §19.5.3,
//! L6-03) — linetype definition (dash/dot/text/shape pattern).
//!
//! # Stream shape (§20.4.58)
//!
//! ```text
//! entry header (TV name + xref bits)
//! TV     description
//! BD     pattern_length
//! RC     alignment              -- always 'A' (0x41) in practice
//! RC     num_dashes             -- per ODA §19.5.3, capped at 256
//!
//! For each dash (0..num_dashes):
//!   BD   length                 -- positive = dash, negative = gap, 0 = dot
//!   BS   shape_number           -- index into the shape file, 0 when text
//!   RD   x_offset
//!   RD   y_offset
//!   BD   scale
//!   BD   rotation               -- radians
//!   BS   shape_flag             -- bit 0x02: has text, bit 0x04: has shape
//!
//! R2004 and earlier: 256 bytes of text area (`X 9`)
//! ```
//!
//! `num_dashes` is capped at 256; anything larger is a malformed or
//! adversarial file.
//!
//! The `flags` and `used_count` fields on [`LtypeEntry`] are retained
//! for API compatibility and are always zero: §20.4.58 lists no such
//! fields, and reading them pushed every pre-R2007 description three
//! bytes late (see [`decode`]).

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};
use crate::tables::modern;
use crate::tables::{TableEntryHeader, read_table_entry_header, read_tv};
use crate::version::Version;

/// Size of the §20.4.58 `X 9` text area on R2004 and earlier — a fixed
/// 256-byte pile of NUL-terminated strings that the complex-dash
/// 75-group indices point into.
pub const LTYPE_TEXT_AREA_BYTES: usize = 256;

/// Cap on the number of dash records per LTYPE, per spec §20.4.58.
pub const MAX_DASHES: usize = 256;

/// Text that follows a shape-carrying dash (SHAPE_FLAG bit 0x02) or
/// the handle of the STYLE entry referenced when bit 0x04 is set.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DashText {
    #[default]
    None,
    Inline(String),
    StyleHandle(Handle),
}

/// One entry in the dash/shape pattern.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LtypeDash {
    pub length: f64,
    pub shape_flag: i16,
    pub x_offset: f64,
    pub y_offset: f64,
    pub scale: f64,
    pub rotation: f64,
    pub shape_number: i16,
    pub text: DashText,
}

impl LtypeDash {
    /// True if this dash draws embedded text (SHAPE_FLAG bit 0x02).
    pub fn has_text(&self) -> bool {
        self.shape_flag & 0x02 != 0
    }

    /// True if this dash draws an embedded shape (SHAPE_FLAG bit 0x04).
    pub fn has_shape(&self) -> bool {
        self.shape_flag & 0x04 != 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LtypeEntry {
    pub header: TableEntryHeader,
    pub flags: u8,
    pub used_count: i16,
    pub description: String,
    pub pattern_length: f64,
    pub alignment: u8,
    pub dashes: Vec<LtypeDash>,
    /// The `X 9` text area as stored: 256 bytes up to R2004, on R2007 and later
    /// 512 bytes of UTF-16LE when some dash carries text, else empty. A text
    /// dash's `shape_number` is its text's byte offset in it.
    pub text_area: Vec<u8>,
}

// Legacy alias retained so callers keep compiling while they migrate to
// [`LtypeEntry`].
pub type LType = LtypeEntry;

/// Decodes a `LtypeEntry` table entry that follows the common object header.
///
/// # Field list (§20.4.58)
///
/// ```text
/// TV   entry name       -- via read_table_entry_header
/// B    64-flag
/// BS   xrefindex + 1
/// B    xdep
/// TV   description
/// BD   pattern length
/// RC   alignment        -- always 'A'
/// RC   numdashes
/// repeat numdashes {
///   BD dash length, BS complex shapecode, RD x-offset, RD y-offset,
///   BD scale, BD rotation, BS shapeflag
/// }
/// R2004 and earlier: 256 bytes of text area
/// ```
///
/// Two departures from an earlier revision, both measured:
///
/// - The `RC flags` + `RS used_count` this used to read after the entry
///   header are not in §20.4.58 and are not in the bytes. They pushed
///   the description three bytes late, which is why every pre-R2007
///   `Continuous` record in the corpus reported its description as
///   `lid line` followed by 200-odd NULs instead of `Solid line` — the
///   R2007+ split-stream path on the same drawing reads `Solid line`.
/// - The dash record's field order now matches §20.4.58 and the
///   R2007+ path in this module, and the `X 9` text area of "R2004 and
///   earlier" is consumed. With both in place every pre-R2007 LTYPE
///   record of the corpus lands exactly on its data-stream boundary.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<LtypeEntry> {
    let header = read_table_entry_header(c, version)?;
    let flags = 0u8;
    let used_count = 0i16;
    let description = read_tv(c, version)?;
    let pattern_length = c.read_bd()?;
    let alignment = c.read_rc()?;
    let num_dashes = c.read_rc()? as usize;
    if num_dashes > MAX_DASHES {
        return Err(Error::SectionMap(format!(
            "LTYPE num_dashes {num_dashes} exceeds spec cap of {MAX_DASHES}"
        )));
    }
    let mut dashes = Vec::with_capacity(num_dashes);
    for _ in 0..num_dashes {
        let length = c.read_bd()?;
        let shape_number = c.read_bs()?;
        let x_offset = c.read_rd()?;
        let y_offset = c.read_rd()?;
        let scale = c.read_bd()?;
        let rotation = c.read_bd()?;
        let shape_flag = c.read_bs()?;
        dashes.push(LtypeDash {
            length,
            shape_flag,
            x_offset,
            y_offset,
            scale,
            rotation,
            shape_number,
            text: DashText::None,
        });
    }
    // §20.4.58 "R2004 and earlier: X 9 — 256 bytes of text area". It is
    // unconditional on these releases, unlike the R2007+ 512-byte area
    // which is written only when some dash sets `shapeflag & 0x02`.
    let mut text_area = Vec::new();
    if !version.is_r2007_plus() {
        for _ in 0..LTYPE_TEXT_AREA_BYTES {
            text_area.push(c.read_rc()?);
        }
    }
    Ok(LtypeEntry {
        header,
        flags,
        used_count,
        description,
        pattern_length,
        alignment,
        dashes,
        text_area,
    })
}

/// Decode an R2007+ LTYPE through the object's split streams (§20.4.58).
///
/// # Measured field order and widths
///
/// §20.4.58 lists the dash record as `BD 49 dash length`, `BS 75
/// complex shapecode`, `RD 44 x-offset`, `RD 45 y-offset`, `BD 46
/// scale`, `BD 50 rotation`, `BS 74 shapeflag` — note the two raw
/// doubles in the middle, and that the shape flag comes *after* the
/// geometry, not before it. The pre-R2007 [`decode`] path in this
/// module reads a different order inherited from an earlier revision;
/// it is left alone because no R2004-or-earlier sample in the corpus
/// exercises a dash record.
///
/// The `X 9` text area at the end is R2007+ 512 bytes **only if some
/// dash sets `shapeflag & 0x02`**, and absent otherwise (§20.4.58).
/// All three failing LTYPE records of `sample_AC1032.dwg` have that bit
/// clear on every dash and close exactly on their string-stream start
/// bit with no text area read.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<LtypeEntry> {
    if !version.is_r2007_plus() {
        let mut c = BitCursor::new(payload);
        skip_to(&mut c, object_body_start)?;
        return decode(&mut c, version);
    }

    let mut split = modern::open_table_entry(payload, object_body_start, version)?;
    let (flag64, xref_index_plus_1, is_xref_dependent) = modern::read_entry_flags(&mut split.data)?;
    let pattern_length = split.data.read_bd()?;
    let alignment = split.data.read_rc()?;
    let num_dashes = split.data.read_rc()? as usize;
    if num_dashes > MAX_DASHES {
        return Err(Error::SectionMap(format!(
            "LTYPE num_dashes {num_dashes} exceeds spec cap of {MAX_DASHES}"
        )));
    }

    let mut dashes = Vec::with_capacity(num_dashes);
    let mut has_text_area = false;
    for _ in 0..num_dashes {
        let length = split.data.read_bd()?;
        let shape_number = split.data.read_bs()?;
        let x_offset = split.data.read_rd()?;
        let y_offset = split.data.read_rd()?;
        let scale = split.data.read_bd()?;
        let rotation = split.data.read_bd()?;
        let shape_flag = split.data.read_bs()?;
        has_text_area |= shape_flag & 0x02 != 0;
        dashes.push(LtypeDash {
            length,
            shape_flag,
            x_offset,
            y_offset,
            scale,
            rotation,
            shape_number,
            text: DashText::None,
        });
    }
    let mut text_area = Vec::new();
    if has_text_area {
        for _ in 0..512 {
            text_area.push(split.data.read_rc()?);
        }
    }
    split.finish("LTYPE")?;

    let name = split.strings.read_tv()?;
    let description = split.strings.read_tv()?;

    Ok(LtypeEntry {
        header: TableEntryHeader {
            name,
            flag_64: flag64,
            xref_index_plus_1,
            is_xref_dependent,
        },
        flags: if flag64 { 64 } else { 0 },
        used_count: 0,
        description,
        pattern_length,
        alignment,
        dashes,
        text_area,
    })
}

fn skip_to(c: &mut BitCursor<'_>, bit: usize) -> Result<()> {
    while c.position_bits() < bit {
        let _ = c.read_b()?;
    }
    Ok(())
}
