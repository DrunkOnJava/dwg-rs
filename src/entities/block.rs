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
use crate::string_stream::StringReader;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub name: String,
}

/// Decode a BLOCK, taking its one `TV` from whichever stream holds it.
///
/// `Some(reader)` is the R2010+ split layout: the name's characters
/// live in the object's string stream and the slot costs the data
/// stream nothing. `None` is the inline layout of R2000-R2004.
///
/// # Measured
///
/// BLOCK's entire field list is that one `TV`, so on R2010+ its data
/// fields have a budget of **zero** bits — and every BLOCK record in
/// the corpus has exactly that: 27/27 on `sample_AC1032.dwg` (R2018)
/// and 3/3 on each of `arc_2010.dwg` and `arc_2013.dwg`, measured with
/// `examples/probe_entity_budgets.rs`. Reading the name inline instead
/// overran the boundary on all 33 (deltas +58 … +266). On R2004 the
/// same records spend 114-122 bits there, which is exactly the inline
/// `BS` length plus the NUL-terminated name, and they close on the
/// `RL` object-data-size with the reading below.
pub fn decode_field(
    c: &mut BitCursor<'_>,
    version: Version,
    strings: &mut Option<StringReader<'_>>,
) -> Result<Block> {
    match strings.as_mut() {
        Some(reader) => Ok(Block {
            name: reader.read_tv()?,
        }),
        None => decode(c, version),
    }
}

/// Decodes the `Block` payload that follows the common entity header.
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
