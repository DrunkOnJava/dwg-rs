//! TEXT entity (§19.4.46) — single-line annotation.
//!
//! # Stream shape (R2000+)
//!
//! TEXT uses a "data flag" bit field (`RC` in older versions, a fused
//! `B` set in R2000+) to elide defaulted fields. The flag reserves
//! one bit per optional member; absent fields default as noted:
//!
//! | Bit | Field            | Default when unset |
//! |-----|------------------|--------------------|
//! | 0   | elevation        | 0.0                |
//! | 1   | alignment_point  | insertion_point    |
//! | 2   | oblique_angle    | 0.0                |
//! | 3   | rotation_angle   | 0.0                |
//! | 4   | width_factor     | 1.0                |
//! | 5   | generation       | 0 (normal)         |
//! | 6   | horizontal_align | 0 (left)           |
//! | 7   | vertical_align   | 0 (baseline)       |
//!
//! ```text
//! RC  data_flag
//! (if elevation present)
//!   RD  elevation
//! RD2 insertion_point
//! (if alignment_point present)
//!   RD2  alignment_point
//! BE  extrusion
//! BT  thickness
//! (if oblique present)      BD oblique
//! (if rotation present)     BD rotation
//! BD  height
//! (if width_factor present) BD width_factor
//! TV  text_string           -- variable, UTF-8/UTF-16 per version
//! (if generation present)   BS generation_flag
//! (if h_align present)      BS h_align
//! (if v_align present)      BS v_align
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Vec3D, read_be, read_bt};
use crate::error::{Error, Result};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub elevation: f64,
    pub insertion_point: Point2D,
    pub alignment_point: Option<Point2D>,
    pub extrusion: Vec3D,
    pub thickness: f64,
    pub oblique_angle: f64,
    pub rotation_angle: f64,
    pub height: f64,
    pub width_factor: f64,
    pub text: String,
    pub generation: i16,
    pub h_align: i16,
    pub v_align: i16,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Text> {
    let flag = c.read_rc()?;

    let elevation = if flag & 0x01 == 0 { 0.0 } else { c.read_rd()? };
    let ip_x = c.read_rd()?;
    let ip_y = c.read_rd()?;
    let insertion_point = Point2D { x: ip_x, y: ip_y };
    let alignment_point = if flag & 0x02 == 0 {
        None
    } else {
        let ax = c.read_rd()?;
        let ay = c.read_rd()?;
        Some(Point2D { x: ax, y: ay })
    };
    let extrusion = read_be(c)?;
    let thickness = read_bt(c)?;
    let oblique_angle = if flag & 0x04 == 0 { 0.0 } else { c.read_bd()? };
    let rotation_angle = if flag & 0x08 == 0 { 0.0 } else { c.read_bd()? };
    let height = c.read_bd()?;
    let width_factor = if flag & 0x10 == 0 { 1.0 } else { c.read_bd()? };

    let text = read_tv(c, version)?;

    let generation = if flag & 0x20 == 0 { 0 } else { c.read_bs()? };
    let h_align = if flag & 0x40 == 0 { 0 } else { c.read_bs()? };
    let v_align = if flag & 0x80 == 0 { 0 } else { c.read_bs()? };

    Ok(Text {
        elevation,
        insertion_point,
        alignment_point,
        extrusion,
        thickness,
        oblique_angle,
        rotation_angle,
        height,
        width_factor,
        text,
        generation,
        h_align,
        v_align,
    })
}

/// Read a variable-text (TV) field. R2007+ uses UTF-16LE with length
/// counted in codepoint shorts (excluding NUL). Prior versions use
/// 8-bit MBCS-or-ASCII.
fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if version.is_r2007_plus() {
        // UTF-16LE, `len` 16-bit units.
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            let lo = c.read_rc()? as u16;
            let hi = c.read_rc()? as u16;
            units.push((hi << 8) | lo);
        }
        // Strip trailing NUL if present.
        if units.last() == Some(&0) {
            units.pop();
        }
        String::from_utf16(&units).map_err(|_| {
            Error::SectionMap("TEXT string is not valid UTF-16".into())
        })
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
    fn roundtrip_minimal_text() {
        let mut w = BitWriter::new();
        w.write_rc(0x00); // no optional fields
        // insertion point
        w.write_rd(10.0);
        w.write_rd(20.0);
        // extrusion + thickness default
        w.write_b(true);
        w.write_b(true);
        // height
        w.write_bd(2.5);
        // text — 5 ASCII chars in an R2000-style TV
        w.write_bs_u(5);
        for b in b"HELLO" {
            w.write_rc(*b);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let t = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(t.elevation, 0.0);
        assert_eq!(t.insertion_point, Point2D { x: 10.0, y: 20.0 });
        assert_eq!(t.height, 2.5);
        assert_eq!(t.width_factor, 1.0);
        assert_eq!(t.text, "HELLO");
    }

    #[test]
    fn roundtrip_full_text_fields() {
        let mut w = BitWriter::new();
        // Set flags 0x01 | 0x02 | 0x04 | 0x08 | 0x10 | 0x20 = 0x3F
        w.write_rc(0x3F);
        w.write_rd(1.0); // elevation
        w.write_rd(10.0); w.write_rd(20.0); // insertion
        w.write_rd(11.0); w.write_rd(21.0); // alignment
        w.write_b(true); // ext default
        w.write_b(true); // thickness default
        w.write_bd(0.15); // oblique
        w.write_bd(0.75); // rotation
        w.write_bd(2.5); // height
        w.write_bd(0.9); // width_factor
        w.write_bs_u(2);
        w.write_rc(b'H'); w.write_rc(b'i');
        w.write_bs(1); // generation
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let t = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(t.elevation, 1.0);
        assert_eq!(t.alignment_point, Some(Point2D { x: 11.0, y: 21.0 }));
        assert!((t.oblique_angle - 0.15).abs() < 1e-12);
        assert!((t.width_factor - 0.9).abs() < 1e-12);
        assert_eq!(t.text, "Hi");
        assert_eq!(t.generation, 1);
    }
}
