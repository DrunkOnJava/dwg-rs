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
/// BS  xref index + 1
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
            is_xref_dependent,
            xref_index_plus_1,
            is_xref_resolved: flag64,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn write_header(w: &mut BitWriter, name: &[u8]) {
        w.write_bs_u(name.len() as u16);
        for b in name {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
    }

    #[test]
    fn roundtrip_standard_style() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"Standard");
        w.write_b(false); // shape file
        w.write_b(false); // vertical
        w.write_bd(0.0); // fixed_height — prompt
        w.write_bd(1.0); // width factor
        w.write_bd(0.0); // oblique
        w.write_rc(0); // generation normal
        w.write_bd(2.5); // last_height
        let font = b"arial.ttf";
        w.write_bs_u(font.len() as u16);
        for b in font {
            w.write_rc(*b);
        }
        w.write_bs_u(0); // no bigfont
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(s.header.name, "Standard");
        assert_eq!(s.width_factor, 1.0);
        assert_eq!(s.last_height, 2.5);
        assert!(s.font_filename.starts_with("arial"));
        assert!(s.bigfont_filename.is_empty());
        assert!(!s.is_shape_file());
        assert!(!s.is_vertical());
    }

    #[test]
    fn r2007_split_stream_style_reads_names_from_string_stream() {
        // Data stream: common object prefix, then the STYLE field body.
        let mut body = BitWriter::new();
        body.write_bs_u(0); // no EED
        body.write_b(true); // no xdictionary
        body.write_b(false); // no binary data (R2013+)
        body.write_b(true); // 64-flag
        body.write_b(false); // xref dependent
        body.write_bs(0); // xref index + 1
        body.write_b(true); // shape file
        body.write_b(true); // vertical
        body.write_bd(0.2); // fixed height
        body.write_bd(1.0); // width factor
        body.write_bd(0.0); // oblique angle
        body.write_rc(0x04); // generation — upside down
        body.write_bd(2.5); // last height
        let bits = crate::string_stream::tests::bits_of(&body);

        let payload = crate::string_stream::tests::build_payload(
            &bits,
            &["Standard", "arial.ttf", "bigfont.shx"],
        );
        let s = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(s.header.name, "Standard");
        assert_eq!(s.font_filename, "arial.ttf");
        assert_eq!(s.bigfont_filename, "bigfont.shx");
        assert!(s.header.is_xref_resolved);
        assert!(s.is_vertical());
        assert!(s.is_shape_file());
        assert_eq!(s.fixed_height, 0.2);
        assert_eq!(s.width_factor, 1.0);
        assert_eq!(s.oblique_angle, 0.0);
        assert_eq!(s.generation, 0x04);
        assert_eq!(s.last_height, 2.5);
    }

    #[test]
    fn r2007_split_stream_style_rejects_misaligned_body() {
        // One extra bit in the data stream must be caught, not silently
        // absorbed — the field body no longer ends on the string start.
        let mut body = BitWriter::new();
        body.write_bs_u(0);
        body.write_b(true);
        body.write_b(false);
        body.write_b(false);
        body.write_b(false);
        body.write_bs(0);
        body.write_b(false);
        body.write_b(false);
        body.write_bd(0.0);
        body.write_bd(1.0);
        body.write_bd(0.0);
        body.write_rc(0);
        body.write_bd(0.0);
        body.write_b(false); // stray bit
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["Standard", "", ""]);
        let err = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap_err();
        assert!(
            format!("{err}").contains("STYLE data fields ended"),
            "{err}"
        );
    }

    #[test]
    fn roundtrip_vertical_shape_style() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"TXT-V");
        w.write_b(true); // shape file
        w.write_b(true); // vertical
        w.write_bd(0.2); // fixed height
        w.write_bd(0.9); // width factor
        w.write_bd(15.0_f64.to_radians());
        w.write_rc(0x04); // upside-down
        w.write_bd(0.2);
        let font = b"txt.shx";
        w.write_bs_u(font.len() as u16);
        for b in font {
            w.write_rc(*b);
        }
        w.write_bs_u(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert!(s.is_shape_file());
        assert!(s.is_vertical());
        assert_eq!(s.generation, 0x04);
        assert!((s.oblique_angle - 15.0_f64.to_radians()).abs() < 1e-12);
    }
}
