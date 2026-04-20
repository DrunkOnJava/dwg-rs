//! Criterion benchmark for the three metadata readers:
//! `DwgFile::summary_info`, `DwgFile::app_info`, `DwgFile::preview`.
//!
//! Run with:
//! ```bash
//! cargo bench --bench metadata_parse
//! ```
//!
//! # What this measures
//!
//! Each accessor reads the corresponding section, runs LZ77
//! decompression if the section is stored compressed, and then feeds
//! the decoded bytes into the metadata parser in `src/metadata.rs`.
//! Cost breakdown is roughly:
//!
//! - `summary_info` — small (< 1 KB decompressed typical), dominated
//!   by UTF-16 string decoding and the property-count loop.
//! - `app_info` — similar shape to SummaryInfo plus up to 16 GUID
//!   buffers; roughly twice the parse time on real corpora.
//! - `preview` — the biggest of the three because it carves the
//!   embedded BMP / WMF / PNG thumbnail payload.
//!
//! As with `object_walk.rs`, the `DwgFile` is opened once outside the
//! timing loop and reused across iterations. Only the per-accessor
//! cost is measured.
//!
//! # Corpus
//!
//! Uses `../../samples/sample_AC1032.dwg` as the primary fixture
//! because it has all three sections populated. If the sample is
//! absent the benches are skipped silently.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use dwg::DwgFile;
use std::path::PathBuf;

fn sample_path(name: &str) -> Option<PathBuf> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p.push(name);
    if p.exists() { Some(p) } else { None }
}

fn bench_metadata(c: &mut Criterion) {
    let Some(path) = sample_path("sample_AC1032.dwg") else {
        eprintln!("bench metadata_parse: sample_AC1032.dwg absent — skipping");
        return;
    };
    let Ok(file) = DwgFile::open(&path) else {
        eprintln!("bench metadata_parse: AC1032 open failed — skipping");
        return;
    };

    let mut group = c.benchmark_group("metadata_parse");

    if file.summary_info().and_then(Result::ok).is_some() {
        group.bench_with_input(BenchmarkId::new("summary_info", "AC1032"), &file, |b, f| {
            b.iter(|| {
                let si = f
                    .summary_info()
                    .expect("section present")
                    .expect("parse ok");
                black_box(si);
            });
        });
    } else {
        eprintln!("bench metadata_parse: AC1032 has no AcDb:SummaryInfo — skipping target");
    }

    if file.app_info().and_then(Result::ok).is_some() {
        group.bench_with_input(BenchmarkId::new("app_info", "AC1032"), &file, |b, f| {
            b.iter(|| {
                let ai = f.app_info().expect("section present").expect("parse ok");
                black_box(ai);
            });
        });
    } else {
        eprintln!("bench metadata_parse: AC1032 has no AcDb:AppInfo — skipping target");
    }

    if file.preview().and_then(Result::ok).is_some() {
        group.bench_with_input(BenchmarkId::new("preview", "AC1032"), &file, |b, f| {
            b.iter(|| {
                let pv = f.preview().expect("section present").expect("parse ok");
                black_box(pv);
            });
        });
    } else {
        eprintln!("bench metadata_parse: AC1032 has no AcDb:Preview — skipping target");
    }

    group.finish();
}

criterion_group!(benches, bench_metadata);
criterion_main!(benches);
