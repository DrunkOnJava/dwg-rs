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
use crate::entities::mtext::{self, MText};
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
    /// R2018+ `RC 71` attribute type: 1 single line, 2 multi-line
    /// ATTRIB, 4 multi-line ATTDEF. `1` on earlier releases, which do
    /// not carry the field.
    pub attribute_type: u8,
    /// The embedded MTEXT record a multi-line attribute carries
    /// (§20.4.4). `None` for a single-line attribute.
    pub mtext: Option<MText>,
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
        attribute_type: 1,
        mtext: None,
    })
}

/// Decode an R2007+ ATTRIB through the object's split streams
/// (§20.4.4), single-line or multi-line.
///
/// # Measured field list (R2018)
///
/// §20.4.4 puts an `RC` version byte (R2010+) and an `RC` attribute
/// type (R2018+) between the shared TEXT body and the attribute's own
/// fields, and branches on the type. On `sample_AC1032.dwg` all four
/// ATTRIB records close exactly on their string-stream start bit under
/// that reading:
///
/// | handle | version | type | body ends | boundary |
/// |--------|---------|------|-----------|----------|
/// | `0x705` | 0 | 1 (single line) | 311 | 311 |
/// | `0x79F` | 0 | 1 (single line) | 389 | 389 |
/// | `0x7A0` | 0 | 1 (single line) | 311 | 311 |
/// | `0x79D` | 0 | 2 (multi-line)  | 1111 | 1111 |
///
/// The multi-line record embeds a whole MTEXT — "all fields of an
/// embedded MTEXT object … starting from the Entmode (entity mode)",
/// so the embedded record has no length, type code, handle, EED chain
/// or graphics block. Its 683 data bits sit between the attribute
/// type byte and the `BS` annotative-data size, and its text
/// (`"my multi line text for the attrrib"`) is the second of the
/// record's three strings — the first being the (empty) TEXT value and
/// the third the tag `"MULTI_LINE_ATT"`.
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
    let body = read_attribute_body(&mut c, version)?;
    let at = c.position_bits();
    if at != string_start {
        return Err(modern::misaligned("ATTRIB", at, string_start));
    }
    text.text = strings.read_tv()?;
    let mut embedded = body.mtext;
    if let Some(m) = embedded.as_mut() {
        m.text = strings.read_tv()?;
    }
    let tag = strings.read_tv()?;
    Ok(Attrib {
        text,
        tag,
        field_length: body.field_length,
        flags: body.flags,
        lock_position: body.lock_position,
        attribute_type: body.attribute_type,
        mtext: embedded,
    })
}

/// The attribute-specific fields shared by ATTRIB and ATTDEF: the
/// version byte, the attribute type, and whichever branch it selects.
pub(crate) struct AttributeBody {
    pub attribute_type: u8,
    pub field_length: i16,
    pub flags: u8,
    pub lock_position: bool,
    pub mtext: Option<MText>,
}

/// Maximum `annotative data size` accepted (§20.4.4 `BS`).
pub const MAX_ANNOTATIVE_DATA: usize = 65_535;

