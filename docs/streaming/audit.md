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
