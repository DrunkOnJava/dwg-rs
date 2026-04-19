//! STYLE table entry (§19.5.56) — text style (font + size policy).
//!
//! # Stream shape
//!
//! ```text
//! entry header
//! BS     flags            -- 0x01 shape-file, 0x04 vertical
//! BD     fixed_height     -- 0 ⇒ prompt for height per-insertion
//! BD     width_factor
//! BD     oblique_angle
//! RC     generation       -- 2=upside-down + backward bit flags
//! BD     last_height
//! TV     font_name
//! TV     bigfont_name
//! ```

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::tables::{TableEntryHeader, read_table_entry_header, read_tv};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub header: TableEntryHeader,
    pub flags: i16,
    pub fixed_height: f64,
    pub width_factor: f64,
    pub oblique_angle: f64,
    pub generation: u8,
    pub last_height: f64,
    pub font_name: String,
    pub bigfont_name: String,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Style> {
    let header = read_table_entry_header(c, version)?;
    let flags = c.read_bs()?;
    let fixed_height = c.read_bd()?;
    let width_factor = c.read_bd()?;
    let oblique_angle = c.read_bd()?;
    let generation = c.read_rc()?;
    let last_height = c.read_bd()?;
    let font_name = read_tv(c, version)?;
    let bigfont_name = read_tv(c, version)?;
    Ok(Style {
        header,
        flags,
        fixed_height,
        width_factor,
        oblique_angle,
        generation,
        last_height,
        font_name,
        bigfont_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_standard_style() {
        let mut w = BitWriter::new();
        let s = b"Standard";
        w.write_bs_u(s.len() as u16);
        for b in s { w.write_rc(*b); }
        w.write_b(false); w.write_bs(0); w.write_b(false);
        w.write_bs(0); // flags
        w.write_bd(0.0); // prompt for height
        w.write_bd(1.0); // width factor
        w.write_bd(0.0); // oblique
        w.write_rc(0); // generation normal
        w.write_bd(2.5);
        let font = b"arial.ttf";
        w.write_bs_u(font.len() as u16);
        for b in font { w.write_rc(*b); }
        w.write_bs_u(0); // no bigfont
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(s.header.name, "Standard");
        assert_eq!(s.width_factor, 1.0);
        assert!(s.font_name.starts_with("arial"));
        assert!(s.bigfont_name.is_empty());
    }
}
