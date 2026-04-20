//! Custom-class definitions beyond the built-in [`crate::ObjectType`]
//! enum (spec §5.7).
//!
//! The class map is the DWG's registry of *non-fixed* object types —
//! entity/object classes loaded at runtime by AutoCAD's ARX modules
//! or by vertical applications (Architecture, MEP, Civil 3D). Each
//! entry records enough metadata for a consumer to decide whether it
//! recognises the class and, if not, at least preserve it as an
//! opaque proxy.
//!
//! # Per-entry stream shape
//!
//! ```text
//! BS    class_number         -- ≥ 500; dwg-rs' built-in enum occupies 1..=498
//! BS    proxy_flags          -- bitfield, see spec §5.7
//! TV    app_name             -- ARX module that owns the class
//! TV    cpp_class_name       -- in-AutoCAD C++ class name
//! TV    dxf_record_name      -- DXF "RECORD NAME" string
//! B     was_zombie           -- R2000+ only; true if class previously erased
//! BS    item_class_id        -- class category (0x1F2 = entity, 0x1F3 = object)
//! ```
//!
//! # Why class_number ≥ 500
//!
//! AutoCAD reserves 1..=498 for the fixed built-in types listed in
//! spec §3.1 and mirrored by [`crate::ObjectType`]. Any class number
//! below 500 here indicates a malformed or tampered file.
//!
//! # Safety caps
//!
//! Entry count capped at [`MAX_CLASS_MAP_EXTENSIONS`] — real drawings
//! rarely exceed a few dozen custom classes.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::tables::read_tv;
use crate::version::Version;

/// Sanity cap on custom-class extension entries.
pub const MAX_CLASS_MAP_EXTENSIONS: usize = 4096;

/// Minimum class number a custom-class entry is allowed to claim;
/// 1..=498 are reserved for the fixed built-in enum.
pub const MIN_CUSTOM_CLASS_NUMBER: u16 = 500;

/// A single custom-class definition parsed from the class map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMapExtensionEntry {
    pub class_number: u16,
    pub proxy_flags: u16,
    pub app_name: String,
    pub cpp_class_name: String,
    pub dxf_record_name: String,
    pub was_zombie: bool,
    pub item_class_id: u16,
}

/// Decoded collection of custom-class entries. The outer object
/// stream carries the entry count; this decoder consumes exactly
/// `num_entries` records.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassMapExtension {
    pub entries: Vec<ClassMapExtensionEntry>,
}

pub fn decode(
    c: &mut BitCursor<'_>,
    version: Version,
    num_entries: usize,
) -> Result<ClassMapExtension> {
    if num_entries > MAX_CLASS_MAP_EXTENSIONS {
        return Err(Error::SectionMap(format!(
            "ClassMapExtension claims {num_entries} entries (>{MAX_CLASS_MAP_EXTENSIONS} sanity cap)"
        )));
    }
    let mut entries = Vec::with_capacity(num_entries);
    for _ in 0..num_entries {
        entries.push(decode_entry(c, version)?);
    }
    Ok(ClassMapExtension { entries })
}

fn decode_entry(c: &mut BitCursor<'_>, version: Version) -> Result<ClassMapExtensionEntry> {
    let class_number = c.read_bs_u()?;
    if class_number < MIN_CUSTOM_CLASS_NUMBER {
        return Err(Error::SectionMap(format!(
            "ClassMapExtension entry class_number {class_number} < {MIN_CUSTOM_CLASS_NUMBER} \
             (reserved for built-in types per spec §3.1)"
        )));
    }
    let proxy_flags = c.read_bs_u()?;
    let app_name = read_tv(c, version)?;
    let cpp_class_name = read_tv(c, version)?;
    let dxf_record_name = read_tv(c, version)?;
    let was_zombie = if version == Version::R14 {
        false
    } else {
        c.read_b()?
    };
    let item_class_id = c.read_bs_u()?;
    Ok(ClassMapExtensionEntry {
        class_number,
        proxy_flags,
        app_name,
        cpp_class_name,
        dxf_record_name,
        was_zombie,
        item_class_id,
    })
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
    fn roundtrip_empty_extension() {
        let mut w = BitWriter::new();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ext = decode(&mut c, Version::R2000, 0).unwrap();
        assert!(ext.entries.is_empty());
    }

    #[test]
    fn roundtrip_single_entry() {
        let mut w = BitWriter::new();
        w.write_bs_u(500); // class_number
        w.write_bs_u(0x1F); // proxy_flags
        encode_tv_r2000(&mut w, b"AcDbLmX");
        encode_tv_r2000(&mut w, b"AcDbLmXClass");
        encode_tv_r2000(&mut w, b"LMX_RECORD");
        w.write_b(false); // was_zombie
        w.write_bs_u(0x1F2); // item_class_id (entity)
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ext = decode(&mut c, Version::R2000, 1).unwrap();
        assert_eq!(ext.entries.len(), 1);
        let e = &ext.entries[0];
        assert_eq!(e.class_number, 500);
        assert_eq!(e.proxy_flags, 0x1F);
        assert_eq!(e.app_name, "AcDbLmX");
        assert_eq!(e.cpp_class_name, "AcDbLmXClass");
        assert_eq!(e.dxf_record_name, "LMX_RECORD");
        assert!(!e.was_zombie);
        assert_eq!(e.item_class_id, 0x1F2);
    }

    #[test]
    fn r14_skips_was_zombie() {
        let mut w = BitWriter::new();
        w.write_bs_u(501);
        w.write_bs_u(0);
        encode_tv_r2000(&mut w, b"A");
        encode_tv_r2000(&mut w, b"B");
        encode_tv_r2000(&mut w, b"C");
        // no was_zombie bit for R14
        w.write_bs_u(0x1F3);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ext = decode(&mut c, Version::R14, 1).unwrap();
        assert!(!ext.entries[0].was_zombie);
        assert_eq!(ext.entries[0].item_class_id, 0x1F3);
    }

    #[test]
    fn rejects_reserved_class_number() {
        let mut w = BitWriter::new();
        w.write_bs_u(499); // one below the cutoff
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000, 1).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("reserved")));
    }

    #[test]
    fn rejects_excessive_entry_count() {
        let mut w = BitWriter::new();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000, MAX_CLASS_MAP_EXTENSIONS + 1).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("ClassMapExtension")));
    }
}
