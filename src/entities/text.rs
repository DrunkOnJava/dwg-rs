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
use crate::string_stream;
use crate::tables::modern;
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

/// Decodes the `Text` payload that follows the common entity header.
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

/// Decode an R2007+ TEXT whose string lives in the object's string
/// stream (ODA v5.4.1 §19.1 split layout, §19.4.46 TEXT field table).
///
/// `object_body_start` is the bit just past the object header; this
/// consumes the common entity preamble itself. Every field is read
/// from the data stream except `text`, which comes from the string
/// stream, and the decode is rejected unless the data fields land
/// exactly on the string-stream start bit.
///
/// # Measured: `height` is an `RD`, not a `BD`
///
/// The TEXT records of `sample_AC1032.dwg` all carry `DataFlags = 0xFF`
/// — every optional field elided — leaving only the flag byte, the
/// `2RD` insertion point, `BE`, `BT` and the height. In object #236
/// that leaves bits 218..284 of the payload, and the height reads as a
/// clean `1.0` only when taken as a raw 64-bit `RD` starting at bit
/// 220, i.e. with `BE` and `BT` occupying one bit each and no `BD`
/// type code in front of the height. The pre-R2007 [`decode`] still
/// reads it as a `BD`; that path is untouched here.
pub fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<Text> {
    let (mut strings, string_start) = modern::open_entity(payload, version)?;
    let mut c = BitCursor::new(payload);
    string_stream::seek(&mut c, object_body_start)?;
    crate::common_entity::read_common_entity_data(&mut c, version)?;

    let mut text = read_modern_fields(&mut c)?;
    let at = c.position_bits();
    if at != string_start {
        return Err(modern::misaligned("TEXT", at, string_start));
    }
    text.text = strings.read_tv()?;
    Ok(text)
}

/// Read the R2007+ TEXT field body from the data stream, leaving
/// [`Text::text`] empty for the caller to fill from the string stream.
///
/// Shared with ATTRIB (§19.4.2) and ATTDEF (§19.4.3), whose records
/// begin with exactly this field list.
pub fn read_modern_fields(c: &mut BitCursor<'_>) -> Result<Text> {
    let flag = c.read_rc()?;
    let elevation = if flag & 0x01 == 0 { c.read_rd()? } else { 0.0 };
    let insertion_point = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let alignment_point = if flag & 0x02 == 0 {
        Some(Point2D {
            x: c.read_dd(insertion_point.x)?,
            y: c.read_dd(insertion_point.y)?,
        })
    } else {
        None
    };
    let extrusion = read_be(c)?;
    let thickness = read_bt(c)?;
    let oblique_angle = if flag & 0x04 == 0 { c.read_bd()? } else { 0.0 };
    let rotation_angle = if flag & 0x08 == 0 { c.read_bd()? } else { 0.0 };
    let height = c.read_rd()?;
    let width_factor = if flag & 0x10 == 0 { c.read_bd()? } else { 1.0 };
    let generation = if flag & 0x20 == 0 { c.read_bs()? } else { 0 };
    let h_align = if flag & 0x40 == 0 { c.read_bs()? } else { 0 };
    let v_align = if flag & 0x80 == 0 { c.read_bs()? } else { 0 };
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
        text: String::new(),
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
        String::from_utf16(&units)
            .map_err(|_| Error::SectionMap("TEXT string is not valid UTF-16".into()))
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
pub(crate) mod tests {
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

    /// Minimal R2018 common entity preamble — no XDATA, no graphics,
    /// ByLayer mode, default colour / linetype / flags.
    pub(crate) fn write_r2018_preamble(w: &mut BitWriter) {
        w.write_bs_u(0); // no XDATA
        w.write_b(false); // no graphics
        w.write_bb(0b00); // entity mode ByLayer
        w.write_bl(0); // no reactors
        w.write_b(true); // no xdictionary
        w.write_b(false); // no DS binary data
        w.write_bs(0); // CMC colour
        w.write_bd(1.0); // linetype scale
        w.write_bb(0b00); // linetype flags
        w.write_bb(0b00); // plotstyle flags
        w.write_bb(0b00); // material flags
        w.write_rc(0); // shadow flags
        w.write_b(false); // full visual style
        w.write_b(false); // face visual style
        w.write_b(false); // edge visual style
        w.write_bs(0); // invisibility
        w.write_rc(0); // lineweight
    }

    #[test]
    fn r2007_split_stream_text_reads_string_from_string_stream() {
        let mut body = BitWriter::new();
        write_r2018_preamble(&mut body);
        body.write_rc(0xFF); // every optional field elided
        body.write_rd(147.5); // insertion x
        body.write_rd(2.75); // insertion y
        body.write_b(true); // BE — default extrusion
        body.write_b(true); // BT — zero thickness
        body.write_rd(1.0); // height (RD, not BD)
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["Hello"]);
        let t = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(t.text, "Hello");
        assert_eq!(t.insertion_point, Point2D { x: 147.5, y: 2.75 });
        assert_eq!(t.height, 1.0);
        assert_eq!(t.width_factor, 1.0);
        assert_eq!(t.elevation, 0.0);
        assert_eq!(t.alignment_point, None);
        assert_eq!(t.thickness, 0.0);
    }

    #[test]
    fn roundtrip_full_text_fields() {
        let mut w = BitWriter::new();
        // Set flags 0x01 | 0x02 | 0x04 | 0x08 | 0x10 | 0x20 = 0x3F
        w.write_rc(0x3F);
        w.write_rd(1.0); // elevation
        w.write_rd(10.0);
        w.write_rd(20.0); // insertion
        w.write_rd(11.0);
        w.write_rd(21.0); // alignment
        w.write_b(true); // ext default
        w.write_b(true); // thickness default
        w.write_bd(0.15); // oblique
        w.write_bd(0.75); // rotation
        w.write_bd(2.5); // height
        w.write_bd(0.9); // width_factor
        w.write_bs_u(2);
        w.write_rc(b'H');
        w.write_rc(b'i');
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
