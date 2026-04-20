//! DWG file writer — **Stage 1 only**.
//!
//! This module is the *planned* inverse of [`crate::reader::DwgFile`].
//! A complete writer has five stages; only Stage 1 exists today.
//!
//! # What this module does today
//!
//! [`WriterScaffold`] collects named sections, assigns deterministic
//! 1-based section numbers, and returns a
//! `Vec<NamedBuiltSection>` where each element is a
//! 32-byte-aligned page ([`crate::section_writer::BuiltSection`]) that
//! decompresses back to the original decompressed bytes. It does
//! **not** assemble those pages into a complete DWG byte buffer —
//! that's Stages 2–5 below.
//!
//! If you need a single round-trippable `Vec<u8>` today, that is not
//! yet available from this crate. This module exists as the
//! infrastructure target Stages 2–5 will build on, and as a
//! round-trip correctness harness for the section-level framing.
//!
//! # The five-stage pipeline
//!
//! ```text
//!   caller supplies:
//!     version (e.g. Version::R2018)
//!     map of section_name -> decompressed_bytes
//!     metadata (SummaryInfo, AppInfo, ...)
//!             │
//!             ▼
//!   Stage 1 — [IMPLEMENTED] for each section, call
//!             section_writer::build_section with a chosen
//!             page_offset and section_number.
//!     ═══════════════════════════════════════════════════════════
//!   Stage 2 — [NOT IMPLEMENTED] assemble the Section Page Map
//!             (§4.4) and Section Info (§4.5) tables.
//!   Stage 3 — [NOT IMPLEMENTED] emit two *system* pages (not data
//!             pages) holding the page map and section info, each
//!             with their own 32-byte header and LZ77 compression.
//!   Stage 4 — [NOT IMPLEMENTED] write the 0x80-byte file-open
//!             header pointing at those system pages; apply XOR
//!             with the 108-byte magic sequence over bytes
//!             0x80..0xEC.
//!   Stage 5 — [NOT IMPLEMENTED] produce the final byte buffer:
//!             [0x00..0x80] version magic + CRC-stamped header,
//!             [0x80..0xEC] XOR-masked page-map/section-info locators,
//!             [0xEC......] page data + system pages.
//! ```
//!
//! A method like `DwgFile::to_bytes()` would require all five stages;
//! that API is deferred until Stages 2-5 ship.

use crate::error::{Error, Result};
use crate::section_writer::{BuiltSection, build_section};
use crate::version::Version;
use std::collections::BTreeMap;
use std::path::Path;

/// Stage-1 writer — collects named sections + decompressed byte
/// payloads, emits a `Vec<NamedBuiltSection>` where each element is
/// a framed 32-byte-aligned page.
///
/// Does NOT emit a complete DWG file. Does NOT assemble a single
/// buffer. The returned list is the *input* to Stages 2-5 of a future
/// full writer. Until those stages ship, this type is useful for:
/// - Round-trip testing per-section LZ77 + Sec_Mask framing.
/// - Building custom writers that patch specific sections in-place
///   inside an existing valid DWG file.
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

// ================================================================
// P0-11 — stream-name validation pre-write
// ================================================================

/// The set of DWG section names recognized by the reader + writer.
///
/// Per ODA Open Design Specification §3.2 and §4.5 the file layer
/// identifies sections by these fixed ASCII strings. Passing an
/// unrecognized name into the writer at section-add time used to be
/// silently accepted and would produce a file whose own reader
/// could not locate the section back. [`validate_section_name`]
/// makes that case an error upfront.
///
/// New entries should only be added when an authoritative spec
/// reference says a new section is defined — the reader must know
/// how to look up the section too, otherwise the round-trip fails.
pub const KNOWN_SECTION_NAMES: &[&str] = &[
    "AcDb:AcDbObjects",
    "AcDb:AppInfo",
    "AcDb:AppInfoHistory",
    "AcDb:AuxHeader",
    "AcDb:Classes",
    "AcDb:FileDepList",
    "AcDb:Handles",
    "AcDb:Header",
    "AcDb:ObjFreeSpace",
    "AcDb:ObjectsSection",
    "AcDb:Preview",
    "AcDb:RevHistory",
    "AcDb:Security",
    "AcDb:Signature",
    "AcDb:SummaryInfo",
    "AcDb:Template",
];

