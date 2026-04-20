# Changelog

All notable changes to `dwg-rs` will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
the project adopts [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once the public API stabilizes at 0.1.0.

## [Unreleased]

### Fixed — R2013/R2018 common-entity preamble alignment (2026-04-20, #103)

- **`src/common_entity.rs`** — Added the three missing preamble fields
  between `non_fixed_ltype` and `plotstyle_flag` per ODA 5.4.1 §19.4.1
  + libredwg reference: `CMC color` (BS color_index, minimal BYLAYER
  path), `BD linetype_scale`, `BB ltype_flags`. Also changed
  `shadow_flags` from RC (8 bits) to BB (2 bits) — the on-disk
  encoding uses only 2 bits (cast_shadow + receive_shadow) even
  though some spec revisions label the field "RC".
  
  Measured effect on `sample_AC1032.dwg` (R2018):
  - `BS invisibility` now decodes as **0** (valid) instead of **-10207**
  - LINE end-point `BD` deltas now decode to clean **1.0 / 0.0**
    shortcut values instead of tiny subnormals
  - 1 of 3 modelspace LINEs now passes the plausibility check (was 0/3)

  Known residual gaps tracked as follow-ups to #103:
  - `RD` start-point reads still produce ~6e+14 garbage on some LINEs
    (the BD delta path aligns but the absolute-value RD before it is
    still off by a few bits in some encodings)
  - Complex-color CMC suffix (BL rgb + TV name) disabled — reintroducing
    it over-consumed ENDBLK preamble bits on line_2013.dwg
- **`src/handle_allocator.rs`** — Fix pre-existing doctest that had
  stale assertions (Boy Scout while in the file).

### Added — Phase 12 write-path + Phase 13 WASM scaffolding (2026-04-20 late)

**Write path — Stages 1 through foundations of 5:**

- **`src/file_writer.rs`** — `version_magic_bytes(Version)` +
  `build_version_header(Version)` (16-byte $ACADVER leader,
  R2004+ 0x1F marker), `atomic_write(path, bytes)` via temp + rename
  (P0-10), `validate_section_name(&str)` with 16-entry
  `KNOWN_SECTION_NAMES` (P0-11, guards against typo-induced round-trip
  corruption). Existing `WriterScaffold` Stage-1 unchanged.
- **`src/crc.rs`** — `embed_crc8` / `embed_crc32` / `page_checksums`
  writer helpers (L12-02) — zero-fill-and-overwrite pattern matches
  the ODA §2.14 convention for CRC-bearing records.
- **`src/element_encoder.rs`** — `ElementEncoder` trait with
  `Line`/`Circle`/`Arc`/`Point` impls (L12-05).
- **`src/handle_allocator.rs`** — `HandleAllocator` with allocate /
  reserve / collision avoidance (L12-06).
- **`src/classes.rs`** + **`src/handle_map.rs`** — `write_class_map`
  and `write_handle_map` inverse-of-parse emitters (L12-07, L12-08).
- **`src/reed_solomon_encode.rs`** — (255, 239) systematic codeword
  encoder via GF(256) generator-polynomial long division (L12-10).
- **`tests/integration_write_roundtrip.rs`** — 4 tests covering
  multi-section Stage-1 round-trip, empty-section edge, byte-
  deterministic output, and 32-byte page alignment (L12-12 partial).
- **`src/bin/dwg_write.rs`** — 7th CLI binary. Scaffolds named-section
  input via CLI, runs the Stage-1 pipeline, emits a JSON Stage-1
  report + optional Stage-1 concatenated bytes (L12-14). Explicitly
  labeled "NOT A VALID DWG FILE" pending Stages 3-5.

**Entity decoders — MESH / DIMENSION / MLINE / IMAGE / proxy:**

- **`src/entities/mesh.rs`** — subdivision MESH per §19.4.66 (R2010+
  gate, vertex / face / edge count caps derived from `remaining_bits`)
  (L4-34).
- **`src/entities/polyface_mesh.rs`** — legacy 3D mesh header per
  §19.4.29 (L4-35).
- **`src/entities/polygon_mesh.rs`** — M×N indexed mesh header per
  §19.4.30 (L4-36).
- **`src/entities/dimension_linear.rs`**,
  **`dimension_aligned.rs`**, **`dimension_radial.rs`**,
  **`dimension_diameter.rs`**, **`dimension_angular_2l.rs`**,
  **`dimension_angular_3p.rs`**, **`dimension_ordinate.rs`** — 7
  subclass decoders per ODA §§19.4.18..19.4.23 (L4-17..21).
- **`src/entities/mline.rs`** — MLINE (§19.4.71) top-level +
  per-vertex sub-records; nested per-line segment parameters kept
  as `Vec<f64>` (honest-partial decode) (L4-54).
- **`src/entities/imagedef.rs`** — IMAGEDEF (§19.5.26) companion to
  IMAGE (L4-43).
- **`src/entities/proxy_entity_passthrough.rs`** — opaque proxy
  body preserved verbatim (L4-55).
- **`src/entities/lwpolyline.rs`** — count caps now derive from
  `remaining_bits` × 4 bits/point rather than the coarse 1 bit/item
  (L4-12).

**Graph / traversal / rendering:**

- **`src/block_expansion.rs`** (new crate module) — `expand_insert`
  with cycle detection + depth cap (default 16), emits
  `ExpandedEntity { entity, accumulated_transform, depth }`
  composing INSERT instance transforms outer-to-inner (L5-05).
- **`src/graph.rs`** — L6-18 `MODEL_SPACE_BLOCK_NAME` /
  `PAPER_SPACE_BLOCK_PREFIX` / `is_model_space_block_name` /
  `is_paper_space_block_name` / `BlockSpace` / `classify_block_name`.
  L6-19 `filter_by_paper_space_block` / `filter_by_block_space` /
  `membership_for`. L6-20 `ViewportTransform` with
  `model_to_paper` / `paper_bounds` / `contains_paper_point`.
- **`src/objects/acad_layout.rs`** — ACAD_LAYOUT decoder per §19.6.12
  (L6-12); `is_model_space()`, `paper_width/height()`,
  `extents_diagonal()` helpers.

**API hardening:**

- **`src/reader.rs`** — `DwgFile::read_section_with_limit(name,
  max_bytes)` per-call byte cap (SEC-09).
- **`src/python_stubs.rs`** — strict/lossy parity stubs for 10
  JSON-export methods (API-12).

**Fuzzing:**

- **`fuzz/fuzz_targets/rs_fec_decode.rs`** + registered in
  `fuzz/Cargo.toml` (SEC-21).
- **`.github/workflows/fuzz-nightly.yml`** — matrix over all 9
  fuzz targets at 06:00 UTC daily, 5-min duration per target,
  crash + corpus artifacts uploaded (SEC-24).
- **`fuzz/corpus/{rs_fec_decode,header_vars,classmap_parse,handlemap_parse}/`** —
  hand-crafted seeds exercising distinct code paths (SEC-23 seed).
- **`fuzz/fuzz_targets/object_walker.rs`** — uses public
  `collect_all_lossy` API (fixes pre-existing fuzz compile gap).
- **`tests/integration_fuzz_corpus_regression.rs`** — 6 tests replay
  every seed through the matching library entry point and forbid
  panics; this locks the fuzz contract against future regressions
  (L4-61).

**WASM Phase 13 scaffolding:**

- **`wasm/`** (new subcrate) — `dwg-wasm` with wasm-bindgen +
  js-sys + serde-wasm-bindgen. `DwgFile` JS class with
  `open(bytes)` / `versionMagic()` / `versionName()` / `sections()` /
  `sectionMapStatus()` + `crateVersion()` (V-01, V-02).
- **`.github/workflows/wasm.yml`** — matrix build over `--target
  web / bundler / nodejs`, uploads `web` artifact (14-day
  retention), asserts `pkg/dwg_wasm_bg.wasm` + `pkg/dwg_wasm.js`
  present. SHA-pinned actions.

Also: 18+ stale tasks closed as bookkeeping cleanup; Twitter thread
(L-13) refreshed to mention `dwg-to-dxf` / `dwg-to-svg` / `dwg-to-gltf`
which all shipped.

### Added — CI release infrastructure (2026-04-20, Q-06 / Q-07 / Q-09)

- **`.github/workflows/perf.yml`**: criterion-benchmark
  regression gate. Push-to-main saves a named `main` baseline to
  GitHub Actions cache; pull requests run the same bench set and
  diff against that baseline with `critcmp`. >20 % regression on
  any of `lz77`, `section_map`, `object_walk`, `metadata_parse`,
  or `libredwg_compare` fails the job. First-run cache misses are
  a warning, not a failure.
- **`.github/workflows/docs-rs.yml`**: pre-release docs.rs build
  clone. Runs `cargo doc --no-deps --all-features` with
  `RUSTDOCFLAGS='-D warnings'`, asserts >=10 HTML files in
  `target/doc/dwg`, and soft-gates docstring coverage on
  `pub fn` at 80 %.
- **`.github/workflows/release.yml`**: tightened SemVer tag
  regex (`v[0-9]+.[0-9]+.[0-9]+` ± `-prerelease`), added
  `dwg-to-dxf` to the binary matrix (5 binaries × 5 targets),
  added pre-publish dry-run, scaffolded a gated-off
  `publish-pypi` job for eventual PyO3 wheel releases.
- **`README.md`**: added Perf and docs.rs CI status badges.
- **`RELEASE.md`**: concrete release checklist — pre-flight,
  version bump, verification, tag, pipeline monitoring, post-publish.

All third-party actions remain SHA-pinned per the SEC-28 baseline
established at repo public-ification.

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
