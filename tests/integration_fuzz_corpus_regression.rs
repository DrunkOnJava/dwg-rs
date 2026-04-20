//! Fuzz-corpus regression test (L4-61, SEC-23 consumer).
//!
//! Replays every seed file in `fuzz/corpus/<target>/` through the
//! target's library entry point and asserts no panic, no hang.
//! Cases that *should* error out via a typed `Result<T, Error>`
//! are fine; panics are not.
//!
//! This test is the regression harness for all four fuzz contracts:
//!   - `lz77::decompress` never panics on arbitrary input
//!   - `BitCursor` primitives never panic on short inputs
//!   - `DwgFile::from_bytes` never panics on adversarial headers
//!   - `ObjectWalker::collect_all_lossy` never panics on garbage payloads
//!   - `reed_solomon::verify` never panics on arbitrary 255-byte codewords
//!
//! When the nightly fuzz CI run produces a new crash artifact, the
//! reduced input lands in `fuzz/corpus/<target>/` and this test picks
//! it up automatically — locking the fix.

use std::fs;
use std::path::{Path, PathBuf};

fn corpus_dir(target: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("fuzz");
    p.push("corpus");
    p.push(target);
    p
}

/// Read every regular file in a corpus directory, or return an empty
/// vector if the directory is absent (corpus lives outside the
/// distributed crate; CI populates it on first run).
fn read_corpus(target: &str) -> Vec<Vec<u8>> {
    let dir = corpus_dir(target);
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() {
                if let Ok(bytes) = fs::read(&p) {
                    out.push(bytes);
                }
            }
        }
    }
    out
}

#[test]
fn lz77_decompress_corpus_no_panic() {
    let seeds = read_corpus("lz77_decompress");
    for bytes in seeds {
        // Result is fine either way — we just forbid panic.
        let _ = dwg::lz77::decompress(&bytes, Some(1024 * 1024));
    }
}

#[test]
fn bitcursor_primitives_corpus_no_panic() {
    let seeds = read_corpus("bitcursor_primitives");
    for bytes in seeds {
        let mut c = dwg::BitCursor::new(&bytes);
        // Exercise each primitive — any of them can error on short
        // inputs; none may panic.
        let _ = c.read_b();
        let _ = c.read_bb();
        let _ = c.read_bs();
        let _ = c.read_bl();
        let _ = c.read_bd();
        let _ = c.read_rc();
        let _ = c.read_rs();
        let _ = c.read_rl();
        let _ = c.read_rd();
    }
}

#[test]
fn dwg_file_open_corpus_no_panic() {
    let seeds = read_corpus("dwg_file_open");
    for bytes in seeds {
        let _ = dwg::DwgFile::from_bytes(bytes);
    }
}

#[test]
fn object_walker_corpus_no_panic() {
    let seeds = read_corpus("object_walker");
    for bytes in seeds {
        if bytes.is_empty() {
            continue;
        }
        let version = match bytes[0] & 0x07 {
            0 => dwg::Version::R14,
            1 => dwg::Version::R2000,
            2 => dwg::Version::R2004,
            3 => dwg::Version::R2007,
            4 => dwg::Version::R2010,
            5 => dwg::Version::R2013,
            _ => dwg::Version::R2018,
        };
        let payload = &bytes[1..];
        let walker = dwg::ObjectWalker::new(payload, version);
        let _ = walker.collect_all_lossy();
    }
}

#[test]
fn rs_fec_corpus_no_panic() {
    let seeds = read_corpus("rs_fec_decode");
    for bytes in seeds {
        let mut codeword = [0u8; dwg::reed_solomon::CODEWORD_BYTES];
        let copy_len = bytes.len().min(dwg::reed_solomon::CODEWORD_BYTES);
        codeword[..copy_len].copy_from_slice(&bytes[..copy_len]);
        let _ = dwg::reed_solomon::verify(&mut codeword);
    }
}

#[test]
fn all_corpora_have_at_least_one_file_or_skip() {
    // Sanity: whichever corpus directories exist, they should have
    // at least one file. An empty directory is a sign of a botched
    // cp/rsync. Missing directories are fine (the corpus is
    // distributed separately from the crate source).
    for target in &[
        "lz77_decompress",
        "bitcursor_primitives",
        "dwg_file_open",
        "object_walker",
        "section_map",
        "classmap_parse",
        "handlemap_parse",
        "header_vars",
        "rs_fec_decode",
    ] {
        let dir = corpus_dir(target);
        if !dir.is_dir() {
            continue;
        }
        let count = fs::read_dir(&dir)
            .map(|it| it.filter_map(Result::ok).count())
            .unwrap_or(0);
        assert!(
            count > 0,
            "corpus {target:?} exists but is empty — did rsync fail?"
        );
    }
    let _ = Path::new(env!("CARGO_MANIFEST_DIR"));
}
