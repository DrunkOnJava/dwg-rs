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
use dwg::{DwgFile, entities::DispatchSummary};
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
    let mut type_histo: BTreeMap<u16, (usize, usize, usize)> = BTreeMap::new();
    let mut unhandled_histo: BTreeMap<String, usize> = BTreeMap::new();

    println!(
        "{:<32} {:<12} {:>6} {:>6} {:>6} {:>7}",
        "file", "version", "deco", "skip", "err", "ratio%"
    );
    println!("{}", "-".repeat(80));

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
                    "{:<32} {:<12} n/a    n/a    n/a       (no-handle-map)",
                    filename,
                    format!("{version}")
                );
                continue;
            }
        };

        println!(
            "{:<32} {:<12} {:>6} {:>6} {:>6} {:>7.1}",
            filename,
            format!("{version}"),
            summary.decoded,
            summary.unhandled,
            summary.errored,
            summary.decoded_ratio() * 100.0
        );

        // Accumulate per-type histogram via error dedup.
        for (tc, _msg) in &summary.errors {
            type_histo.entry(*tc).or_default().2 += 1;
        }
        // Custom(N) codes mean nothing on their own — the DXF class
        // name comes from this file's AcDb:Classes table, so an
        // unhandled custom object is only honest when it is named.
        let class_map = file.class_map().and_then(std::result::Result::ok);
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

    println!("{}", "-".repeat(80));
    println!(
        "{:<32} {:<12} {:>6} {:>6} {:>6} {:>7.1}",
        "TOTAL",
        "",
        totals.decoded,
        totals.unhandled,
        totals.errored,
        totals.decoded_ratio() * 100.0
    );
    println!();
    if !type_histo.is_empty() {
        println!("Error histogram by type code (top 10):");
        let mut err_counts: Vec<(u16, usize)> = type_histo
            .iter()
            .map(|(tc, (_, _, err))| (*tc, *err))
            .collect();
        err_counts.sort_by_key(|(_, cnt)| std::cmp::Reverse(*cnt));
        for (tc, cnt) in err_counts.iter().take(10) {
            println!("  type_code {tc:<5} → {cnt} errors");
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
