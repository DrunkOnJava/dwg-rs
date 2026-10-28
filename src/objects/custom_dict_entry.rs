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

/// Decodes the entries of a custom dictionary that follow the common object header.
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
    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Looks up the handle stored under `key`.
    pub fn get(&self, key: &str) -> Option<&Handle> {
        self.entries
            .iter()
            .find(|e| e.key == key)
            .map(|e| &e.value_handle)
    }
}
