//! STYLE table entry (ODA Open Design Specification v5.4.1 §19.5.4,
//! L6-04) — text style (font + size policy).
//!
//! # Stream shape
//!
//! ```text
//! entry header (TV name + xref bits)
//! RC     flags            -- 0x01 shape-file, 0x04 vertical, 0x08 xref-dep
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
use crate::tables::{TableEntryHeader, read_table_entry_header, read_tv};
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

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<StyleEntry> {
    let header = read_table_entry_header(c, version)?;
    let flags = c.read_rc()?;
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
        w.write_rc(0); // flags
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
    fn roundtrip_vertical_shape_style() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"TXT-V");
        w.write_rc(0x05); // shape file + vertical
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
