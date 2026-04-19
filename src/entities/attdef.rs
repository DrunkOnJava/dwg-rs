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
use crate::entities::text::{self, Text};
use crate::error::{Error, Result};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct AttDef {
    pub text: Text,
    pub prompt: String,
    pub tag: String,
    pub field_length: i16,
    pub flags: u8,
    pub lock_position: bool,
}

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
