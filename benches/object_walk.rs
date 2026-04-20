//! Criterion benchmark for `DwgFile::all_objects` over the corpus.
//!
//! Run with:
//! ```bash
//! cargo bench --bench object_walk
//! ```
//!
//! # What this measures
//!
//! `all_objects` is the handle-map-driven object walker — it parses
//! `AcDb:Handles` to find every object, decompresses
//! `AcDb:AcDbObjects`, and returns every raw object blob with its
//! `ObjectType` and handle resolved. This is the hottest read path
//! once a DWG file is open, and the primary thing downstream tools
//! iterate.
//!
//! Because opening the file dominates cost for small fixtures, the
//! bench *pre-opens* each sample once outside the timing loop and
//! only measures the object-walk step. This matches how real
//! consumers use the API (open once, query many times).
//!
//! # Corpus
//!
//! Walks every `.dwg` under `../../samples/`. Each file that is
//! R2004-family (R2004 / R2010 / R2013 / R2018) registers one
//! Criterion bench target named after the file stem — e.g.
//! `all_objects/sample_AC1032`. Files that return `None` from
//! `all_objects` (notably the R14/R2000/R2007 formats where the
//! walker is not yet wired) are skipped with a log line.
//!
//! When `samples/` is absent the group is empty and the bench exits
//! silently, matching the `tests/corpus_roundtrip.rs` convention.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use dwg::DwgFile;
use std::fs;
use std::path::PathBuf;

fn samples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p
}

/// Enumerate `.dwg` fixtures, sorted for deterministic bench ordering.
fn list_dwg_samples() -> Vec<PathBuf> {
    let dir = samples_dir();
    let Ok(read) = fs::read_dir(&dir) else {
        eprintln!(
            "bench object_walk: {} missing — skipping whole suite",
            dir.display()
        );
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dwg"))
        .collect();
    out.sort();
    out
}

fn bench_object_walk(c: &mut Criterion) {
    let samples = list_dwg_samples();
    if samples.is_empty() {
        return;
    }
    let mut group = c.benchmark_group("all_objects");
    for path in samples {
        let Ok(file) = DwgFile::open(&path) else {
            eprintln!(
                "bench object_walk: {} failed to open — skipping",
                path.display()
            );
            continue;
        };
        // Probe once to confirm the walker is wired for this version;
        // registering a bench on a file that always returns None from
        // `all_objects` would just measure an `Option` unwrap.
        match file.all_objects() {
            Some(Ok(_)) => {}
            _ => {
                eprintln!(
                    "bench object_walk: {} has no walkable AcDbObjects — skipping",
                    path.display()
                );
                continue;
            }
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        group.bench_with_input(BenchmarkId::new("walk", &stem), &file, |b, f| {
            b.iter(|| {
                let v = f
                    .all_objects()
                    .expect("walker-supported file")
                    .expect("walk ok");
                black_box(v.len());
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_object_walk);
criterion_main!(benches);
