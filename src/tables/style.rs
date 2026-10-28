//! STYLE table entry (ODA Open Design Specification v5.4.1 §19.5.4,
//! L6-04) — text style (font + size policy).
//!
//! # Stream shape
//!
//! ```text
//! entry header (TV name + xref bits)
//! B      shape_file       -- surfaced as flags bit 0x01
//! B      vertical         -- surfaced as flags bit 0x04
//! BD     fixed_height     -- 0 prompts for height per-insertion
//! BD     width_factor
//! BD     oblique_angle    -- radians
//! RC     generation       -- bit 0x02 backward, bit 0x04 upside-down
//! BD     last_height
//! TV     font_filename
//! TV     bigfont_filename -- empty when none
//! ```

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::tables::{TableEntryHeader, modern, read_table_entry_header, read_tv};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct StyleEntry {
    pub header: TableEntryHeader,
    pub flags: u8,
    pub fixed_height: f64,
    pub width_factor: f64,
    pub oblique_angle: f64,
    pub generation: u8,
    pub last_height: f64,
    pub font_filename: String,
    pub bigfont_filename: String,
}

// Legacy alias retained so callers keep compiling while they migrate to
// [`StyleEntry`].
pub type Style = StyleEntry;

impl StyleEntry {
    /// True if the style is backed by a `.shx` shape file rather than a
    /// TrueType/font file (flags bit 0x01).
    pub fn is_shape_file(&self) -> bool {
        self.flags & 0x01 != 0
    }

    /// True if the style renders vertically (flags bit 0x04).
    pub fn is_vertical(&self) -> bool {
        self.flags & 0x04 != 0
    }

    /// True if the style is cloned from an external reference (flags
    /// bit 0x08).
    pub fn is_xref_dependent(&self) -> bool {
        self.flags & 0x08 != 0
    }
}

/// Decodes a `StyleEntry` table entry that follows the common object header.
///
/// # Measured
///
/// The shape-file / vertical pair is two `B` bits, not a packed `RC`
/// byte. In `line_2004.dwg` the `Standard` STYLE record has
/// `obj_size = 275` bits and its common object data ends at bit 63;
/// reading the pair as an `RC` overruns the record by exactly the
/// 6 extra bits (`Bit cursor exhausted: wanted 8 bits, 6 bits
/// remain`), while the two-bit form lands the trailing `TV` fields
/// inside the record. This matches the field order the R2007+
/// split-stream decoder was already measured against.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<StyleEntry> {
    let header = read_table_entry_header(c, version)?;
    let mut flags = 0u8;
    if c.read_b()? {
        flags |= 0x01; // shape file
    }
    if c.read_b()? {
        flags |= 0x04; // vertical
    }
    if header.is_xref_dependent {
        flags |= 0x08;
    }
    let fixed_height = c.read_bd()?;
    let width_factor = c.read_bd()?;
    let oblique_angle = c.read_bd()?;
    let generation = c.read_rc()?;
    let last_height = c.read_bd()?;
    let font_filename = read_tv(c, version)?;
    let bigfont_filename = read_tv(c, version)?;
    Ok(StyleEntry {
        header,
        flags,
        fixed_height,
        width_factor,
        oblique_angle,
        generation,
        last_height,
        font_filename,
        bigfont_filename,
    })
}

/// Decode an R2007+ STYLE whose `TV` fields live in the object's string
/// stream (ODA v5.4.1 §19.1 split layout, §20.4.56 STYLE field table).
///
/// The data stream carries, after the common object prefix:
///
/// ```text
/// B   64-flag
/// B   xref dependent
/// B   shape file
/// B   vertical
/// BD  fixed height
/// BD  width factor
/// BD  oblique angle
/// RC  generation
/// BD  last height
/// ```
///
/// and the string stream carries `name`, `font_filename`,
/// `bigfont_filename` in that order. `flags` is reconstructed from the
/// vertical / shape-file / xref-dependent bits so the accessors on
/// [`StyleEntry`] keep working across versions.
///
/// Verified against every STYLE record in `sample_AC1032.dwg` (R2018),
/// `line_2013.dwg` (R2013) and `arc_2010.dwg` (R2010): the data fields
/// above end exactly on the string-stream start bit in all of them.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<StyleEntry> {
    let mut split = modern::open_table_entry(payload, object_body_start, version)?;
    let (flag64, xref_index_plus_1, is_xref_dependent) = modern::read_entry_flags(&mut split.data)?;
    let is_shape_file = split.data.read_b()?;
    let is_vertical = split.data.read_b()?;
    let fixed_height = split.data.read_bd()?;
    let width_factor = split.data.read_bd()?;
    let oblique_angle = split.data.read_bd()?;
    let generation = split.data.read_rc()?;
    let last_height = split.data.read_bd()?;
    split.finish("STYLE")?;

    let name = split.strings.read_tv()?;
    let font_filename = split.strings.read_tv()?;
    let bigfont_filename = split.strings.read_tv()?;

    let mut flags = 0u8;
    if is_shape_file {
        flags |= 0x01;
    }
    if is_vertical {
        flags |= 0x04;
    }
    if is_xref_dependent {
        flags |= 0x08;
    }

    Ok(StyleEntry {
        header: TableEntryHeader {
            name,
            flag_64: flag64,
            xref_index_plus_1,
            is_xref_dependent,
        },
        flags,
        fixed_height,
        width_factor,
        oblique_angle,
        generation,
        last_height,
        font_filename,
        bigfont_filename,
    })
}
