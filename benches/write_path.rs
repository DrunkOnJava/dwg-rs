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
//! - 1 KiB of alternating-byte pattern (worst case for LZ77 match
//!   finding)
//! - 16 KiB of uniform fill (best case — maximum compression)
//! - 64 KiB of random-looking data (typical `AcDb:Header`)
//! - 256 KiB of mixed (approximates a heavy `AcDbObjects` stream)
//!
//! Throughput is reported in bytes/second of input — `criterion
//! --throughput bytes` shows this as MiB/s in the CLI output.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use dwg::file_writer::WriterScaffold;
use dwg::version::Version;

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
            b.iter(|| {
                let mut scaffold = WriterScaffold::new(Version::R2018);
                scaffold.add_section("AcDb:Header", black_box(p.clone()));
                let _ = scaffold.build_sections();
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_stage1_build);
criterion_main!(benches);
