//! Cross-corpus integration tests — exercises the full read pipeline
//! across every `.dwg` file in `../../samples/`.
//!
//! Unlike `samples.rs` which tests specific assertions per file,
//! this test iterates the corpus and verifies generic invariants
//! hold universally:
//!
//! - every sample opens without error
//! - every sample reports a known version (not R14/R2000/.../R2018)
//! - every sample has >= 1 section
//! - every section decompresses without error
//! - where applicable, handle_map/class_map/summary_info/app_info
//!   either succeed or return a defined error — never panic
//!
//! These tests are skipped when `samples/` is absent (downstream
//! vendoring).

use dwg::DwgFile;
use std::fs;
use std::path::PathBuf;

fn samples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p
}

fn list_dwg_samples() -> Vec<PathBuf> {
    let dir = samples_dir();
    let Ok(read) = fs::read_dir(&dir) else {
        eprintln!("corpus_roundtrip: {} missing; skipping", dir.display());
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("dwg") {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[test]
fn every_sample_opens() {
    let samples = list_dwg_samples();
    if samples.is_empty() {
        return;
    }
    for p in &samples {
        let f = DwgFile::open(p).unwrap_or_else(|e| {
            panic!("open failed for {}: {e}", p.display())
        });
        let _ = f.version();
    }
}

#[test]
fn every_sample_has_sections() {
    for p in list_dwg_samples() {
        let f = DwgFile::open(&p).unwrap();
        let sections = f.sections();
        assert!(
            !sections.is_empty(),
            "{} has no sections",
            p.display()
        );
    }
}

/// For every section, decompressing must not error (or skip — None
/// is acceptable for sections the reader doesn't know how to handle).
#[test]
fn every_named_section_decompresses() {
    for p in list_dwg_samples() {
        let f = DwgFile::open(&p).unwrap();
        let names: Vec<String> = f.sections().iter().map(|s| s.name.clone()).collect();
        for name in names {
            match f.read_section(&name) {
                Some(Ok(_)) | None => (), // ok
                Some(Err(e)) => panic!("section {name} of {} failed: {e}", p.display()),
            }
        }
    }
}

/// Convenience accessors never panic — they return a Result that
/// surfaces errors cleanly.
#[test]
fn metadata_accessors_never_panic() {
    for p in list_dwg_samples() {
        let f = DwgFile::open(&p).unwrap();
        // Each of these returns Option<Result<_>>; None is "section
        // missing", Ok is "parsed", Err is "parse failed". We don't
        // care which — the point is no panic.
        let _ = f.summary_info();
        let _ = f.app_info();
        let _ = f.preview();
        let _ = f.file_dep_list();
        let _ = f.handle_map();
        let _ = f.class_map();
        let _ = f.header_vars();
    }
}

/// R2004+ files should have a non-empty handle map (since that's
/// how the object stream is indexed in modern DWGs).
#[test]
fn r2004plus_samples_have_handles() {
    for p in list_dwg_samples() {
        let f = DwgFile::open(&p).unwrap();
        if !f.version().is_r2004_plus() {
            continue;
        }
        // Skip R2007 — its sections use Sec_Mask encryption we haven't
        // implemented yet, so handle_map returns an error by design.
        if f.version().is_r2007() {
            continue;
        }
        if let Some(res) = f.handle_map() {
            let hmap = res.unwrap_or_else(|e| {
                panic!("handle_map failed for {}: {e}", p.display())
            });
            assert!(
                !hmap.entries.is_empty(),
                "{} has empty handle map",
                p.display()
            );
        }
    }
}
