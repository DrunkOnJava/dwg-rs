//! Honest coverage report — runs the entity dispatcher against every
//! DWG file in a directory and prints per-file + aggregate
//! decoded/unhandled/errored counts. No rationalization, no rounding
//! up, no excuses.
//!
//! ```bash
//! cargo run --release --example coverage_report -- path/to/corpus/
//! ```
//!
//! This example intentionally does NOT dump per-entity field values.
//! Its audience is CI (the coverage-smoke job calls it) and humans
//! wanting a quick corpus-wide summary; printing every decoded
//! value would bury the summary in noise. For per-entity field
//! inspection, see the sibling example
//! [`dump_decoded_entities`](../examples/dump_decoded_entities.rs).

use dwg::entities::DecodedEntity;
use dwg::{DwgFile, ObjectType, entities::DispatchSummary};
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(dir_arg) = env::args().nth(1) else {
        eprintln!("usage: coverage_report <directory-of-dwg-files>");
        return ExitCode::FAILURE;
    };
    let dir = PathBuf::from(dir_arg);
    let Ok(read) = std::fs::read_dir(&dir) else {
        eprintln!("cannot read directory {}", dir.display());
        return ExitCode::FAILURE;
    };

    let mut files: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dwg"))
        .collect();
    files.sort();

    if files.is_empty() {
        eprintln!("no .dwg files under {}", dir.display());
        return ExitCode::FAILURE;
    }

    let mut totals = DispatchSummary::default();
    let mut type_histo: BTreeMap<String, usize> = BTreeMap::new();
    let mut unhandled_histo: BTreeMap<String, usize> = BTreeMap::new();
    // Walker diagnostics are tracked separately from dispatch counts:
    // a handle-map entry the walker could not turn into a record never
    // reaches a decoder, so folding it into `err` would make coverage
    // ratios incomparable across releases.
    let mut walk_skips: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_walk_skips = 0usize;
    let mut total_handle_mismatches = 0usize;

    println!(
        "{:<32} {:<12} {:>6} {:>6} {:>6} {:>6} {:>7}",
        "file", "version", "deco", "skip", "err", "wskip", "ratio%"
    );
    println!("{}", "-".repeat(87));

    for path in &files {
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("(unnamed)");
        let file = match DwgFile::open(path) {
            Ok(f) => f,
            Err(e) => {
                println!("{:<32} open-failed: {e}", filename);
                continue;
            }
        };
        let version = file.version();
        let (entities, summary) = match file.decoded_entities() {
            Some(Ok((e, s))) => (e, s),
            Some(Err(e)) => {
                println!("{:<32} decoded_entities-failed: {e}", filename);
                continue;
            }
            None => {
                println!(
                    "{:<32} {:<12} n/a    n/a    n/a    n/a       (no-handle-map)",
                    filename,
                    format!("{version}")
                );
                continue;
            }
        };

        // Walker-level diagnostic pass: how many handle-map entries did
        // not resolve to a record at all?
        let file_walk_skips = match file.all_objects_lossy() {
            Some(Ok((_, walk))) => {
                for entry in &walk.skipped {
                    *walk_skips.entry(classify_skip(&entry.reason)).or_default() += 1;
                }
                total_handle_mismatches += walk.handle_mismatches.len();
                walk.skipped.len()
            }
            _ => 0,
        };
        total_walk_skips += file_walk_skips;

        println!(
            "{:<32} {:<12} {:>6} {:>6} {:>6} {:>6} {:>7.1}",
            filename,
            format!("{version}"),
            summary.decoded,
            summary.unhandled,
            summary.errored,
            file_walk_skips,
            summary.decoded_ratio() * 100.0
        );

        // Custom(N) codes mean nothing on their own — the DXF class
        // name comes from this file's AcDb:Classes table, so an
        // unhandled custom object is only honest when it is named.
        let class_map = file.class_map().and_then(std::result::Result::ok);
        // Accumulate per-type histogram via error dedup, keyed by the
        // same name the unhandled histogram uses.
        for (tc, _msg) in &summary.errors {
            let label = class_map
                .as_ref()
                .and_then(|m| m.by_type_code(*tc))
                .map(|d| format!("{} (custom class)", d.dxf_class_name))
                .unwrap_or_else(|| format!("{} (0x{tc:04X})", ObjectType::from_code(*tc)));
            *type_histo.entry(label).or_default() += 1;
        }
        for entity in &entities {
            if let DecodedEntity::Unhandled { type_code, kind } = entity {
                // Custom class numbers are per-file, so an aggregate
                // histogram keys them by name alone; built-in codes
                // keep their code because it is stable across files.
                let label = class_map
                    .as_ref()
                    .and_then(|m| m.by_type_code(*type_code))
                    .map(|d| format!("{} (custom class)", d.dxf_class_name))
                    .unwrap_or_else(|| format!("{kind} (0x{type_code:04X})"));
                *unhandled_histo.entry(label).or_default() += 1;
            }
        }
        totals.decoded += summary.decoded;
        totals.unhandled += summary.unhandled;
        totals.errored += summary.errored;
    }

    println!("{}", "-".repeat(87));
    println!(
        "{:<32} {:<12} {:>6} {:>6} {:>6} {:>6} {:>7.1}",
        "TOTAL",
        "",
        totals.decoded,
        totals.unhandled,
        totals.errored,
        total_walk_skips,
        totals.decoded_ratio() * 100.0
    );
    println!();
    println!(
        "Handle-map self-check: {total_handle_mismatches} record(s) carry a handle \
         that disagrees with the map."
    );
    if walk_skips.is_empty() {
        println!("Walker diagnostics: every handle-map entry resolved to a record.");
    } else {
        println!("Walker diagnostics (handle-map entries that yielded no record):");
        let mut rows: Vec<(&String, &usize)> = walk_skips.iter().collect();
        rows.sort_by_key(|(label, cnt)| (std::cmp::Reverse(**cnt), (*label).clone()));
        for (label, cnt) in rows {
            println!("  {label:<44} → {cnt}");
        }
    }
    println!();
    if !type_histo.is_empty() {
        println!("Error histogram by kind (top 10):");
        let mut rows: Vec<(&String, &usize)> = type_histo.iter().collect();
        rows.sort_by_key(|(label, cnt)| (std::cmp::Reverse(**cnt), (*label).clone()));
        for (label, cnt) in rows.iter().take(10) {
            println!("  {label:<32} → {cnt} errors");
        }
    }
    if !unhandled_histo.is_empty() {
        println!();
        println!("Unhandled histogram by kind (top 15):");
        let mut rows: Vec<(&String, &usize)> = unhandled_histo.iter().collect();
        rows.sort_by_key(|(label, cnt)| (std::cmp::Reverse(**cnt), (*label).clone()));
        for (label, cnt) in rows.iter().take(15) {
            println!("  {label:<32} → {cnt}");
        }
    }

    ExitCode::SUCCESS
}

/// Collapse a per-record skip reason into a stable bucket so the
/// diagnostic histogram stays readable. Offsets and handle values are
/// dropped; the failure mode is what matters.
fn classify_skip(reason: &str) -> String {
    if reason.contains("past end of object stream") {
        "offset past end of object stream".to_string()
    } else if reason.contains("handle counter") {
        "handle counter above 8 (not a handle field)".to_string()
    } else if reason.contains("returned None") {
        "no record at offset (zero-length or truncated)".to_string()
    } else {
        reason
            .split(':')
            .next()
            .unwrap_or(reason)
            .trim()
            .to_string()
    }
}
