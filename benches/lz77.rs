//! Criterion benchmarks for `crate::lz77::decompress`.
//!
//! Run with:
//! ```bash
//! cargo bench --bench lz77
//! ```
//!
//! The benchmarks pin performance on three realistic input shapes:
//!
//! - Pure literal-run streams (no back-references) — the common case
//!   for small sections where the payload is already uncompressed.
//! - Long-run RLE-style back-references — stress the copy loop in
//!   the decompressor and the self-reading-window edge case.
//! - Mixed streams with many small back-references — closer to what
//!   a real R2004+ AcDb section looks like.
//!
//! Output lives under `target/criterion/`. Commit meaningful
//! regressions to the discussion; use `cargo bench -- --save-baseline
//! <name>` to pin a reference point across branches.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use dwg::lz77;

/// Build a literal-only stream of `n_literal_bytes` bytes wrapped by
/// the DWG LZ77 "literal length" prefix and `0x11` terminator.
fn literal_stream(n_literal_bytes: usize) -> Vec<u8> {
    let mut s = Vec::with_capacity(n_literal_bytes + 8);
    // For small n (<= 0x12 = 18) the length is encoded as (n - 3) in
    // a single byte 0x01..=0x0F. For larger n we use the 0x00-run
    // extension.
    if (3..=0x12).contains(&n_literal_bytes) {
        s.push((n_literal_bytes - 3) as u8);
    } else {
        // 0x00 starts the extension with an initial 0x0F total.
        // Each subsequent 0x00 byte adds 0xFF; a non-zero byte adds
        // itself + 3 and terminates.
        let remainder = n_literal_bytes - 3;
        // remainder = 0x0F (from initial 0x00) + k * 0xFF + terminating_byte
        let mut running = 0x0Fusize;
        s.push(0x00);
        while running + 0xFF < remainder {
            s.push(0x00);
            running += 0xFF;
        }
        let terminating_byte = remainder - running;
        assert!(terminating_byte > 0 && terminating_byte < 0x100);
        s.push(terminating_byte as u8);
    }
    for i in 0..n_literal_bytes {
        s.push((i & 0xFF) as u8);
    }
    s.push(0x11);
    s
}

/// Build a back-reference stream: a 4-byte literal run ("ABCD")
/// followed by ONE 4-byte back-reference copy at offset 3, plus 2
/// literal bytes, then terminator. This matches the
/// `back_reference_with_rle_wrap` test case in `src/lz77.rs`.
fn backref_stream_basic() -> Vec<u8> {
    vec![
        0x04, b'A', b'B', b'C', b'D', b'E', b'F', b'G', // 7-byte literal
        0x22, 0x0A, 0x00, // opcode 0x22 → copy 4 at offset 3, lit_count 2
        b'X', b'Y', // 2 literal bytes
        0x11, // terminator
    ]
}

fn bench_lz77(c: &mut Criterion) {
    let mut group = c.benchmark_group("lz77_decompress");

    for n in [64, 1024, 16 * 1024] {
        let stream = literal_stream(n);
        group.throughput(Throughput::Bytes(n as u64));
        group.bench_with_input(BenchmarkId::new("literal_only", n), &stream, |b, s| {
            b.iter(|| {
                let out = lz77::decompress(black_box(s), Some(n)).unwrap();
                black_box(out);
            })
        });
    }

    let backref_stream = backref_stream_basic();
    group.throughput(Throughput::Bytes(13));
    group.bench_with_input(
        BenchmarkId::new("backref_copy", 1),
        &backref_stream,
        |b, s| {
            b.iter(|| {
                let out = lz77::decompress(black_box(s), None).unwrap();
                black_box(out);
            })
        },
    );

    group.finish();
}

criterion_group!(benches, bench_lz77);
criterion_main!(benches);
