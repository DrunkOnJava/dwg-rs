//! Criterion benchmark for the Phase 12 write path.
//!
//! Run with:
//! ```bash
//! cargo bench --bench write_path
//! ```
//!
//! # What this measures
//!
//! Four fixed-size payloads run through the Stage-1 writer (LZ77
//! encode + 32-byte page header + Sec_Mask mask). This is the
//! per-section cost any future full writer will pay N times over,
//! so catching regressions at Stage 1 is the leverage point.
//!
//! - 1 KiB of alternating-byte pattern
//! - 16 KiB of uniform fill
//! - 64 KiB of random-looking data (typical `AcDb:Header`)
//! - 256 KiB of mixed (approximates a heavy `AcDbObjects` stream)
//!
//! Throughput is reported in bytes/second of input — `criterion
//! --throughput bytes` shows this as MiB/s in the CLI output.
//!
//! # What the payload labels do and do not mean (issue #17)
//!
//! The cells are named after their byte patterns for historical
//! reasons, but `lz77_encode::compress` is **literal-only** — it emits
//! one initial literal plus a terminator and performs no match search
//! whatsoever. So "alternating" vs "uniform" vs "mixed" is not a
//! compressibility axis here; the four cells differ only in size.
//!
//! Measured 2026-08-30: ~92 % of every cell is
//! `section_writer::compute_checksum`, a strictly serial 2-cycles-per-
//! byte rotate-and-add chain run twice per section (once over the
//! compressed bytes, once over the decompressed ones). Read a
//! regression here as "the §4.6.1 checksum or the page framing got
//! slower", not as "LZ77 matching got worse".
//!
//! # Why `iter_batched`
//!
//! `WriterScaffold::add_section` takes the payload by value, so each
//! iteration needs its own copy. Cloning inside `b.iter(...)` charged
//! a 16-256 KiB allocate-plus-memcpy to the measurement — pure
//! allocator variance on top of the work under test. `iter_batched`
//! moves the clone into the setup closure, which criterion excludes
//! from the timing.

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use dwg::file_writer::WriterScaffold;
use dwg::version::Version;
use std::hint::black_box;

fn make_alternating(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i & 0xFF) as u8).collect()
}

fn make_uniform(n: usize, fill: u8) -> Vec<u8> {
    vec![fill; n]
}

fn make_mixed(n: usize) -> Vec<u8> {
    // Deterministic pseudo-random via a tiny linear-congruential
    // generator — avoids adding a rand dep.
    let mut out = Vec::with_capacity(n);
    let mut s: u32 = 0xDEADBEEF;
    for _ in 0..n {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
        out.push((s >> 16) as u8);
    }
    out
}

fn bench_stage1_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("stage1_build_section");
    for &(label, size) in &[
        ("alt_1k", 1024usize),
        ("uniform_16k", 16 * 1024),
        ("mixed_64k", 64 * 1024),
        ("mixed_256k", 256 * 1024),
    ] {
        let payload = match label {
            "alt_1k" => make_alternating(size),
            "uniform_16k" => make_uniform(size, 0x55),
            "mixed_64k" | "mixed_256k" => make_mixed(size),
            _ => unreachable!(),
        };
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &payload, |b, p| {
            b.iter_batched(
                || p.clone(),
                |owned| {
                    let mut scaffold = WriterScaffold::new(Version::R2018);
                    scaffold.add_section("AcDb:Header", black_box(owned));
                    let _ = scaffold.build_sections();
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_stage1_build);
criterion_main!(benches);
