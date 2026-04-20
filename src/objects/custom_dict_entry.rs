//! Generic walker for unknown ACAD_* and third-party entries in the
//! named-objects dictionary (spec §19.5.19 — dictionary body layout).
//!
//! The named-object DICTIONARY tree is extensible: vertical
//! applications drop their own top-level keys (`AcDbAecDbMgr`,
//! `AcDbVariableDictionary`, `SPATIAL_INDEX`, etc.) without updating
//! the DWG spec. Callers that want to enumerate "everything in the
//! NOD" — for instance to surface third-party data in an inspector
//! UI — use this decoder to read a flat list of `(key, handle)`
//! pairs without committing to a type for the referenced object.
//!
//! # Stream shape
//!
//! ```text
//! BS    num_entries           -- ≤ 100 000
//! // For each entry:
//! TV    key                   -- dictionary key string (e.g. "ACAD_LAYOUT")
//! H     value_handle          -- soft pointer to the value object
//! ```
//!
//! # Relationship to [`super::dictionary::Dictionary`]
//!
//! `Dictionary` is the *canonical* decoder for a DICTIONARY object
//! body (BL-counted). This module exists for contexts where the
//! enclosing object carries only a BS count — notably the "custom
//! dictionary" side-car emitted by some vertical applications. The
//! separate type keeps the two on-wire layouts distinguishable at
//! the type level.

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};
use crate::tables::read_tv;
use crate::version::Version;

/// Sanity cap on entry count.
pub const MAX_CUSTOM_DICT_ENTRIES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomDictEntry {
    pub key: String,
    pub value_handle: Handle,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CustomDictEntries {
    pub entries: Vec<CustomDictEntry>,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<CustomDictEntries> {
    let num_entries = c.read_bs_u()? as usize;
    if num_entries > MAX_CUSTOM_DICT_ENTRIES {
        return Err(Error::SectionMap(format!(
            "CustomDictEntries claims {num_entries} entries (>{MAX_CUSTOM_DICT_ENTRIES} sanity cap)"
        )));
    }
    let mut entries = Vec::with_capacity(num_entries);
    for _ in 0..num_entries {
        let key = read_tv(c, version)?;
        let value_handle = c.read_handle()?;
        entries.push(CustomDictEntry { key, value_handle });
    }
    Ok(CustomDictEntries { entries })
}

impl CustomDictEntries {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Handle> {
        self.entries
            .iter()
            .find(|e| e.key == key)
            .map(|e| &e.value_handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn encode_tv_r2000(w: &mut BitWriter, s: &[u8]) {
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
    }

    #[test]
    fn roundtrip_empty() {
        let mut w = BitWriter::new();
        w.write_bs_u(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let e = decode(&mut c, Version::R2000).unwrap();
        assert!(e.is_empty());
    }

    #[test]
    fn roundtrip_populated() {
        let mut w = BitWriter::new();
        w.write_bs_u(2);
        encode_tv_r2000(&mut w, b"SPATIAL_INDEX");
        w.write_handle(2, 0x40);
        encode_tv_r2000(&mut w, b"AcDbVariableDictionary");
        w.write_handle(2, 0x41);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let e = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e.get("SPATIAL_INDEX").unwrap().value, 0x40);
        assert_eq!(e.get("AcDbVariableDictionary").unwrap().value, 0x41);
        assert!(e.get("MISSING").is_none());
    }

    #[test]
    fn lookup_is_case_sensitive() {
        let mut w = BitWriter::new();
        w.write_bs_u(1);
        encode_tv_r2000(&mut w, b"CaseSensitiveKey");
        w.write_handle(2, 0x99);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let e = decode(&mut c, Version::R2000).unwrap();
        assert!(e.get("CaseSensitiveKey").is_some());
        assert!(e.get("casesensitivekey").is_none());
    }

    /// The cap sits above the natural BS upper bound (65535). The
    /// check still exists as a defensive guard if the reader is
    /// ever refactored to use a wider count field.
    #[test]
    fn cap_constant_is_set_conservatively() {
        assert!(MAX_CUSTOM_DICT_ENTRIES >= u16::MAX as usize);
    }
}
