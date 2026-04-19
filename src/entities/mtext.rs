//! MTEXT entity (§19.4.23) — multi-line text.
//!
//! Unlike TEXT, MTEXT stores a formatted paragraph that may span
//! multiple lines, with embedded style codes (\\L for underline,
//! \\O for overline, \\S for stacked fractions, etc.) interpreted by
//! the renderer. The stream includes the insertion point, the text
//! direction vector, the rectangular width, the nominal text
//! height, and the string itself.
//!
//! # Stream shape (R2000+)
//!
//! ```text
//! BD3  insertion_point
//! BD3  extrusion
//! BD3  x_axis_direction    -- unit-length vector along text baseline
//! BD   rect_width           -- column width (text wraps to this)
//! (R2007+)
//!   BD   rect_height         -- for auto-height columns
//! BD   nominal_text_height   -- per-line height
//! BS   attachment_point     -- 1..9, alignment in the bounding box
//! BS   drawing_direction    -- left-to-right / right-to-left / ...
//! BD   extents_height        -- actual rendered height
//! BD   extents_width         -- actual rendered width (after layout)
//! TV   text_string           -- with embedded MTEXT control codes
//! BS   linespace_style        -- R2000+
//! BD   linespace_factor       -- R2000+
//! B    unknown_b              -- R2000+ (spec calls it "unknown bit")
//! (R2004+)
//!   BL   background_flags
//!   BL   background_scale_factor
//!   CMC  background_color
//!   BL   background_transparency
//! ```
//!
//! This decoder reads through `text_string` + `linespace_style/factor +
//! unknown_b`, which covers the fields viewers actually render. Post-
//! R2004 background fields are intentionally skipped.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct MText {
    pub insertion_point: Point3D,
    pub extrusion: Vec3D,
    pub x_axis_direction: Vec3D,
    pub rect_width: f64,
    pub rect_height: Option<f64>,
    pub nominal_text_height: f64,
    pub attachment_point: i16,
    pub drawing_direction: i16,
    pub extents_height: f64,
    pub extents_width: f64,
    pub text: String,
    pub linespace_style: i16,
    pub linespace_factor: f64,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<MText> {
    let insertion_point = read_bd3(c)?;
    let extrusion = read_bd3(c)?;
    let x_axis_direction = read_bd3(c)?;
    let rect_width = c.read_bd()?;
    let rect_height = if version.is_r2007_plus() {
        Some(c.read_bd()?)
    } else {
        None
    };
    let nominal_text_height = c.read_bd()?;
    let attachment_point = c.read_bs()?;
    let drawing_direction = c.read_bs()?;
    let extents_height = c.read_bd()?;
    let extents_width = c.read_bd()?;
    let text = read_tv(c, version)?;
    let linespace_style = c.read_bs()?;
    let linespace_factor = c.read_bd()?;
    let _unknown_b = c.read_b()?;
    Ok(MText {
        insertion_point,
        extrusion,
        x_axis_direction,
        rect_width,
        rect_height,
        nominal_text_height,
        attachment_point,
        drawing_direction,
        extents_height,
        extents_width,
        text,
        linespace_style,
        linespace_factor,
    })
}

fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if version.is_r2007_plus() {
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            let lo = c.read_rc()? as u16;
            let hi = c.read_rc()? as u16;
            units.push((hi << 8) | lo);
        }
        if units.last() == Some(&0) {
            units.pop();
        }
        String::from_utf16(&units)
            .map_err(|_| Error::SectionMap("MTEXT is not valid UTF-16".into()))
    } else {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(c.read_rc()?);
        }
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_mtext_r2000() {
        let mut w = BitWriter::new();
        // insertion point
        w.write_bd(10.0); w.write_bd(20.0); w.write_bd(0.0);
        // extrusion
        w.write_bd(0.0); w.write_bd(0.0); w.write_bd(1.0);
        // x axis direction
        w.write_bd(1.0); w.write_bd(0.0); w.write_bd(0.0);
        w.write_bd(100.0); // rect width
        w.write_bd(2.5); // text height
        w.write_bs(1); // attachment
        w.write_bs(5); // drawing direction
        w.write_bd(5.0); // extents height
        w.write_bd(50.0); // extents width
        // TV "Hi\nEveryone"
        let s = b"Hi\\PEveryone";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_bs(0); // linespace_style
        w.write_bd(1.0); // linespace_factor
        w.write_b(false); // unknown bit
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(m.insertion_point, Point3D { x: 10.0, y: 20.0, z: 0.0 });
        assert_eq!(m.rect_width, 100.0);
        assert_eq!(m.text, "Hi\\PEveryone");
        assert_eq!(m.attachment_point, 1);
    }
}
