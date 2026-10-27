//! TEXT entity (§20.4.3, type 1) — single-line annotation.
//!
//! # Stream shape (§20.4.3)
//!
//! ```text
//! R13-R14:
//!   BD   elevation
//!   2RD  insertion point                    10
//!   2RD  alignment point                    11
//!   3BD  extrusion                         210
//!   BD   thickness                          39
//!   BD   oblique angle                      51
//!   BD   rotation angle                     50
//!   BD   height                             40
//!   BD   width factor                       41
//!   TV   text                                1
//!   BS   generation, horizontal, vertical alignment   71, 72, 73
//! R2000+:
//!   RC   data flags; a SET bit means the field is NOT present
//!   RD   elevation          if !(flags & 0x01)
//!   2RD  insertion point                    10
//!   2DD  alignment point    if !(flags & 0x02), defaults the insertion point  11
//!   BE   extrusion                         210
//!   BT   thickness                          39
//!   RD   oblique angle      if !(flags & 0x04)   51
//!   RD   rotation angle     if !(flags & 0x08)   50
//!   RD   height                             40
//!   RD   width factor       if !(flags & 0x10)   41
//!   TV   text                                1
//!   BS   generation         if !(flags & 0x20)   71
//!   BS   horizontal align   if !(flags & 0x40)   72
//!   BS   vertical align     if !(flags & 0x80)   73
//! ```
//!
//! The STYLE handle follows in the handle stream. On R2007+ the `TV`
//! lives in the string stream and costs no data-stream bits.
//!
//! # Measured (2026-10-26, #226 / #62)
//!
//! The previous R2000+ reader inverted the data flags (it read a field
//! when its bit was *set*) and read the height and the three optional
//! angles and factors as `BD`; R13/R14 records went through the same
//! R2000+ reader. The R2007+ reader had the flags right and the height
//! as an `RD`, but still read oblique, rotation and width factor as
//! `BD` where §20.4.3 prints `RD` (#62). Every TEXT record of the old
//! corpus elides those three fields, so nothing showed it.
//!
//! The public corpus does exercise them, and §20.4.3 as printed closes
//! every TEXT record in it, from R14 to R2018. ARCHITECTURE.md §7v has
//! the table; `examples/probe_text.rs` compares the values with
//! AutoCAD's DXF exports.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Vec3D, read_bd3, read_be, read_bt, read_rd2};
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

/// Decodes the §20.4.3 TEXT payload that follows the common entity
/// data, with the `TV` inline (R13 to R2004).
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Text> {
    if version.is_r13_r14() {
        return read_r13_r14_fields(c, version);
    }
    read_r2000_fields(c, |c| read_tv(c, version))
}

/// The R13-R14 branch of §20.4.3: every field present, all `BD` apart
/// from the two `2RD` points.
fn read_r13_r14_fields(c: &mut BitCursor<'_>, version: Version) -> Result<Text> {
    let elevation = c.read_bd()?;
    let insertion_point = read_rd2(c)?;
    let alignment_point = Some(read_rd2(c)?);
    let extrusion = read_bd3(c)?;
    let thickness = c.read_bd()?;
    let oblique_angle = c.read_bd()?;
    let rotation_angle = c.read_bd()?;
    let height = c.read_bd()?;
    let width_factor = c.read_bd()?;
    let text = read_tv(c, version)?;
    let generation = c.read_bs()?;
    let h_align = c.read_bs()?;
    let v_align = c.read_bs()?;
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

/// The R2000+ branch of §20.4.3. `read_text` reads the `TV` at its
/// place in the field order: inline before R2007, nothing (the caller
/// takes it from the string stream) from R2007.
fn read_r2000_fields(
    c: &mut BitCursor<'_>,
    read_text: impl FnOnce(&mut BitCursor<'_>) -> Result<String>,
) -> Result<Text> {
    let flag = c.read_rc()?;
    let elevation = if flag & 0x01 == 0 { c.read_rd()? } else { 0.0 };
    let insertion_point = read_rd2(c)?;
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
    let oblique_angle = if flag & 0x04 == 0 { c.read_rd()? } else { 0.0 };
    let rotation_angle = if flag & 0x08 == 0 { c.read_rd()? } else { 0.0 };
    let height = c.read_rd()?;
    let width_factor = if flag & 0x10 == 0 { c.read_rd()? } else { 1.0 };
    let text = read_text(c)?;
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
        text,
        generation,
        h_align,
        v_align,
    })
}

/// Decode an R2007+ TEXT whose string lives in the object's string
/// stream (ODA v5.4.1 §19.1 split layout, §20.4.3 TEXT field table).
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
/// type code in front of the height, as §20.4.3 prints it. The
/// R2000-R2004 [`decode`] now reads the same field list.
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
/// Shared with ATTRIB (§20.4.4) and ATTDEF (§20.4.5), whose records
/// begin with exactly this field list.
pub fn read_modern_fields(c: &mut BitCursor<'_>) -> Result<Text> {
    read_r2000_fields(c, |_| Ok(String::new()))
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
        w.write_rc(0xFF); // no optional fields
        // insertion point
        w.write_rd(10.0);
        w.write_rd(20.0);
        // extrusion + thickness default
        w.write_b(true);
        w.write_b(true);
        // height
        w.write_rd(2.5);
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
        // Clear flags 0x01 | 0x02 | 0x04 | 0x08 | 0x10 | 0x20: present
        w.write_rc(0xC0);
        w.write_rd(1.0); // elevation
        w.write_rd(10.0);
        w.write_rd(20.0); // insertion
        w.write_dd(10.0, 11.0);
        w.write_dd(20.0, 21.0); // alignment
        w.write_b(true); // ext default
        w.write_b(true); // thickness default
        w.write_rd(0.15); // oblique
        w.write_rd(0.75); // rotation
        w.write_rd(2.5); // height
        w.write_rd(0.9); // width_factor
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
