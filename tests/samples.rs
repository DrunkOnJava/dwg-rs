//! Integration tests against the 19-file DWG sample corpus at
//! `../../samples/`. The corpus was assembled from the public
//! `nextgis/dwg_samples` repository (AutoCAD R14 → 2018) plus a 1 MB
//! AC1032 sample.
//!
//! Tests skip gracefully if samples are absent — useful when this crate
//! is vendored into a build system that doesn't carry the corpus.

use dwg::section::SectionKind;
use dwg::{DwgFile, Version};
use std::path::PathBuf;

fn samples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p
}

fn sample(name: &str) -> PathBuf {
    let mut p = samples_dir();
    p.push(name);
    p
}

fn open_if_present(name: &str) -> Option<DwgFile> {
    let p = sample(name);
    if !p.exists() {
        eprintln!("skipping {name}: sample not present");
        return None;
    }
    Some(DwgFile::open(&p).unwrap_or_else(|e| panic!("{name} open failed: {e}")))
}

// ================================================================
// Version detection — one assertion per released format.
// ================================================================

#[test]
fn arc_r14_is_ac1014() {
    if let Some(f) = open_if_present("arc_R14.dwg") {
        assert_eq!(f.version(), Version::R14);
        assert_eq!(&f.version().magic(), b"AC1014");
    }
}

#[test]
fn arc_2000_is_ac1015() {
    if let Some(f) = open_if_present("arc_2000.dwg") {
        assert_eq!(f.version(), Version::R2000);
        assert_eq!(&f.version().magic(), b"AC1015");
    }
}

#[test]
fn arc_2004_is_ac1018() {
    if let Some(f) = open_if_present("arc_2004.dwg") {
        assert_eq!(f.version(), Version::R2004);
        assert_eq!(&f.version().magic(), b"AC1018");
    }
}

#[test]
fn arc_2007_is_ac1021() {
    if let Some(f) = open_if_present("arc_2007.dwg") {
        assert_eq!(f.version(), Version::R2007);
        assert_eq!(&f.version().magic(), b"AC1021");
        // Phase A: R2007 has a deferred layout; we only populate the
        // common header, not the R2004-family struct.
        assert!(
            f.r2007_common().is_some(),
            "R2007 should populate r2007_common"
        );
        assert!(
            f.r2004_header().is_none(),
            "R2007 must NOT populate r2004_header"
        );
    }
}

#[test]
fn arc_2010_is_ac1024() {
    if let Some(f) = open_if_present("arc_2010.dwg") {
        assert_eq!(f.version(), Version::R2010);
    }
}

#[test]
fn arc_2013_is_ac1027() {
    if let Some(f) = open_if_present("arc_2013.dwg") {
        assert_eq!(f.version(), Version::R2013);
    }
}

#[test]
fn sample_ac1032_is_r2018() {
    if let Some(f) = open_if_present("sample_AC1032.dwg") {
        assert_eq!(f.version(), Version::R2018);
        assert_eq!(&f.version().magic(), b"AC1032");
    }
}

// ================================================================
// R13-R15 header details
// ================================================================

#[test]
fn r14_has_locator_records() {
    if let Some(f) = open_if_present("arc_R14.dwg") {
        let h = f.r13_header().expect("R14 must parse R13R15 header");
        // Spec remark §3.2.6: seen with 3..=6 records. We expect at least 3.
        assert!(
            h.locator_count >= 3,
            "unexpected locator_count = {}",
            h.locator_count
        );
        assert_eq!(h.locators.len(), h.locator_count as usize);
        // Record 0 is always "Header variables".
        let hdr_rec = h.locators.iter().find(|r| r.number == 0);
        assert!(hdr_rec.is_some(), "R14 should have header record (0)");
    }
}

#[test]
fn r2000_has_classes_and_handles_sections() {
    if let Some(f) = open_if_present("arc_2000.dwg") {
        assert!(
            f.section_of_kind(SectionKind::Header).is_some(),
            "expected Header section"
        );
        assert!(
            f.section_of_kind(SectionKind::Classes).is_some(),
            "expected Classes section"
        );
        assert!(
            f.section_of_kind(SectionKind::Handles).is_some(),
            "expected Handles section (object map)"
        );
    }
}

// ================================================================
// R2004+ header details
// ================================================================

#[test]
fn ac1032_decrypts_file_id() {
    if let Some(f) = open_if_present("sample_AC1032.dwg") {
        let h = f.r2004_header().expect("R2018 must parse R2004 header");
        let id = &h.file_id[..11];
        assert_eq!(id, b"AcFssFcAJMB", "decrypt failed");
    }
}

