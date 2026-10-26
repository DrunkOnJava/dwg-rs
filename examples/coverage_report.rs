//! Honest coverage report — runs the entity dispatcher against every
//! DWG file in a directory and prints per-file + aggregate
//! decoded/unhandled/errored counts. No rationalization, no rounding
//! up, no excuses.
//!
//! ```bash
//! cargo run --release --example coverage_report -- path/to/corpus/ [--tsv out.tsv]
//! ```
//!
//! This example intentionally does NOT dump per-entity field values.
//! Its audience is CI (the coverage-smoke and corpus jobs call it) and
//! humans wanting a quick corpus-wide summary; printing every decoded
//! value would bury the summary in noise. For per-entity field
//! inspection, see the sibling example
//! [`dump_decoded_entities`](../examples/dump_decoded_entities.rs).
//!
//! `--tsv <path>` also writes the same measurement as tab-separated
//! rows, which `tools/corpus/measure.py` reads instead of the aligned
//! table:
//!
//! ```text
//! file       <name> <AC10xx> <decoded> <skipped> <errored> <walk-skips> <handle-mismatches>
//! error      <name> <type label> <count>
//! unhandled  <name> <type label> <count>
//! failed     <name> <stage> <message>
//! ```

use dwg::entities::DecodedEntity;
use dwg::{DwgFile, ObjectType, entities::DispatchSummary};
use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut dir_arg = None;
    let mut tsv_path = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--tsv" {
            tsv_path = args.get(i + 1).map(PathBuf::from);
            i += 2;
        } else {
            dir_arg.get_or_insert_with(|| args[i].clone());
            i += 1;
        }
    }
    let Some(dir_arg) = dir_arg else {
        eprintln!("usage: coverage_report <directory-of-dwg-files> [--tsv <path>]");
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

    let mut tsv = String::new();
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
                let _ = writeln!(tsv, "failed\t{filename}\topen\t{}", one_line(&e));
                continue;
            }
        };
        let version = file.version();
        let magic = String::from_utf8_lossy(&version.magic()).into_owned();
        let (entities, summary) = match file.decoded_entities() {
            Some(Ok((e, s))) => (e, s),
            Some(Err(e)) => {
                println!("{:<32} decoded_entities-failed: {e}", filename);
                let _ = writeln!(tsv, "failed\t{filename}\tdecode\t{}", one_line(&e));
                continue;
            }
            None => {
                println!(
                    "{:<32} {:<12} n/a    n/a    n/a    n/a       (no-handle-map)",
                    filename,
                    format!("{version}")
                );
                let _ = writeln!(tsv, "failed\t{filename}\twalk\tno handle map");
                continue;
            }
        };

        // Walker-level diagnostic pass: how many handle-map entries did
        // not resolve to a record at all?
        let (file_walk_skips, file_handle_mismatches) = match file.all_objects_lossy() {
            Some(Ok((_, walk))) => {
                for entry in &walk.skipped {
                    *walk_skips.entry(classify_skip(&entry.reason)).or_default() += 1;
                }
                (walk.skipped.len(), walk.handle_mismatches.len())
            }
            _ => (0, 0),
        };
        total_walk_skips += file_walk_skips;
        total_handle_mismatches += file_handle_mismatches;

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
        let _ = writeln!(
            tsv,
            "file\t{filename}\t{magic}\t{}\t{}\t{}\t{file_walk_skips}\t{file_handle_mismatches}",
            summary.decoded, summary.unhandled, summary.errored
        );

        // Custom(N) codes mean nothing on their own — the DXF class
        // name comes from this file's AcDb:Classes table, so an
        // unhandled custom object is only honest when it is named.
        let class_map = file.class_map().and_then(std::result::Result::ok);
        let mut file_errors: BTreeMap<String, usize> = BTreeMap::new();
        let mut file_unhandled: BTreeMap<String, usize> = BTreeMap::new();
        // Accumulate per-type histogram via error dedup, keyed by the
        // same name the unhandled histogram uses.
        for (tc, _msg) in &summary.errors {
            let label = class_map
                .as_ref()
                .and_then(|m| m.by_type_code(*tc))
                .map(|d| format!("{} (custom class)", d.dxf_class_name))
                .unwrap_or_else(|| format!("{} (0x{tc:04X})", ObjectType::from_code(*tc)));
            *file_errors.entry(label).or_default() += 1;
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
                *file_unhandled.entry(label).or_default() += 1;
            }
        }
        for (label, cnt) in file_errors {
            let _ = writeln!(tsv, "error\t{filename}\t{label}\t{cnt}");
            *type_histo.entry(label).or_default() += cnt;
        }
        for (label, cnt) in file_unhandled {
            let _ = writeln!(tsv, "unhandled\t{filename}\t{label}\t{cnt}");
            *unhandled_histo.entry(label).or_default() += cnt;
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
        println!("Error histogram by kind:");
        let mut rows: Vec<(&String, &usize)> = type_histo.iter().collect();
        rows.sort_by_key(|(label, cnt)| (std::cmp::Reverse(**cnt), (*label).clone()));
        for (label, cnt) in rows {
            println!("  {label:<32} → {cnt} errors");
        }
    }
    if !unhandled_histo.is_empty() {
        println!();
        println!("Unhandled histogram by kind:");
        let mut rows: Vec<(&String, &usize)> = unhandled_histo.iter().collect();
        rows.sort_by_key(|(label, cnt)| (std::cmp::Reverse(**cnt), (*label).clone()));
        for (label, cnt) in rows {
            println!("  {label:<32} → {cnt}");
        }
    }

    if let Some(path) = tsv_path {
        if let Err(e) = std::fs::write(&path, tsv) {
            eprintln!("cannot write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// Flatten an error message onto one TSV field: tabs and newlines would
/// split the row.
fn one_line(e: &impl std::fmt::Display) -> String {
    e.to_string().replace(['\t', '\n', '\r'], " ")
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