/// Validate a section name against [`KNOWN_SECTION_NAMES`] before
/// handing it to the writer. Returns
/// [`Error::Unsupported`] with a helpful message on typos or
/// custom names.
///
/// Callers that need to emit a non-standard section (e.g. during
/// reverse-engineering experiments) can bypass this check by
/// inserting directly into [`WriterScaffold`]'s backing map — but
/// the round-trip won't work if the reader doesn't recognize the
/// name.
pub fn validate_section_name(name: &str) -> Result<()> {
    if KNOWN_SECTION_NAMES.contains(&name) {
        Ok(())
    } else {
        Err(Error::Unsupported {
            feature: format!(
                "unknown section name {name:?}; expected one of the ODA-spec'd \
                 AcDb:* names (KNOWN_SECTION_NAMES). Typos in section names \
                 silently corrupt round-trip — reject at write time"
            ),
        })
    }
}

// ================================================================
// L12-03 — per-version magic byte + file-header writer
// ================================================================

/// Return the 6-byte ASCII `$ACADVER` magic for a given DWG version
/// (spec §3.1, first 6 bytes of the file).
///
/// | Version      | Magic  |
/// |--------------|--------|
/// | R14          | AC1014 |
/// | R2000..R2002 | AC1015 |
/// | R2004..R2006 | AC1018 |
/// | R2007..R2009 | AC1021 |
/// | R2010..R2012 | AC1024 |
/// | R2013..R2017 | AC1027 |
/// | R2018+       | AC1032 |
pub fn version_magic_bytes(version: Version) -> [u8; 6] {
    let s: &[u8; 6] = match version {
        Version::R14 => b"AC1014",
        Version::R2000 => b"AC1015",
        Version::R2004 => b"AC1018",
        Version::R2007 => b"AC1021",
        Version::R2010 => b"AC1024",
        Version::R2013 => b"AC1027",
        Version::R2018 => b"AC1032",
    };
    *s
}

/// Build the first 16 bytes of a DWG file — the "version string"
/// region per spec §3.1:
///
/// ```text
/// [0x00..0x06]  6 ASCII bytes  — $ACADVER magic (e.g. "AC1032")
/// [0x06..0x0B]  5 bytes 0x00   — reserved
/// [0x0B]        1 byte         — maintenance release (0)
/// [0x0C..0x0D]  1 byte 0x00    — reserved
/// [0x0D]        1 byte 0x1F    — marker (0x1F on R2004+, 0x00 older)
/// [0x0E..0x10]  2 bytes 0x00   — reserved
/// ```
///
/// Downstream stages (Section Page Map, file-open header, XOR magic)
/// are added by Stages 2-5 of the writer pipeline. This function
/// produces only the leading 16-byte ACADVER block.
pub fn build_version_header(version: Version) -> [u8; 16] {
    let mut out = [0u8; 16];
    let magic = version_magic_bytes(version);
    out[0..6].copy_from_slice(&magic);
    // [6..11] stays zero.
    // [11] maintenance release — 0 for now.
    // [12] stays zero.
    // [13] marker: 0x1F on R2004+, 0x00 older.
    out[13] = if matches!(
        version,
        Version::R2004 | Version::R2007 | Version::R2010 | Version::R2013 | Version::R2018
    ) {
        0x1F
    } else {
        0x00
    };
    // [14..16] stays zero.
    out
}

// ================================================================
// P0-10 — atomic write via temp + rename
// ================================================================

