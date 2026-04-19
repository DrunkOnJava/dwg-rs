//! BLOCK entity (§19.4.3) — marks the start of a block's entity
//! sublist inside a BLOCK_HEADER (the BLOCK_HEADER holds the list of
//! entities; BLOCK/ENDBLK are sentinels that delimit it).
//!
//! # Stream shape
//!
//! ```text
//! TV   name            -- block name ("A$C0062DE6B", "*Model_Space", etc.)
//! ```
//!
//! The rest of a block's content lives on the BLOCK_HEADER table
//! entry, not on BLOCK itself.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub name: String,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Block> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(Block {
            name: String::new(),
        });
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
        let name = String::from_utf16(&units)
            .map_err(|_| Error::SectionMap("BLOCK name is not valid UTF-16".into()))?;
        Ok(Block { name })
    } else {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(c.read_rc()?);
        }
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        Ok(Block {
            name: String::from_utf8_lossy(&bytes).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_modelspace_block_r2000() {
        let mut w = BitWriter::new();
        let s = b"*Model_Space";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(b.name, "*Model_Space");
    }
}
