# Changelog

All notable changes to `dwg-rs` will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
the project adopts [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once the public API stabilizes at 0.1.0.

## [Unreleased]

### Added — rendering pipeline primitives (2026-04-20)

Decoder-independent building blocks for the SVG / PDF / glTF / DXF
export paths. These ship without waiting on the common-entity
preamble fix (tracked below) so the downstream renderer work can
proceed in parallel.

- **`src/api.rs`**: `ParseMode { Strict, BestEffort }` enum,
  `Decoded<T> { value, diagnostics, complete }` wrapper with
  `complete()` / `partial()` / `map()`, `Warning { code, message,
  bit_offset }`, and a `Diagnostics` accumulator with `warn` /
  `warn_at` / `confidence(total)` / `is_clean`. Lays the API spine
  for the strict/lossy discipline planned across every public
  entry point.
- **`src/geometry.rs`**: `Point2D` / `Point3D` inherent methods
  (`add`, `sub`, `distance`, `lerp`, `new`), `VecOps` trait on
  `Vec3D` (`scale` / `dot` / `cross` / `length` / `normalize`),
  4×4 `Transform3` with `identity` / `translation` / `scale` /
  `rotation_z` / `compose` / `transform_point` / `transform_vector`,
  axis-aligned `BBox3` with empty-sentinel identity under union,
  and an indexed `Mesh` container (shared vertex list + triangle
  indices, `push_triangle` / `push_quad`).
- **`src/curve.rs`**: unified `Curve` enum (`Line` / `Circle` /
  `Arc` / `Ellipse` / `Polyline` / `Spline` / `Helix`) with
  conservative `bounds()` per variant, and `Path { segments,
  closed }` with `from_polyline` helper and union-of-segments
  bounds.
- **`src/color.rs`**: 256-entry ACI palette (`aci_to_rgb(u8)` →
  `(u8, u8, u8)` and `aci_to_hex(u8)` → `#RRGGBB`). Provenance
  noted in module docs.
- **`src/svg.rs`**: string-based SVG 1.1 writer (`SvgDoc::new` /
  `begin_layer` / `end_layer` / `push_curve` / `push_path` /
  `finish`). `Style { stroke, stroke_width, fill, dashes }`. CAD
  Y-up → SVG Y-down flip applied at the root `<g>`.
- **`src/dxf.rs`**: group-code DXF writer (`DxfWriter::new`,
  section balance enforced with `begin_section` / `end_section`,
  typed `write_string` / `write_int` / `write_double` /
  `write_point` / `write_handle` / `write_entity_header` /
  `write_comment`, terminated by `finish`). Panics on misuse
  (nested sections, finish-with-section-open, double-finish).
- **`src/limits.rs`**: new `WalkerLimits` struct for graph
  iteration (`max_handles`, `max_scan_bytes`, `max_block_nesting`)
  with `safe` / `paranoid` / `permissive` profiles mirroring
  `ParseLimits`.
- **`src/handle_map.rs`**: `HandleMap::iter()`, `len()`,
  `is_empty()`, and `IntoIterator for &HandleMap` so callers can
  walk `(handle, offset)` pairs without directly touching the
  `entries` field.

### Added — forensic + external surfaces

- **`examples/trace_common_entity.rs`**: forensic tracer that
  prints every common-entity preamble field's bit position and
  decoded value for the LINE at offset 11884 in `line_2013.dwg`.
  The output is the starting point for the ODA §19.4.1 R2004+
  cross-reference that closes the preamble-misalignment bug.
- **`examples/dump_line_payload.rs`**: bit-walk of the LINE
  payload (MC + object_type + handle) for manual verification
  against the spec.
- **`examples/test_h2_truncate.rs`**: empirical falsification of
  H2 (data-stream boundary bleed) — confirms the preamble field
  order itself is wrong, not a cursor-into-handle-stream bleed.
- **README**: capability matrix ("parsing / metadata / entities /
  geometry / write / IFC-equivalent" × shipped / alpha / pending)
  at the top, rvt-rs sibling cross-link in Related Projects.
- **CONTRIBUTING.md**: entity-decoder coverage is now the #1
  most-wanted contribution.
- **RELEASE.md**: SemVer commitment (with 0.x breakage window),
  cut-a-release runbook, yank / backport / deprecation policies.
- **docs/EXTENDING_DECODERS.md**: worked POINT example (struct,
  decoder fn, dispatcher wiring, tests, defensive caps).
- **cliff.toml**: git-cliff config for automated CHANGELOG
  generation from conventional commits.
- **`.github/ISSUE_TEMPLATE/corpus_submission.yml`**: licensed
  public-corpus submission flow.
- **`.github/ISSUE_TEMPLATE/unsupported_version.yml`**: AC1033+
  version-not-supported intake.
- **`fuzz/fuzz_targets/`**: three new libfuzzer harnesses
  (`classmap_parse`, `handlemap_parse`, `header_vars`) exercising
  all 8 supported versions.
- **GitHub Discussions** enabled on the repo.

### Changed

- `#![deny(unsafe_code)]` → `#![forbid(unsafe_code)]` in
  `src/lib.rs`. The crate ships with zero `unsafe`, so `forbid`
  is satisfiable and makes the invariant a hard compile-time
  error rather than a lint.
- `lz77::decompress` is now documented to clamp its output at
  256 MiB via `DecompressLimits::default()`; new regression test
  pins the contract (`default_limits_cap_output_at_256_mib`) and
  a compression-bomb test proves a 6-byte input claiming 1 TiB
  stays bounded (`small_input_with_huge_expected_size_stays_bounded`).

### Known — decoder-correctness regression discovered (task #97)

Task #97 (validate decoders against real R2013 corpus) surfaced a
deeper architectural gap than the dispatcher type-code bugs that
#71-#96 closed:

1. **Handle walk misses modelspace geometry.** The single-entity
   R2013 samples (`line_2013.dwg`, `circle_2013.dwg`, `arc_2013.dwg`)
   each decode 6 objects, all of which are empty `BLOCK`/`ENDBLK`
   shells. The user-drawn LINE/CIRCLE/ARC is stored at a handle
   reachable only through `BLOCK_HEADER → owned entities` — a
   traversal the current reader does not perform.
2. **Bit-cursor offset inside typed payloads is wrong on R2018.**
   `sample_AC1032.dwg` is the one corpus file where typed entity
   decoders fire on real data, and the results are garbage: LINE
   endpoints with `z = 1.2e+225`, POINT positions with
   `x = 4.4e+138`, CIRCLE centers with `z = -3.2e+113`. This
   indicates the cursor is not positioned where the spec says it
   should be after the common-entity preamble — either a bit-count
   error earlier in the pipeline or a missed preamble field in the
   R2018 layout.

Four integration tests in `tests/r2013_entity_values.rs` pin the
expected invariants. They are `#[ignore]`'d so `cargo test` stays
green; `cargo test --release -- --ignored` reproduces the regression
on demand. The "honest coverage" numbers below measure *dispatch
success*, not *value correctness*.

## [0.1.0-alpha.1] — 2026-04-19

First public pre-release. **Not production-ready.** See [README](./README.md)
for the full empirical coverage story; the short version is below.

### Scope reality check

- **Entity-decode end-to-end coverage**, measured by
  `examples/coverage_report.rs` against the `nextgis/dwg_samples` +
  `sample_AC1032.dwg` corpus (19 files) after the dimension-subtype
  correction (task #71):
  - R14 / R2000 / R2007 — **not supported** (no handle-map walker for these layouts yet).
  - R2004 — 0 / 21 entities decoded (**0 %**).
  - R2010 — 9 / 21 entities decoded (**43 %**).
  - R2013 — 18 / 21 entities decoded (**86 %**).
  - R2018 (`sample_AC1032.dwg`) — 66 / 306 entities decoded (**22 %**).
  - **Aggregate:** 93 / 369 attempted entities decoded = **25 %**.
- 439 objects in the R2018 sample are legitimate non-entity types
  (dictionaries, controls, symbol-table entries) that the dispatcher
  correctly returns as `Unhandled` — these are not counted as failures.
- Task #71 rewrote the dispatcher's fixed code table to match ODA spec
  §5 Table 4. Pre-fix numbers (27 % aggregate) counted structurally
  wrong dimension decodes as successes; post-fix numbers are the
  honest figure.

The gap between "all 27 entity decoders have passing unit tests" and
"27 % of real entities decode end-to-end" is exactly the
common-entity-preamble + object-stream layout work that 0.1.0 stable
will fix.

### Added

**Container layer (shipping, 193 tests green)**
- `DwgFile::open` / `DwgFile::from_bytes` — top-level reader.
- Version identification for AC1014, AC1015, AC1018, AC1021, AC1024, AC1027, AC1032.
- R13–R15 simple file header + R2004+ XOR-encrypted header + CRC-32 verify.
- LZ77 decompressor (ACadSharp-verified +1 offset dialect).
- Section Page Map + Section Info parser.
- `DwgFile::read_section(name)` for every named section.
- Reed-Solomon(255,239) over GF(256) decoder — Berlekamp-Massey + Chien + Forney.
- Metadata parsers: `SummaryInfo`, `AppInfo` (R18 ANSI + R21+ UTF-16 auto-detect),
  `Preview` (BMP / WMF / PNG code-6), `FileDepList`.
- `HandleMap`, `ClassMap`, `HeaderVars` parsers.
- `ObjectWalker` (R2004+ only) — `all_objects()` returns `Vec<RawObject>` with
  handle-indexed iteration. **Works reliably** on R2018 (745 objects enumerated
  from sample corpus file).

**Entity dispatcher (alpha)**
- 27 per-entity decoders under `src/entities/*.rs` (LINE, POINT, CIRCLE, ARC,
  ELLIPSE, RAY, XLINE, SOLID, 3DFACE, TRACE, SPLINE, TEXT, MTEXT, ATTRIB,
  ATTDEF, INSERT, BLOCK, ENDBLK, VERTEX, POLYLINE, LWPOLYLINE, DIMENSION (7
  subtypes), LEADER, IMAGE, HATCH, MLEADER, VIEWPORT).
- `DecodedEntity` typed enum + `decode_from_raw(raw, version)` dispatcher.
- `DwgFile::decoded_entities()` — end-to-end walk + dispatch + summary.
- `DispatchSummary` — honest bookkeeping (decoded / unhandled / errored).
- **All 27 decoders pass unit tests on synthetic input.** Real-world coverage
  is the 27 % cited above.

**Symbol tables + control objects**
- LAYER, LTYPE, STYLE, VIEW, UCS, VPORT, APPID, DIMSTYLE, BLOCK_RECORD under
  `src/tables/*.rs` — decoder functions exist, not wired into a walker
  dispatcher yet.
- DICTIONARY, XRECORD, `*_CONTROL` under `src/objects/*.rs`.

**Write path (partial)**
- Bit-writer: inverse of every BitCursor primitive, round-trip tested.
- LZ77 literal-only encoder (correctness-first; matcher pass is future work).
- `section_writer::build_section` — per-section framer with Sec_Mask XOR +
  CRC + LZ77. Verified: built sections decompress back to input bit-exactly.
- `file_writer::WriterScaffold` — stage-1 of 5 of a full `DwgFile::to_bytes()`
  pipeline. Stages 2–5 (page map, section info, system pages, file-open
  header) are scaffolded with an explicit roadmap in the module comment.

**R2007 Sec_Mask**
- Layer 1 (byte XOR with per-section LCG seed) — implemented, tested, NOT
  wired into reader yet.
- Layer 2 (7-byte window bit-rotation) — scaffolded, partial implementation.
- R2007 files currently parse header + section list only; section payloads
  return a placeholder error.

**CLI tools**
- `dwg-info`, `dwg-corpus`, `dwg-dump`, `dwg-convert`.
- `examples/coverage_report.rs` — the script that produced the empirical
  numbers above. Run it on your files before relying on decode output.

**Infrastructure**
- CI matrix: Linux / macOS / Windows × (stable, MSRV 1.85) ×
  fmt / clippy / test / doc / deny / msrv.
- `deny.toml` — supply-chain policy: Apache-2 / MIT / BSD / ISC / Zlib /
  Unicode-3.0 / MPL-2.0 / CC0-1.0 allowed; GPL denied; crates.io-only sources.
- Dependabot — weekly cargo + monthly actions.
- Issue + PR templates with clean-room declaration checkbox.
- SECURITY.md with private reporting flow + threat model.
- CITATION.cff for academic citations.
- ARCHITECTURE.md — technical deep-dive.
- Fuzz scaffolding: 5 `cargo-fuzz` targets (lz77_decompress,
  bitcursor_primitives, dwg_file_open, section_map, object_walker) under
  `fuzz/`. Compile-verified; overnight sweep is pre-1.0 work.

### Safety

- `#![deny(unsafe_code)]` on the entire crate.
- 193 tests: 156 unit + 5 corpus + 9 proptest + 22 sample-specific + 1 doctest.
- `cargo clippy --all-targets --all-features -- -D warnings` clean.
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` clean.
- `cargo fmt --all -- --check` clean.
- `cargo publish --dry-run` succeeds — 89 files, 129 KB compressed.

### What's deferred

These block 0.1.0 stable:

1. **Common-entity preamble fixes** to lift R2004 / R2010 / R2018 entity decode
   coverage from 0–22 % to >90 %. This is the highest-impact work item.
2. **R14 / R2000 object-stream walker** — different layout from R2004-family.
3. **R2007 Sec_Mask layer-2 bookkeeping** — spec §5.2.
4. **Table-entry dispatcher** — the equivalent of `DecodedEntity` for
   symbol-table records; today each table-entry decoder is call-it-yourself.
5. **Fuzz session** — first overnight run of the 5 targets under `fuzz/`.
6. **Write path stages 2–5** — `DwgFile::to_bytes()` file-level assembly.

### Legal posture

Clean-room — no Autodesk SDK, no ODA SDK, no LibreDWG (GPL-3) source
consulted. Implemented against the ODA's freely-redistributable *Open Design
Specification for .dwg files* (v5.4.1). Where the spec is ambiguous in one
place (an LZ77 offset-encoding corner), the authors consulted a publicly
documented errata reading via algorithm descriptions only — no implementation
code was reviewed or ported.

### Not yet

- Not published to crates.io.
- No official release tarball.
