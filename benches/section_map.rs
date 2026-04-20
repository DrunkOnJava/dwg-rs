//! Criterion benchmark for the R2004+ section-map cold-start parse.
//!
//! Run with:
//! ```bash
//! cargo bench --bench section_map
//! ```
//!
//! # What this measures
//!
//! `DwgFile::from_bytes` is the cold-start entry point that:
//!
//! 1. Identifies the file magic and picks a version branch.
//! 2. For R2004-family files (AC1018 / AC1024 / AC1027 / AC1032) runs
//!    the full section-map parse — `R2004Header::parse` +
//!    `extract_r2004_sections` (the crate-private helper in
//!    `reader.rs`) — which walks the page map, the section-info list,
//!    and resolves the decoded section layout.
//!
//! The public surface we benchmark is `DwgFile::from_bytes` with a
//! pre-loaded buffer, so filesystem I/O is excluded from the timing
//! loop. `extract_r2004_sections` itself is not pub, which is fine:
//! the overwhelming majority of its cost is `parse_page_map` +
//! `parse_section_info`, both of which run unconditionally during
//! `from_bytes` on an R2018 sample, so measuring the public call is a
//! faithful proxy.
//!
//! # Corpus
//!
//! Uses `../../samples/sample_AC1032.dwg` (a 1 MB R2018 fixture from
//! the public `nextgis/dwg_samples` corpus). When the corpus is
//! absent — as it is in vendored downstream builds — the bench
//! registers an empty `criterion_group!` and exits silently. This
//! matches the skip-when-absent convention used by the integration
//! tests in `tests/corpus_roundtrip.rs` and `tests/samples.rs`.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use dwg::DwgFile;
use std::path::PathBuf;

/// Resolve `../../samples/<name>` relative to the crate manifest, the
/// same way the integration tests do. Returns `None` if the sample is
/// absent so the bench can register an empty suite instead of
/// panicking.
fn sample_bytes(name: &str) -> Option<Vec<u8>> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p.push(name);
    if !p.exists() {
        eprintln!(
            "bench section_map: sample {} absent at {} — skipping",
            name,
            p.display()
        );
        return None;
    }
    match std::fs::read(&p) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!(
                "bench section_map: failed to read {}: {e} — skipping",
                p.display()
            );
            None
        }
    }
}

fn bench_section_map(c: &mut Criterion) {
    let Some(bytes) = sample_bytes("sample_AC1032.dwg") else {
        return;
    };
    let mut group = c.benchmark_group("section_map_cold_start");
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("from_bytes_AC1032", |b| {
        b.iter(|| {
            // Clone is cheap vs. the downstream section-map walk and
            // matches the real-world open-from-memory code path.
            let f = DwgFile::from_bytes(black_box(bytes.clone())).expect("AC1032 parse");
            black_box(f.sections().len());
        });
    });
    group.finish();
}

criterion_group!(benches, bench_section_map);
criterion_main!(benches);
