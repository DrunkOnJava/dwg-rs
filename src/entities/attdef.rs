//! ATTDEF entity (§19.4.1ter) — attribute *definition* attached to a
//! BLOCK. An ATTDEF supplies the default/prompt text that every
//! INSERT of the block will see; each INSERT then carries one ATTRIB
//! with the actual value.
//!
//! # Stream shape (R2000+)
//!
//! Same as ATTRIB (see [`super::attrib`]) with one extra TV between
//! the TEXT preamble and the tag:
//!
//! ```text
//! TEXT-like preamble
//! TV   prompt            -- e.g. "Enter part price:"
//! TV   tag
//! BS   field_length
//! RC   flags
//! (R2018+) B lock_position
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::attrib;
use crate::entities::mtext::MText;
use crate::entities::text::{self, Text};
use crate::error::{Error, Result};
use crate::string_stream;
use crate::tables::modern;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct AttDef {
    pub text: Text,
    pub prompt: String,
    pub tag: String,
    pub field_length: i16,
    pub flags: u8,
    pub lock_position: bool,
    /// R2018+ `RC 71` attribute type: 1 single line, 4 multi-line.
    pub attribute_type: u8,
    /// The embedded MTEXT record a multi-line attribute definition
    /// carries (§20.4.4 via §20.4.5). `None` for a single-line ATTDEF.
    pub mtext: Option<MText>,
}

/// Decodes the `AttDef` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AttDef> {
    let text = text::decode(c, version)?;
    let prompt = read_tv(c, version)?;
    let tag = read_tv(c, version)?;
    let field_length = c.read_bs()?;
    let flags = c.read_rc()?;
    let lock_position = if matches!(version, Version::R2018) {
        c.read_b()?
    } else {
        false
    };
    Ok(AttDef {
        text,
        prompt,
        tag,
        field_length,
        flags,
        lock_position,
        attribute_type: 1,
        mtext: None,
    })
}

/// Decode an R2007+ ATTDEF through the object's split streams
/// (§20.4.5), single-line or multi-line.
///
/// §20.4.5 defines ATTDEF as "Common ATTRIB Entity Data", then an
/// R2010+ `RC` version byte of its own, then a `TV 3` prompt. So the
/// whole ATTRIB body — including the R2018 attribute-type branch and
/// the embedded MTEXT a multi-line definition carries — comes first,
/// and only two fields are added.
///
/// # Measured (R2018, `sample_AC1032.dwg`)
///
/// | handle | type | body ends | boundary | strings |
/// |--------|------|-----------|----------|---------|
/// | `0x6F8` | 1 | 319 | 319 | value, tag, prompt |
/// | `0x797` | 1 | 397 | 397 | value, tag, prompt |
/// | `0x798` | 1 | 319 | 319 | value, tag, prompt |
/// | `0x799` | 1 | 319 | 319 | value, tag, prompt |
/// | `0x796` | 4 (multi-line) | 871 | 871 | value, MTEXT text, tag, prompt |
///
/// The previous reading of this record placed a lone unexplained `RC`
/// after the lock-position bit; it was the ATTDEF version byte, and the
/// two bytes §20.4.4 puts *before* the tag were missing entirely. The
/// three bytes cancel out on a single-line record, which is why the
/// old field list closed on the four single-line ATTDEFs and failed on
/// the multi-line one.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<AttDef> {
    let (mut strings, string_start) = modern::open_entity(payload, version)?;
    let mut c = BitCursor::new(payload);
    string_stream::seek(&mut c, object_body_start)?;
    crate::common_entity::read_common_entity_data(&mut c, version)?;
    let mut text = text::read_modern_fields(&mut c)?;
    let body = attrib::read_attribute_body(&mut c, version)?;
    if version.is_r2010_plus() {
        let _attdef_version = c.read_rc()?;
    }
    let at = c.position_bits();
    if at != string_start {
        return Err(modern::misaligned("ATTDEF", at, string_start));
    }
    text.text = strings.read_tv()?;
    let mut embedded = body.mtext;
    if let Some(m) = embedded.as_mut() {
        m.text = strings.read_tv()?;
    }
    let tag = strings.read_tv()?;
    let prompt = strings.read_tv()?;
    Ok(AttDef {
        text,
        prompt,
        tag,
        field_length: body.field_length,
        flags: body.flags,
        lock_position: body.lock_position,
        attribute_type: body.attribute_type,
        mtext: embedded,
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
            .map_err(|_| Error::SectionMap("ATTDEF tag/prompt is not valid UTF-16".into()))
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
    fn r2007_split_stream_attdef_reads_value_tag_and_prompt() {
        let mut body = BitWriter::new();
        text::tests::write_r2018_preamble(&mut body);
        body.write_rc(0xFF);
        body.write_rd(0.0);
        body.write_rd(0.0);
        body.write_b(true); // extrusion default
        body.write_b(true); // thickness zero
        body.write_rd(2.5); // height
        body.write_bs(0); // field length
        body.write_rc(0x00); // flags
        body.write_b(false); // lock position
        body.write_rc(0); // trailing ATTDEF byte
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload =
            crate::string_stream::tests::build_payload(&bits, &["1", "ATTINFO", "Enter number:"]);
        let a = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(a.tag, "ATTINFO");
        assert_eq!(a.prompt, "Enter number:");
        assert_eq!(a.text.text, "1");
        assert_eq!(a.text.height, 2.5);
    }

    #[test]
    fn roundtrip_attdef_r2000() {
        let mut w = BitWriter::new();
        // TEXT preamble — minimal
        w.write_rc(0x00);
        w.write_rd(0.0);
        w.write_rd(0.0);
        w.write_b(true);
        w.write_b(true);
        w.write_bd(2.5);
        w.write_bs_u(0); // empty default text
        // ATTDEF extras
        w.write_bs_u(14); // prompt
        for b in b"Enter price: " {
            w.write_rc(*b);
        }
        w.write_rc(0); // trailing NUL (stripped)
        w.write_bs_u(5); // tag
        for b in b"PRICE" {
            w.write_rc(*b);
        }
        w.write_bs(0); // field length
        w.write_rc(0x00); // flags
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let a = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(a.prompt, "Enter price: ");
        assert_eq!(a.tag, "PRICE");
    }
}
