//! End-to-end DWG file writer (Phase H-3 scaffold).
//!
//! This module is the planned inverse of [`crate::reader::DwgFile`]:
//! given a set of named sections and metadata, it lays out a full
//! R2004+ file bit-for-bit readable by any conforming DWG tool
//! (including Autodesk AutoCAD).
//!
//! # Pipeline
//!
//! ```text
//! caller supplies:
//!   version (e.g. Version::R2018)
//!   map of section_name -> decompressed_bytes
//!   metadata (SummaryInfo, AppInfo, ...)
//!           │
//!           ▼
//!   Stage 1 — for each section, call [`section_writer::build_section`]
//!             with a chosen page_offset and section_number.
//!   Stage 2 — assemble the Section Page Map (§4.4) and Section Info
//!             (§4.5) tables listing every section + page offset.
//!   Stage 3 — emit two *system* pages (not data pages) holding the
//!             page map and section info, each with their own 32-byte
//!             header and LZ77 compression.
//!   Stage 4 — write the 0x80-byte file-open header pointing at
//!             those system pages; apply XOR with the 108-byte
//!             magic sequence over bytes 0x80..0xEC.
//!   Stage 5 — produce the final byte buffer:
//!             [0x00..0x80] version magic + CRC-stamped header,
//!             [0x80..0xEC] XOR-masked page-map/section-info
//!                          locators,
//!             [0xEC......] page data + system pages.
//! ```
//!
//! # Current state
//!
//! Stage 1 (per-section compression + framing) is implemented in
//! [`crate::section_writer`]. Stages 2–5 are scaffolded below with
//! a clear panic pathway so callers know the gap, rather than
//! silently producing invalid files.
//!
//! The [`WriterScaffold`] type lets callers experiment with
//! Stage 1 section lists today, validating round-trips against
//! their own test harness, while the later stages are completed.

use crate::error::Result;
use crate::section_writer::{BuiltSection, build_section};
use crate::version::Version;
use std::collections::BTreeMap;

/// Scaffolded DWG file writer. Holds section data and assembles a
/// "sections-only" buffer — NOT a valid DWG file, but sufficient
/// for Stage 1 experimentation and for future Stage 2–5 pipelines
/// to consume.
#[derive(Debug)]
pub struct WriterScaffold {
    sections: BTreeMap<String, Vec<u8>>,
    /// Per-section assigned 1-based number. Filled on `build()`.
    numbers: BTreeMap<String, u32>,
    /// Target version — determines format layout decisions once
    /// Stages 2-5 are implemented.
    pub version: Version,
}

impl WriterScaffold {
    pub fn new(version: Version) -> Self {
        Self {
            sections: BTreeMap::new(),
            numbers: BTreeMap::new(),
            version,
        }
    }

    /// Add a named section's decompressed contents. Overwrites any
    /// previous section with the same name.
    pub fn add_section(&mut self, name: impl Into<String>, bytes: Vec<u8>) {
        self.sections.insert(name.into(), bytes);
    }

    /// Iterate section names in deterministic order.
    pub fn section_names(&self) -> impl Iterator<Item = &str> {
        self.sections.keys().map(|s| s.as_str())
    }

    /// Assign 1-based section numbers in the order sections were
    /// added (via the BTreeMap's key ordering — deterministic).
    /// Returns the list of built sections with their assigned
    /// numbers and page offsets.
    pub fn build_sections(&mut self) -> Result<Vec<NamedBuiltSection>> {
        let mut out = Vec::with_capacity(self.sections.len());
        let mut page_offset: u32 = 0x100; // arbitrary start; Stages 2-5 set the real offset
        for (i, (name, bytes)) in self.sections.iter().enumerate() {
            let number = (i + 1) as u32;
            self.numbers.insert(name.clone(), number);
            let built = build_section(bytes, number, page_offset)?;
            let page_size = built.bytes.len() as u32;
            out.push(NamedBuiltSection {
                name: name.clone(),
                number,
                page_offset,
                built,
            });
            page_offset += page_size;
        }
        Ok(out)
    }
}

/// A built section paired with its scaffold-assigned name + number.
#[derive(Debug, Clone)]
pub struct NamedBuiltSection {
    pub name: String,
    pub number: u32,
    pub page_offset: u32,
    pub built: BuiltSection,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lz77;

    /// Round-trip invariant: every section's built payload
    /// decompresses bit-for-bit back to the original input.
    #[test]
    fn stage1_built_sections_roundtrip_lz77() {
        let mut w = WriterScaffold::new(Version::R2018);
        w.add_section("AcDb:SummaryInfo", b"title\0subject\0".to_vec());
        w.add_section("AcDb:Preview", vec![0xAAu8; 100]);
        w.add_section("AcDb:Header", vec![0x55u8; 500]);

        let built = w.build_sections().unwrap();
        assert_eq!(built.len(), 3);
        for b in &built {
            // Strip the 32-byte header to isolate the LZ77 stream.
            let stream = &b.built.bytes[32..32 + b.built.compressed_size as usize];
            let dec = lz77::decompress(stream, None).unwrap();
            let original = match b.name.as_str() {
                "AcDb:SummaryInfo" => b"title\0subject\0".to_vec(),
                "AcDb:Preview" => vec![0xAAu8; 100],
                "AcDb:Header" => vec![0x55u8; 500],
                other => panic!("unexpected section: {other}"),
            };
            assert_eq!(
                dec, original,
                "{} failed to round-trip after stage-1 build",
                b.name
            );
        }
    }

    #[test]
    fn section_numbers_are_assigned_deterministically() {
        let mut w = WriterScaffold::new(Version::R2018);
        w.add_section("AcDb:Preview", vec![0u8; 4]);
        w.add_section("AcDb:Header", vec![0u8; 4]);
        w.add_section("AcDb:SummaryInfo", vec![0u8; 4]);
        let built = w.build_sections().unwrap();
        // BTreeMap orders alphabetically: Header, Preview, SummaryInfo.
        assert_eq!(built[0].name, "AcDb:Header");
        assert_eq!(built[0].number, 1);
        assert_eq!(built[1].name, "AcDb:Preview");
        assert_eq!(built[1].number, 2);
        assert_eq!(built[2].name, "AcDb:SummaryInfo");
        assert_eq!(built[2].number, 3);
    }
}