/// Write bytes to `path` atomically: first write to a sibling
/// temporary file, then rename into place. Rename is atomic on all
/// POSIX + NTFS platforms for same-volume targets — consumers that
/// cross volumes should opt into a copy+unlink fallback.
///
/// On success the target file contains exactly `bytes`. On failure
/// the target is unchanged; the temp file is deleted if it exists.
///
/// The temp file is named `<target>.tmp-<pid>` to avoid collisions
/// with other processes writing the same target.
///
/// # Errors
///
/// Returns [`Error::Io`] for any filesystem failure. Unlike a naive
/// `File::create + write_all` pair, an interrupted atomic write
/// never leaves a half-written target behind.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::fs;
    use std::io::Write as IoWrite;

    let parent = path.parent().ok_or_else(|| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("atomic_write: path {:?} has no parent directory", path),
        ))
    })?;

    let pid = std::process::id();
    let temp_name = format!(
        "{}.tmp-{pid}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("dwg-rs-atomic")
    );
    let temp_path = parent.join(&temp_name);

    // Scope the File so it's closed before rename.
    {
        let mut f = fs::File::create(&temp_path).map_err(Error::Io)?;
        let write_err = f.write_all(bytes).err();
        let sync_err = f.sync_all().err();
        drop(f);
        if let Some(e) = write_err.or(sync_err) {
            let _ = fs::remove_file(&temp_path);
            return Err(Error::Io(e));
        }
    }

    // Atomic rename (POSIX + NTFS same-volume guarantee).
    if let Err(e) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(Error::Io(e));
    }
    Ok(())
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

    // ---- L12-03: version magic + file-header writer ----

    #[test]
    fn version_magic_matches_spec_table() {
        assert_eq!(&version_magic_bytes(Version::R14), b"AC1014");
        assert_eq!(&version_magic_bytes(Version::R2000), b"AC1015");
        assert_eq!(&version_magic_bytes(Version::R2004), b"AC1018");
        assert_eq!(&version_magic_bytes(Version::R2007), b"AC1021");
        assert_eq!(&version_magic_bytes(Version::R2010), b"AC1024");
        assert_eq!(&version_magic_bytes(Version::R2013), b"AC1027");
        assert_eq!(&version_magic_bytes(Version::R2018), b"AC1032");
    }

    #[test]
    fn version_header_first_6_bytes_are_ascii_magic() {
        let header = build_version_header(Version::R2018);
        assert_eq!(&header[0..6], b"AC1032");
        // Reserved bytes [6..11] must be zero.
        assert!(header[6..11].iter().all(|&b| b == 0));
    }

    #[test]
    fn version_header_marker_is_0x1f_on_r2004_plus() {
        assert_eq!(build_version_header(Version::R14)[13], 0x00);
        assert_eq!(build_version_header(Version::R2000)[13], 0x00);
        assert_eq!(build_version_header(Version::R2004)[13], 0x1F);
        assert_eq!(build_version_header(Version::R2018)[13], 0x1F);
    }

    #[test]
    fn version_header_is_exactly_16_bytes() {
        let header = build_version_header(Version::R2000);
        assert_eq!(header.len(), 16);
    }

    // ---- P0-10: atomic write ----

    #[test]
    fn atomic_write_creates_target_with_exact_bytes() {
        let tmp_dir = std::env::temp_dir();
        let target = tmp_dir.join(format!("dwg-rs-atomic-test-{}.bin", std::process::id()));
        let payload = b"\xACtest atomic write";

        atomic_write(&target, payload).expect("atomic_write must succeed");
        let read_back = std::fs::read(&target).expect("target must exist after atomic_write");
        assert_eq!(read_back, payload);

        std::fs::remove_file(&target).ok();
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let tmp_dir = std::env::temp_dir();
        let target = tmp_dir.join(format!(
            "dwg-rs-atomic-overwrite-{}.bin",
            std::process::id()
        ));

        std::fs::write(&target, b"old contents").expect("seed write");
        atomic_write(&target, b"new contents").expect("atomic overwrite");

        let read_back = std::fs::read(&target).unwrap();
        assert_eq!(read_back, b"new contents");

        std::fs::remove_file(&target).ok();
    }

    #[test]
    fn atomic_write_rejects_path_with_no_parent() {
        // A single-component path on most platforms has no parent
        // directory other than the CWD — but Path::parent() returns
        // Some("") for "filename". So use the empty path as a
        // pathological case.
        let bad = Path::new("");
        let err = atomic_write(bad, b"anything").unwrap_err();
        assert!(matches!(err, Error::Io(_)));
    }

    // ---- P0-11: stream-name validation ----

    #[test]
    fn validate_section_name_accepts_spec_names() {
        assert!(validate_section_name("AcDb:Header").is_ok());
        assert!(validate_section_name("AcDb:SummaryInfo").is_ok());
        assert!(validate_section_name("AcDb:AcDbObjects").is_ok());
        assert!(validate_section_name("AcDb:Classes").is_ok());
    }

    #[test]
    fn validate_section_name_rejects_typos() {
        // Common mis-spellings: lowercase db, missing colon, etc.
        let err = validate_section_name("acdb:Header").unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }));
        let err = validate_section_name("AcDb.Header").unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }));
        let err = validate_section_name("Header").unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }));
    }

    #[test]
    fn validate_section_name_rejects_empty() {
        assert!(validate_section_name("").is_err());
    }

    #[test]
    fn known_section_names_are_unique() {
        let mut seen: Vec<&str> = KNOWN_SECTION_NAMES.to_vec();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), KNOWN_SECTION_NAMES.len(),
            "KNOWN_SECTION_NAMES has duplicates");
    }

    #[test]
    fn atomic_write_leaves_no_temp_file_on_success() {
        let tmp_dir = std::env::temp_dir();
        let target = tmp_dir.join(format!(
            "dwg-rs-atomic-cleanup-{}.bin",
            std::process::id()
        ));
        atomic_write(&target, b"clean").unwrap();

        // No sibling .tmp-<pid> file should remain.
        let pid = std::process::id();
        let orphan = tmp_dir.join(format!(
            "dwg-rs-atomic-cleanup-{pid}.bin.tmp-{pid}"
        ));
        assert!(!orphan.exists(), "temp file leaked: {orphan:?}");

        std::fs::remove_file(&target).ok();
    }
}