/// Read the R2010+ attribute body that follows the shared TEXT fields.
pub(crate) fn read_attribute_body(
    c: &mut BitCursor<'_>,
    version: Version,
) -> Result<AttributeBody> {
    if version.is_r2010_plus() {
        let _version_byte = c.read_rc()?;
    }
    let attribute_type = if matches!(version, Version::R2018) {
        c.read_rc()?
    } else {
        1
    };
    if attribute_type == 1 {
        let field_length = c.read_bs()?;
        let flags = c.read_rc()?;
        let lock_position = if version.is_r2007_plus() {
            c.read_b()?
        } else {
            false
        };
        return Ok(AttributeBody {
            attribute_type,
            field_length,
            flags,
            lock_position,
            mtext: None,
        });
    }
    // Multi-line: an embedded MTEXT record, starting at its entity-mode
    // bits, then the annotative-data block and the attribute's flags.
    crate::common_entity::read_entity_mode_onwards(c, version, false, false)?;
    let embedded = mtext::read_modern_fields(c, version)?;
    let annotative_size = c.read_bs_u()? as usize;
    if annotative_size > MAX_ANNOTATIVE_DATA || annotative_size * 8 > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "ATTRIB annotative data size {annotative_size} exceeds cap \
             ({MAX_ANNOTATIVE_DATA}) or remaining_bits ({})",
            c.remaining_bits()
        )));
    }
    if annotative_size > 0 {
        for _ in 0..annotative_size {
            let _ = c.read_rc()?;
        }
        // `H` registered application — handle stream, no data bits.
        let _unknown_72 = c.read_bs()?;
    }
    // `TV 2` tag string sits here in field order and consumes no data
    // bits on R2007+.
    let _unknown_73 = c.read_bs()?;
    let flags = c.read_rc()?;
    let lock_position = c.read_b()?;
    Ok(AttributeBody {
        attribute_type,
        field_length: 0,
        flags,
        lock_position,
        mtext: Some(embedded),
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
        body.write_rc(0); // R2010+ version byte
        body.write_rc(1); // R2018+ attribute type = single line
        body.write_bs(0); // field length
        body.write_rc(0x01); // flags — invisible
        body.write_b(false); // lock position
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["17", "ATTINFO"]);
        let a = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(a.tag, "ATTINFO");
        assert_eq!(a.text.text, "17");
        assert_eq!(a.text.height, 2.5);
        assert_eq!(a.attribute_type, 1);
        assert!(a.mtext.is_none());
        assert!(a.is_invisible());
        assert!(!a.lock_position);
    }

    /// The R2018 multi-line shape measured on `sample_AC1032.dwg`
    /// handle `0x79D`: attribute type `2`, then a whole MTEXT record
    /// embedded from its entity-mode bits on, then the annotative-data
    /// size and the attribute's own flags. Its three strings are the
    /// (empty) TEXT value, the embedded MTEXT's text, and the tag.
    #[test]
    fn r2018_multi_line_attrib_embeds_an_mtext() {
        let mut body = BitWriter::new();
        text::tests::write_r2018_preamble(&mut body);
        body.write_rc(0xFF); // TEXT data flags: nothing optional present
        body.write_rd(977.28); // insertion x
        body.write_rd(27.03); // insertion y
        body.write_b(true); // extrusion default
        body.write_b(true); // thickness zero
        body.write_rd(1.72); // height
        body.write_rc(0); // R2010+ version byte
        body.write_rc(2); // R2018+ attribute type = multi-line ATTRIB
        // Embedded MTEXT: the common entity preamble from entmode on.
        body.write_bb(0b00); // entmode
        body.write_bl(0); // num reactors
        body.write_b(true); // no xdictionary
        body.write_b(false); // no AcDs binary data
        body.write_bs(0); // colour
        body.write_bd(1.0); // linetype scale
        body.write_bb(0b00);
        body.write_bb(0b00);
        body.write_bb(0b00);
        body.write_rc(0);
        body.write_b(false);
        body.write_b(false);
        body.write_b(false);
        body.write_bs(0); // invisibility
        body.write_rc(0x1D); // lineweight
        // Embedded MTEXT field body.
        for v in [977.28, 28.75, 0.0] {
            body.write_bd(v);
        }
        for v in [0.0, 0.0, 1.0] {
            body.write_bd(v);
        }
        for v in [1.0, 0.0, 0.0] {
            body.write_bd(v);
        }
        body.write_bd(0.0); // rect width
        body.write_bd(0.0); // rect height
        body.write_bd(1.72); // nominal text height
        body.write_bs(1); // attachment point
        body.write_bs(5); // drawing direction
        body.write_bd(1.84); // extents height
        body.write_bd(16.37); // extents width
        body.write_bs(1); // linespace style
        body.write_bd(1.0); // linespace factor
        body.write_b(false); // unknown bit
        body.write_bl(0); // background flags
        body.write_b(false); // R2018 "is NOT annotative" — clear
        // Back in the ATTRIB.
        body.write_bs(0); // annotative data size
        body.write_bs(0); // unknown 73
        body.write_rc(0x00); // flags
        body.write_b(true); // lock position

        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(
            &bits,
            &["", "my multi line text for the attrrib", "MULTI_LINE_ATT"],
        );
        let a = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(a.attribute_type, 2);
        assert_eq!(a.tag, "MULTI_LINE_ATT");
        assert_eq!(a.text.text, "");
        let m = a.mtext.expect("embedded MTEXT");
        assert_eq!(m.text, "my multi line text for the attrrib");
        assert_eq!(m.extents_width, 16.37);
        assert!(a.lock_position);
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
