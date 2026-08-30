//! ATTRIB entity (§19.4.1bis) — attribute value instance attached to
//! an INSERT. An ATTRIB is a TEXT with an extra tag (the attribute
//! name) and flags indicating whether it is invisible, constant,
//! verifiable, or preset.
//!
//! # Stream shape (R2000+)
//!
//! ```text
//! TEXT-like preamble     -- same 8-bit data_flag / insertion /
//!                            alignment / extrusion / thickness /
//!                            oblique / rotation / height / width /
//!                            text / generation / h_align / v_align
//!                            layout as TEXT (§19.4.46)
//! TV   tag               -- the attribute name (e.g. "PRICE")
//! BS   field_length       -- length-in-chars for verifiable attribs
//! RC   flags             -- bits: 0x01 invisible, 0x02 constant,
//!                           0x04 verifiable, 0x08 preset
//! (R2018+)
//!   B    lock_position
//! ```
//!
//! Implementation-wise, ATTRIB re-uses [`super::text::decode`] for
//! the TEXT-shaped preamble, then reads the ATTRIB-specific trailer.

use crate::bitcursor::BitCursor;
use crate::entities::text::{self, Text};
use crate::error::{Error, Result};
use crate::string_stream;
use crate::tables::modern;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Attrib {
    pub text: Text,
    pub tag: String,
    pub field_length: i16,
    pub flags: u8,
    pub lock_position: bool,
}

impl Attrib {
    /// Bit 0x01 of `flags`: the attribute is invisible.
    pub fn is_invisible(&self) -> bool {
        self.flags & 0x01 != 0
    }
    /// Bit 0x02 of `flags`: the attribute is constant.
    pub fn is_constant(&self) -> bool {
        self.flags & 0x02 != 0
    }
    /// Bit 0x04 of `flags`: verification is required on insertion.
    pub fn is_verifiable(&self) -> bool {
        self.flags & 0x04 != 0
    }
    /// Bit 0x08 of `flags`: the attribute is preset (no prompt on insertion).
    pub fn is_preset(&self) -> bool {
        self.flags & 0x08 != 0
    }
}

/// Decodes the `Attrib` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Attrib> {
    let text = text::decode(c, version)?;
    let tag = read_tv(c, version)?;
    let field_length = c.read_bs()?;
    let flags = c.read_rc()?;
    let lock_position = if matches!(version, Version::R2018) {
        c.read_b()?
    } else {
        false
    };
    Ok(Attrib {
        text,
        tag,
        field_length,
        flags,
        lock_position,
    })
}

/// Decode an R2007+ single-line ATTRIB whose `TV` fields live in the
/// object's string stream (ODA v5.4.1 §19.1 split layout, §19.4.2).
///
/// The data stream carries the TEXT field body (see
/// [`text::read_modern_fields`]), then the field length, the flags and
/// the R2018 lock-position bit; the string stream carries the
/// attribute value then the tag.
///
/// # Multi-line attributes are not decoded
///
/// R2010 added multi-line attributes, which embed a whole MTEXT record
/// between the TEXT body and the tag. `sample_AC1032.dwg` contains one
/// (`MULTI_LINE_ATT`, three strings in its stream rather than two);
/// this decoder does not model the embedded record and reports the
/// misalignment rather than guessing at it.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<Attrib> {
    let (mut strings, string_start) = modern::open_entity(payload, version)?;
    let mut c = BitCursor::new(payload);
    string_stream::seek(&mut c, object_body_start)?;
    crate::common_entity::read_common_entity_data(&mut c, version)?;
    let mut text = text::read_modern_fields(&mut c)?;
    let field_length = c.read_bs()?;
    let flags = c.read_rc()?;
    let lock_position = if matches!(version, Version::R2018) {
        c.read_b()?
    } else {
        false
    };
    let at = c.position_bits();
    if at != string_start {
        return Err(modern::misaligned("ATTRIB", at, string_start));
    }
    text.text = strings.read_tv()?;
    let tag = strings.read_tv()?;
    Ok(Attrib {
        text,
        tag,
        field_length,
        flags,
        lock_position,
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
            .map_err(|_| Error::SectionMap("ATTRIB tag is not valid UTF-16".into()))
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
    fn r2007_split_stream_attrib_reads_value_and_tag() {
        let mut body = BitWriter::new();
        text::tests::write_r2018_preamble(&mut body);
        body.write_rc(0xFF);
        body.write_rd(0.0);
        body.write_rd(0.0);
        body.write_b(true); // extrusion default
        body.write_b(true); // thickness zero
        body.write_rd(2.5); // height
        body.write_bs(0); // field length
        body.write_rc(0x01); // flags — invisible
        body.write_b(false); // lock position
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["17", "ATTINFO"]);
        let a = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(a.tag, "ATTINFO");
        assert_eq!(a.text.text, "17");
        assert_eq!(a.text.height, 2.5);
        assert!(a.is_invisible());
        assert!(!a.lock_position);
    }

    #[test]
    fn roundtrip_invisible_constant_attrib() {
        let mut w = BitWriter::new();
        // minimal TEXT payload
        w.write_rc(0x00);
        w.write_rd(0.0);
        w.write_rd(0.0);
        w.write_b(true); // ext default
        w.write_b(true); // thickness default
        w.write_bd(2.5);
        w.write_bs_u(3); // "ABC"
        w.write_rc(b'A');
        w.write_rc(b'B');
        w.write_rc(b'C');
        // attrib-specific
        w.write_bs_u(5); // tag "PRICE"
        w.write_rc(b'P');
        w.write_rc(b'R');
        w.write_rc(b'I');
        w.write_rc(b'C');
        w.write_rc(b'E');
        w.write_bs(0); // field_length
        w.write_rc(0x03); // invisible + constant
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let a = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(a.tag, "PRICE");
        assert_eq!(a.text.text, "ABC");
        assert!(a.is_invisible());
        assert!(a.is_constant());
        assert!(!a.is_verifiable());
    }
}
