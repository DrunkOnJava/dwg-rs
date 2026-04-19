//! DICTIONARY object (§19.5.19) — string-keyed handle map.
//!
//! DICTIONARY is the backbone of the DWG extension mechanism: every
//! custom object type (groups, layouts, materials, table styles,
//! multileader styles, visual styles) is attached to the drawing
//! via a chain of dictionaries rooted at the "named object
//! dictionary" (NOD). Each dictionary maps a string name to a
//! handle.
//!
//! # Stream shape
//!
//! ```text
//! BL     num_items
//! BS     cloning_flag     -- 1=keep, 2=ignore, 3=replace, 4=xref
//! RC     hard_owner_flag  -- R2000+
//! // For each entry:
//! TV     name
//! H      value_handle
//! ```
//!
//! This decoder only reads the body (name/handle pairs). Handles to
//! parent, reactors, xdictionary, etc. live *after* the body and are
//! consumed by the object-handle-reader pass, not here.

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};
use crate::tables::read_tv;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Dictionary {
    pub cloning_flag: i16,
    pub hard_owner: bool,
    pub entries: Vec<DictionaryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryEntry {
    pub name: String,
    pub value_handle: Handle,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Dictionary> {
    let num_items = c.read_bl()? as usize;
    if num_items > 1_000_000 {
        return Err(Error::SectionMap(format!(
            "DICTIONARY claims {num_items} items (>1M sanity cap)"
        )));
    }
    let cloning_flag = c.read_bs()?;
    let hard_owner_flag = c.read_rc()?;

    let mut entries = Vec::with_capacity(num_items);
    for _ in 0..num_items {
        let name = read_tv(c, version)?;
        let value_handle = c.read_handle()?;
        entries.push(DictionaryEntry { name, value_handle });
    }
    Ok(Dictionary {
        cloning_flag,
        hard_owner: hard_owner_flag != 0,
        entries,
    })
}

impl Dictionary {
    /// Look up a handle by string key (case-sensitive exact match).
    pub fn get(&self, name: &str) -> Option<&Handle> {
        self.entries
            .iter()
            .find(|e| e.name == name)
            .map(|e| &e.value_handle)
    }

    /// Count entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Is the dictionary empty?
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_empty_dict() {
        let mut w = BitWriter::new();
        w.write_bl(0); // no items
        w.write_bs(1); // keep
        w.write_rc(1); // hard-owner
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode(&mut c, Version::R2000).unwrap();
        assert!(d.is_empty());
        assert_eq!(d.cloning_flag, 1);
        assert!(d.hard_owner);
    }

    #[test]
    fn roundtrip_dict_with_entries() {
        let mut w = BitWriter::new();
        w.write_bl(2);
        w.write_bs(1);
        w.write_rc(1);
        // entry 1: "ACAD_LAYOUT" → handle
        let key1 = b"ACAD_LAYOUT";
        w.write_bs_u(key1.len() as u16);
        for b in key1 { w.write_rc(*b); }
        w.write_handle(3, 0x1A);
        // entry 2: "ACAD_MATERIAL" → handle
        let key2 = b"ACAD_MATERIAL";
        w.write_bs_u(key2.len() as u16);
        for b in key2 { w.write_rc(*b); }
        w.write_handle(3, 0x2B);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d.get("ACAD_LAYOUT").unwrap().value, 0x1A);
        assert_eq!(d.get("ACAD_MATERIAL").unwrap().value, 0x2B);
        assert!(d.get("NONEXISTENT").is_none());
    }
}
