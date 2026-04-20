//! Harness for comparing dwg-rs read performance against LibreDWG.
//!
//! Run the dwg-rs side of the comparison with:
//! ```bash
//! cargo bench --bench libredwg_compare
//! ```
//!
//! # Why this file does NOT call LibreDWG
//!
//! LibreDWG is **GPL-3-licensed**. `dwg-rs` is a CLEANROOM Apache-2
//! implementation — we explicitly do not link, call, or shell out to
//! LibreDWG source, binaries, or headers from this crate. See
//! [`CLEANROOM.md`](../CLEANROOM.md) for the full isolation policy.
//!
//! Shelling out to an external GPL-3 process from an Apache-2 bench
//! file would not taint the library (process isolation keeps the
//! licenses separate) but it would invite confusion about the
//! cleanroom posture and tempt future contributors to pull in
//! LibreDWG headers to "make the comparison more precise." The bench
//! therefore measures only the dwg-rs side and documents — in this
//! docstring — how a GPL-aware operator with LibreDWG already
//! installed can reproduce the comparison on the same fixture.
//!
//! # Manual comparison procedure
//!
//! 1. Install LibreDWG on a machine you control (Homebrew:
//!    `brew install libredwg`; Debian/Ubuntu: `apt install libredwg`).
//!    Because LibreDWG is GPL-3, keep it in its own sandbox / Docker
//!    image; **do not** vendor its source or copy any of it into the
//!    dwg-rs repository.
//!
//! 2. Time LibreDWG's `dwgread` against the same fixture used below
//!    (`../../samples/sample_AC1032.dwg`):
//!    ```bash
//!    # Warm the page cache first; `dwgread -O` prints the parse
//!    # time and an object summary without re-serializing.
//!    for i in 1 2 3 4 5; do
//!      /usr/bin/time -v dwgread -O samples/sample_AC1032.dwg \
//!        > /tmp/libredwg-out.$i 2> /tmp/libredwg-time.$i
//!    done
//!    # Grep "Elapsed (wall clock) time" out of the /tmp/libredwg-time.*
//!    # files and take the median.
//!    ```
//!
//! 3. Run the dwg-rs bench (this file) and note the
//!    `libredwg_compare/dwg_rs_open_plus_walk` point estimate printed
//!    by Criterion (or read `target/criterion/.../estimates.json`).
//!
//! 4. To compare against LibreDWG, run libredwg's `dwgread -O` on the
//!    same fixture and divide the median wall-clock time by the
//!    Criterion point estimate — that ratio is the speedup (or
//!    regression) factor.
//!
//! # What the dwg-rs side measures
//!
//! A full cold-start read: open the file, walk every object via
//! `all_objects`, and parse `summary_info` + `app_info` + `preview`.
//! This is the closest public-API approximation to what `dwgread -O`
//! does internally (page-map parse → object walk → metadata dump).

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use dwg::DwgFile;
use std::path::PathBuf;

fn sample_bytes(name: &str) -> Option<Vec<u8>> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p.push(name);
    if !p.exists() {
        eprintln!(
            "bench libredwg_compare: sample {} absent at {} — skipping",
            name,
            p.display()
        );
        return None;
    }
    std::fs::read(&p).ok()
}

fn bench_libredwg_compare(c: &mut Criterion) {
    let Some(bytes) = sample_bytes("sample_AC1032.dwg") else {
        return;
    };
    let mut group = c.benchmark_group("libredwg_compare");
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    // Keep sample count modest — the walk+metadata path takes tens
    // of ms per iteration on realistic corpora and we'd rather have a
    // stable estimate than an overlong wall-clock run.
    group.sample_size(20);
    group.bench_function("dwg_rs_open_plus_walk", |b| {
        b.iter(|| {
            let f = DwgFile::from_bytes(black_box(bytes.clone())).expect("parse");
            // All three closest-matching `dwgread -O` operations: the
            // object walk and the three public metadata parsers.
            if let Some(Ok(objs)) = f.all_objects() {
                black_box(objs.len());
            }
            if let Some(Ok(si)) = f.summary_info() {
                black_box(si);
            }
            if let Some(Ok(ai)) = f.app_info() {
                black_box(ai);
            }
            if let Some(Ok(pv)) = f.preview() {
                black_box(pv);
            }
        });
    });
    group.finish();
}

criterion_group!(benches, bench_libredwg_compare);
criterion_main!(benches);
