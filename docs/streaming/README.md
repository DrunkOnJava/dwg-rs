# Streaming core

The first implemented scope is the complete **file header** of R13, R14 and
R2000 (AC1012/AC1014/AC1015), ODA v5.4.1 §3.2. It is a hand-written pure
state machine. It does not parse drawing objects or the newer containers.
Those remain open in [#205](https://github.com/DrunkOnJava/dwg-rs/issues/205).
The spec compiler is an upgrade after the hand-written baseline, tracked in
[#208](https://github.com/DrunkOnJava/dwg-rs/issues/208).

`streaming::FlatHeaderParser::request()` declares the exact remaining byte
count and absolute offset for the current transition. `feed()` accepts a
nonempty prefix of that demand. A short feed saves its partial field;
wrong offsets, empty input and oversized delivery are rejected without
mutation. Grammar failures are terminal values. CRC, locator identities,
source ranges and the end sentinel must all pass before `header()` returns
output. Record 5 is kept as an opaque numbered locator; its meaning is not
guessed. R2004+ yields `UnsupportedLayout`, rather than a successful header
with missing sections.

The grammar retains a fixed 256-byte buffer and has no heap allocation,
source handles or I/O. CI compiles the **same source files** independently
with `no_std`, no allocator and `forbid(unsafe_code)`, on native and
`wasm32-unknown-unknown`. Compile-time bounds cap parser state at 512 bytes
and the checkpoint holder at 272 bytes, including bookkeeping. This proof
covers `src/streaming/`; the legacy reader still loads complete files and
sections. It is not a full-crate streaming certification.

`byte_source::ByteSource` is the pausable delivery contract. A poll returns
`Ready(n)`, `Pending`, EOF or an adapter error. `poll_header()` performs at
most one read into caller-owned bounded scratch. `FileSource` handles short
reads, interrupted calls and absolute positioning without seeking to the
end; `SliceSource` borrows immutable bytes. HTTP and worker implementations
can satisfy the same requests without changing grammar. Their actual
network/worker adapters are still open in #206. A forward-only transport
for a complete DWG needs a spool/cache because indexes can follow objects.

## Checkpoint contract

A checkpoint contains the source identity and length, phase, byte position,
current target, locator count/index/mask, failure marker and all retained
partial header bytes. The wire form is canonical, versioned and little
endian. Its current maximum is 157 bytes (97 consumed header bytes plus 60
bytes of metadata/checksum). The holder and every scratch buffer fit within
one declared 256-byte chunk.

| Byte offset | Encoding |
| --- | --- |
| 0 | Four-byte `DWGH` marker |
| 4 | Format version 1 |
| 5–9 | Phase, locator count, locator index, seen mask, failure flag |
| 10 | u16 consumed byte count |
| 12 | u16 current transition target |
| 14 | u16 chunk limit |
| 16 | u64 source length |
| 24 | 32-byte source identity |
| 56 | Consumed prefix, including any partial field |
| Last 4 | CRC32 of all preceding checkpoint bytes |

Restore checks the complete length before copying, rejects incompatible
versions and source identities, checks CRC, then validates phase and counters
by replaying only the retained bytes **inside the checkpoint**. No source
bytes are read to reconstruct state. Failure snapshots restore the same
failure. The next request starts at the first unconsumed source byte.

The caller supplies identity from a trusted content manifest or immutable
object version; the parser does not reread a multi-gigabyte source to hash
it. CRC detects accidental checkpoint corruption and provides no authenticity
guarantee. The caller owns atomic custody of saved state and any already
committed output. `header()` is an inspection API, not an exactly-once event
queue. Full object/decompression checkpoints remain open in #207.

## Repeatable acceptance

The real executable is `examples/stream_header.rs`. The Python driver uses
it in separate OS processes; the proptest executable generates and shrinks
chunk schedules for that same entry point. No new unit tests are involved.

```sh
cargo build --locked --example stream_header --example streaming_schedules
python3 tools/streaming/verify.py target/debug/examples/stream_header target/debug/examples/streaming_schedules
python3 tools/streaming/check_invariants.py
mkdir -p streaming-build
rustc --edition=2024 --crate-type rlib tools/streaming/no_std.rs --out-dir streaming-build
rustc --edition=2024 --crate-type rlib --target wasm32-unknown-unknown tools/streaming/no_std.rs --out-dir streaming-build
```

Run these on the project's build host. The workflow uploads
`streaming-evidence.json` and the generated schedule JSONL files. It checks
12 synthetic version/count combinations, the canonical R2000 fixture,
bytewise and short delivery,
truncation at every header byte, malformed input, two hash-pinned real
R14/R2000 headers, 128 generated schedules per real file, 98 fresh-process
resume positions, actual source-read continuity, pending delivery, corrupt
and inconsistent checkpoints, source mismatch and failure-state restore.
The real-file manifest records header fields, not object counts. No real R13
file, multi-gigabyte model or independent object-count oracle has passed
this reader. The kernel WASM compile does not establish browser runtime or
cross-runtime checkpoint acceptance.

## Current evidence

[Run 40234765141](https://github.com/DrunkOnJava/dwg-rs/actions/runs/40234765141)
passed on `a6dc6baa2d449e3f33db3bba0e3998803cc87834`: native executable
acceptance, proptest schedules, fresh-process resume and the native/WASM
no-allocator kernel builds. The evidence artifact identifies the built
binary by SHA-256. The same commit also passed the existing three WASM
wrapper build targets. Repository-wide checks are reported separately by the
PR; these claims do not establish a complete model parse or release readiness.

The canonical R2000 fixture's writer used to omit the file-header CRC and
sentinel, so the strict reader rejected it ([#214](https://github.com/DrunkOnJava/dwg-rs/issues/214)).
The generator now writes both and the acceptance run checks that file
under every chunk size from 1 to 32 bytes. Its other sections remain
placeholders. The audit and complete ordered work
are in [audit.md](audit.md) and [#204](https://github.com/DrunkOnJava/dwg-rs/issues/204).
