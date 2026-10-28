//! The insert-count run of every BLOCK_HEADER, against the handle stream (§20.4.52, #386).
//!
//! # What this proves
//!
//! §20.4.52 puts, after the base point and the xref path, "a sequence of zero or
//! more non-zero RC's, followed by a terminating 0 RC. The total number of these
//! indicates how many insert handles will be present." The decoder once read at
//! most 257 of them, so a block with more inserts than that misread everything after
//! the run (the description, the preview size) and the record errored.
//!
//! For every BLOCK_HEADER of every DWG given, the probe decodes the record and
//! checks the run against the record's own handle stream, which holds the block
//! control, the reactors, the extension dictionary, a NULL, the owned entities
//! (R2004 and later, `Owned Object Count` of them), the ENDBLK, one handle per
//! insert and the layout (and on R2000 the first and last entity of a block that
//! is not an xref). The record agrees when the handles after the extension
//! dictionary number exactly that sum, an independent count of the same run that
//! reads none of its bytes.
//!
//! A record that does not decode, or that decodes with a handle stream that does
//! not agree, exits 1. Records are tallied by release, with how many carry a run
//! longer than the 256 the decoder used to stop at and the longest run seen; with
//! no BLOCK_HEADER at all it prints `NOT MEASURED` and exits 0.
//!
//! # How to verify
//!
//! ```text
//! cargo run --release --example probe_block_header_inserts -- <file-or-dir> ...
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dwg::entities::DecodedEntity;
use dwg::{DwgFile, ObjectType, Version};

#[derive(Default)]
struct Tally {
    records: usize,
    decoded: usize,
    agree: usize,
    long_runs: usize,
    longest: usize,
}

fn walk(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        entries.sort();
        for e in entries {
            walk(&e, out);
        }
    } else if path
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("dwg"))
    {
        out.push(path.to_path_buf());
    }
}

fn main() -> ExitCode {
    let mut paths: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        paths.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples"));
    }
    let mut dwgs = Vec::new();
    for p in &paths {
        walk(p, &mut dwgs);
    }
    dwgs.sort();
    dwgs.dedup();
    let mut by: BTreeMap<String, Tally> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    let mut long: Vec<String> = Vec::new();
    for path in &dwgs {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let Ok(file) = DwgFile::open(path) else {
            continue;
        };
        let version = file.version();
        if matches!(version, Version::R14) {
            continue;
        }
        let Some(Ok(objects)) = file.all_objects() else {
            continue;
        };
        let t = by.entry(format!("{version}")).or_default();
        for raw in objects.iter().filter(|r| r.kind == ObjectType::BlockHeader) {
            t.records += 1;
            let DecodedEntity::BlockRecord(b) = dwg::entities::decode_from_raw(raw, version) else {
                failures.push(format!(
                    "{name} {:X}: BLOCK_HEADER does not decode",
                    raw.handle.value
                ));
                continue;
            };
            t.decoded += 1;
            t.longest = t.longest.max(b.insert_count);
            if b.insert_count > 256 {
                t.long_runs += 1;
                long.push(format!(
                    "{name} {:X} {:?}: {} insert handles, {} owned objects",
                    raw.handle.value,
                    b.header.name,
                    b.insert_count,
                    b.num_owned_objects.unwrap_or(0)
                ));
            }
            let Ok(refs) = dwg::tables::handles::refs(raw, version) else {
                failures.push(format!(
                    "{name} {:X}: handle stream does not read",
                    raw.handle.value
                ));
                continue;
            };
            let mut expected = 2;
            if matches!(version, Version::R2000) && !b.is_xref && !b.xref_overlay {
                expected += 2;
            }
            if version.is_r2004_plus() {
                expected += b.num_owned_objects.unwrap_or(0) as usize;
            }
            expected += 1 + b.insert_count + 1;
            if refs.rest.len() == expected {
                t.agree += 1;
            } else {
                failures.push(format!(
                    "{name} {:X}: {} insert handles in the run, {} handles after the extension dictionary against {expected} expected",
                    raw.handle.value,
                    b.insert_count,
                    refs.rest.len()
                ));
            }
        }
    }
    println!(
        "{:14} {:>8} {:>8} {:>8} {:>10} {:>8}",
        "release", "records", "decoded", "agree", "run > 256", "longest"
    );
    for (release, t) in &by {
        println!(
            "{release:14} {:>8} {:>8} {:>8} {:>10} {:>8}",
            t.records, t.decoded, t.agree, t.long_runs, t.longest
        );
    }
    for l in &long {
        println!("run > 256: {l}");
    }
    for f in &failures {
        println!("FAILED {f}");
    }
    if by.values().all(|t| t.records == 0) && failures.is_empty() {
        println!("NOT MEASURED: no BLOCK_HEADER in any drawing under {paths:?}");
    }
    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
