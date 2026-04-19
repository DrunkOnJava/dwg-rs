//! BLOCK_HEADER (aka BLOCK_RECORD) table entry (§19.5.51) — the
//! authoritative record for a block definition. Holds the block's
//! name, base point, and the handles of its first/last entities.
//!
//! # Stream shape
//!
//! ```text
//! entry header (name + xref bits)
//! B      is_anonymous
//! B      has_attribs
//! B      is_xref           -- block references external file
//! B      xref_overlay      -- for xref blocks, overlay vs attach
//! B      is_loaded_xref    -- xref has resolved
//! (R2004+)
//!   BL   num_owned_objects
//! BD3    base_point
//! TV     xref_path         -- filesystem path for xref blocks
//! (R2004+)
//!   RC*  insert_count_bytes  -- until 0x00 terminator; lets older
//!                               readers skip the count
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::tables::{TableEntryHeader, read_table_entry_header, read_tv};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct BlockRecord {
    pub header: TableEntryHeader,
    pub is_anonymous: bool,
    pub has_attribs: bool,
    pub is_xref: bool,
    pub xref_overlay: bool,
    pub is_loaded_xref: bool,
    pub num_owned_objects: Option<u32>,
    pub base_point: Point3D,
    pub xref_path: String,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<BlockRecord> {
    let header = read_table_entry_header(c, version)?;
    let is_anonymous = c.read_b()?;
    let has_attribs = c.read_b()?;
    let is_xref = c.read_b()?;
    let xref_overlay = c.read_b()?;
    let is_loaded_xref = c.read_b()?;
    let num_owned_objects = if version.is_r2004_plus() {
        Some(c.read_bl()? as u32)
    } else {
        None
    };
    let base_point = read_bd3(c)?;
    let xref_path = read_tv(c, version)?;
    Ok(BlockRecord {
        header,
        is_anonymous,
        has_attribs,
        is_xref,
        xref_overlay,
        is_loaded_xref,
        num_owned_objects,
        base_point,
        xref_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_model_space_block_record_r2004() {
        let mut w = BitWriter::new();
        let s = b"*Model_Space";
        w.write_bs_u(s.len() as u16);
        for b in s { w.write_rc(*b); }
        w.write_b(false); w.write_bs(0); w.write_b(false);
        // 5 flag bits — all false
        w.write_b(false); w.write_b(false); w.write_b(false);
        w.write_b(false); w.write_b(false);
        // R2004+: num_owned_objects
        w.write_bl(42);
        // base point at origin
        w.write_bd(0.0); w.write_bd(0.0); w.write_bd(0.0);
        // empty xref path
        w.write_bs_u(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode(&mut c, Version::R2004).unwrap();
        assert_eq!(b.header.name, "*Model_Space");
        assert_eq!(b.num_owned_objects, Some(42));
        assert!(!b.is_xref);
    }
}