#[test]
fn ac1032_has_nonzero_section_page_map() {
    if let Some(f) = open_if_present("sample_AC1032.dwg") {
        let h = f.r2004_header().unwrap();
        assert!(
            h.section_page_map_addr > 0,
            "section_page_map_addr must point somewhere"
        );
        assert!(
            h.section_page_amount >= 1,
            "must have at least one section page"
        );
    }
}

// ================================================================
// CRC-32 validation — spec §4.1 says the decrypted block's CRC-32,
// with bytes 0x68..0x6C zeroed, must equal the stored value.
// ================================================================

#[test]
fn ac1032_header_crc_matches() {
    if let Some(f) = open_if_present("sample_AC1032.dwg") {
        let bytes = f.raw_bytes();
        let (expected, actual) = dwg::reader::validate_r2004_header_crc(bytes).unwrap();
        assert_eq!(
            expected, actual,
            "R2004+ header CRC mismatch: stored={expected:#x}, computed={actual:#x}"
        );
    }
}

// ================================================================
// Every entity file in the corpus opens without error.
// ================================================================

#[test]
fn all_corpus_files_open() {
    let dir = samples_dir();
    if !dir.exists() {
        eprintln!("skipping corpus sweep: {} does not exist", dir.display());
        return;
    }
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("dwg"))
        .collect();
    if entries.is_empty() {
        eprintln!("skipping corpus sweep: no .dwg files under {}", dir.display());
        return;
    }
    let mut failures = Vec::new();
    for e in &entries {
        if let Err(err) = DwgFile::open(e.path()) {
            failures.push(format!("{}: {err}", e.path().display()));
        }
    }
    assert!(
        failures.is_empty(),
        "{} / {} files failed to open:\n{}",
        failures.len(),
        entries.len(),
        failures.join("\n")
    );
}

// ================================================================
// Every version in the corpus reports the section types we expect.
// ================================================================

#[test]
fn every_file_reports_some_sections() {
    let dir = samples_dir();
    if !dir.exists() {
        return;
    }
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if e.path().extension().and_then(|s| s.to_str()) != Some("dwg") {
            continue;
        }
        let f = DwgFile::open(e.path()).expect("open");
        assert!(
            !f.sections().is_empty(),
            "{:?} reports zero sections — reader is broken",
            e.path().file_name()
        );
    }
}

// ================================================================
// Phase B: named-section enumeration via LZ77 + Section Map walk
// ================================================================

#[test]
fn ac1032_enumerates_named_sections() {
    let Some(f) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    let names: Vec<&str> = f.sections().iter().map(|s| s.name.as_str()).collect();
    // After Phase B wiring we expect the canonical AcDb: names.
    let must_have = [
        "AcDb:Header",
        "AcDb:Classes",
        "AcDb:Handles",
        "AcDb:AcDbObjects",
    ];
    for expected in must_have {
        assert!(
            names.iter().any(|n| *n == expected),
            "expected section {:?} not found. Got: {:?}",
            expected,
            names
        );
    }
}

#[test]
fn ac1032_sections_have_nonzero_sizes() {
    let Some(f) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    // The critical named sections (AcDb:Header, AcDb:AcDbObjects) must
    // have real size data from the section-info table, not a stub 0.
    for name in ["AcDb:Header", "AcDb:AcDbObjects"] {
        let Some(s) = f.section_by_name(name) else {
            panic!("section {name:?} missing from enumeration");
        };
        assert!(
            s.size > 0,
            "section {name:?} reports size=0, Phase B not wired?"
        );
    }
}

#[test]
fn ac1032_preview_is_classified() {
    let Some(f) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    if let Some(preview) = f.section_of_kind(SectionKind::Preview) {
        assert_eq!(preview.name, "AcDb:Preview");
    }
}

// ================================================================
// The R13-R15 and R2004+ code paths should NEVER both activate.
// ================================================================

#[test]
fn header_paths_are_mutually_exclusive() {
    let dir = samples_dir();
    if !dir.exists() {
        return;
    }
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if e.path().extension().and_then(|s| s.to_str()) != Some("dwg") {
            continue;
        }
        let f = DwgFile::open(e.path()).unwrap();
        let r13 = f.r13_header().is_some();
        let r24 = f.r2004_header().is_some();
        let r27 = f.r2007_common().is_some();
        let n = [r13, r24, r27].iter().filter(|&&b| b).count();
        assert_eq!(
            n,
            1,
            "{:?}: exactly one of r13/r2004/r2007 must be populated (got {})",
            e.path().file_name(),
            n
        );
        match f.version() {
            v if v.is_r13_r15() => assert!(r13),
            v if v.is_r2007() => assert!(r27),
            v if v.is_r2004_family() => assert!(r24),
            v => panic!("unexpected version classification for {:?}", v),
        }
    }
}
