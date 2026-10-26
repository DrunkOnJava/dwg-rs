# Streaming architecture audit

Audited base: `ece6adf`, October 25, 2026. Tracking epic: [#204](https://github.com/DrunkOnJava/dwg-rs/issues/204).

| Priority | Requirement | Current evidence | Issue |
| --- | --- | --- | --- |
| 1 | Pure byte-demand state machine | `reader.rs` uses `fs::read`, retains all input, materializes sections; decoder calls are synchronous full slices | #205 |
| 2 | Pausable transport independent of grammar | No byte source trait; native and WASM supply entire owned files | #206 |
| 3 | Portable exact-position checkpoints | No versioned parser-state encoding or fresh-process resume | #207 |
| 4 | Reviewed grammar compiler | Rust field lists handwritten; spec coverage is a Markdown matrix | #208 |
| 5 | Streaming WASM | Existing native/WASM build CI; WASM open copies whole upload; browser uses `arrayBuffer()` | #209 |
| 6 | Boundary equivalence and automatic walls | Existing proptest/fuzz targets exercise whole slices; no chunk equivalence or parser allocation gate | #210 |
| 7 | Real golden and multi-gigabyte evidence | Five committed synthetic fixtures; no independent large-model oracle; public corpus CI tracked in #95/#97 | #211 |
| 8 | Spec change watcher | No pinned public-document hash or revision notification | #212 |

The existing `Version` rejects R13/AC1012 (#119). `OpenLimits` documents
several downstream caps as advisory; limits alone do not establish bounded
streaming. The four viewer samples are identical generated skeletons, not
four independently authored drawings. All remain explicit migration gaps.

## Decisions

1. A hand-written state machine comes first. A compiler may replace measured
   transitions after parity; generated files are never manually patched.
2. Grammar declares absolute byte demands. Source I/O, pending callbacks,
   scheduling, caching and file descriptors stay in adapters. An indexed
   DWG on a forward-only stream requires a spool/cache or a range-capable
   source; transport independence cannot remove this physical requirement.
3. The first core milestone is a bounded file-header state machine. Its
   completion means a header completed, never a model or object walk.
   Container and object migration remains open under #205.
4. Explicit failed/unsupported outcomes are part of the output contract.
   Unknown spec fields are preserved or surfaced; never silently guessed.
5. E2E acceptance invokes a built executable and saves receipts. Proptest
   generates chunk schedules for that executable. No new unit tests.
6. No full-parser memory or performance claim until it is measured on real
   files with independent object counts. Missing large models remain
   NOT MEASURED. Synthetic fixtures and build success cannot substitute.
7. Commit contract for the R13–R2000 container walk (October 25, 2026).
   **Index bytes are verified before anything is derived from them;
   payload bytes are delivered provisionally and committed by a verdict.**
   The file header fits in one chunk and is verified whole. An object-map
   chunk (up to 2032 bytes) does not, so the walker reads it twice: a
   CRC-only pass with no storage, then a decoding pass that visits
   objects. No object is visited from an unverified chunk; the second
   pass costs one extra read of the map, which is small next to the
   objects it indexes. Header-variable, class and object data are handed
   to the caller as they arrive, in slices no larger than the request,
   and become committed only at the `Section` or `Object` step emitted
   after their CRC, sentinel and handle checks pass. A failed check is
   a terminal error; the caller discards uncommitted payload.

## Core failure cases to exercise before implementation

- EOF at every byte of a field, locator, CRC and sentinel.
- Wrong magic, unsupported release, unrecognized locator count/number,
  duplicate locators, overflow/out-of-source section ranges and bad CRC.
- Exact-demand, one-byte and random short reads produce identical results.
- Oversized, empty or out-of-position feed rejected without mutation.
- Pending transport leaves parser unchanged; zero-byte EOF is distinct.
- Mid-field checkpoint, terminal checkpoint, corrupted/truncated/oversized
  checkpoint, incompatible schema, wrong source identity/length or limits.
- Resume in a fresh process starts at the first unconsumed source byte.
- Parser emits no success event until integrity checks pass.
- Oversized chunk configuration never induces an allocation.

Spec authority: ODA public *Open Design Specification for .dwg files*
v5.4.1, chapters 3–8 and §2.14. Checked against the official PDF on
October 25, 2026. Only specification and this repository's own implementation
were consulted; no third-party DWG implementation source.

## Container walk failure cases (R13–R2000, written before the walker)

Spec framing, confirmed on `line_R14.dwg` and `line_2000.dwg` before
coding: §9 and §10.1 sections are start sentinel, `RL` size, data, `RS`
CRC seeded 0xC0C1 over size and data, end sentinel, and the locator size
equals 38 plus the data size. §23.1 map chunks are a big-endian size
that counts its own two bytes, then entries, then a big-endian CRC
seeded 0xC0C1 over size and entries; a size-2 chunk ends the map at the
locator's end. §20.1 records are `MS` size, data, then an `RS` CRC
seeded 0xC0C1 over the `MS` bytes and data.

- EOF inside every fixed field, payload run, map entry and record.
- Wrong start or end sentinel; locator size not 38 plus the data size;
  zero-size class locator; section CRC mismatch.
- Map chunk size below 2 or above 2032, running past the map locator,
  final chunk not ending exactly at the locator end, chunk CRC mismatch.
- Map entry crossing its chunk end, `MC` longer than a `u64` holds,
  handle or offset overflow, negative absolute offset.
- Record outside the source, overlapping the file header or the map,
  `MS` of zero or of more than two words, record CRC mismatch, prologue
  too short for type and handle, handle counter above 8, record handle
  different from the map's handle.
- Every chunk schedule (1 byte, odd sizes, random) produces the same
  steps and committed records.
- Checkpoint at every step boundary, restored in a fresh process: the
  concatenated source reads and outputs of the two processes equal the
  uninterrupted run's, so no byte is read twice and no record is
  delivered twice.
- Corrupt, truncated, oversized, wrong-version, wrong-identity and
  internally inconsistent walk checkpoints are rejected before use.
- Committed records equal the legacy reader's handle-map walk (count,
  handle, offset, size, type, payload CRC-32) and an independent
  Python framing parse on every real file in the pinned manifest.
