# Status — 2026-08-30

A plain-text snapshot of what has shipped in this crate, organized
by the task-tracker labels, so contributors can orient without
scrolling the changelog.

## Summary

- **Lib tests:** 766 passing in default/debug and release-all-features
  profiles, clippy + fmt clean.
- **WASM tests:** 40 passing in `wasm/` sub-crate.
- **Integration tests:** DXF round-trip (7), glTF smoke (3), SVG
  goldens (3), fuzz-corpus regression (6), write-path (5),
  entity-regression (18), real-DWG value regression (8),
  legacy-container (11: R14 / R2000 byte-built flat files, the R2007
  string-stream boundary, and four corpus-gated pins).
- **Current real-file decode coverage:** 3,695 decoded / 741 skipped /
  **9 errored** / 83.1% on the local 19-file `samples/` corpus. The
  R2018 `sample_AC1032.dwg` sample is 776 / 57 / 9 / 92.2%. Per
  version: R14 48.6%, R2000 84.4%, R2004 97.5%, R2007 82.8%,
  R2010 97.8%, R2013 97.0%, R2018 92.2%.
- **All nineteen corpus files now decode.** Until #104/#110 the nine
  R14 / R2000 / R2007 files reported `n/a (no-handle-map)` — no record
  of those releases reached a decoder, so none of them counted in
  either column. The aggregate rate therefore *fell* from 95.6% while
  the decoded count rose by 1,422: the old figure averaged ten files,
  the new one nineteen. The per-version rows are comparable; the
  aggregates are not.
- **Every entity record is boundary-checked** (#63). The nine errors
  are the honest residue of turning that check on for the types that
  previously had none — see the honesty note below. They are not a
  regression from the 2,262 / 116 / **0** of #67: the same nine records
  "decoded" then, without anything checking that they had.
- **Handle-map completeness:** every one of the 842 `AcDb:Handles`
  entries on `sample_AC1032.dwg` resolves to a record whose own handle
  field matches the map, and the walked records cover 1,191,935 of the
  section's 1,192,851 bytes (916 unclaimed, longest run 4 bytes).
- **Fuzz targets:** 9 (lz77 / bitcursor / dwg-file-open / section-map /
  object-walker / classmap / handlemap / header-vars / rs-fec).
  Seed corpus: 30 hand-crafted inputs across all targets.
- **CLI binaries (7):** `dwg-info`, `dwg-corpus`, `dwg-dump`,
  `dwg-convert`, `dwg-to-dxf`, `dwg-to-gltf`, `dwg-write`.
- **CI workflows:** `ci`, `docs-rs`, `perf`, `release`, `fuzz-nightly`,
  `wasm`, `pages`.

## Shipped

### Container layer (Phase 1)
- File identification across 8 DWG versions (R14 → R2018).
- **R13-R15 flat section locators (§3.2.6)** named with the canonical
  `AcDb:` names and read as byte ranges; the object map's offsets are
  absolute file offsets, so the whole file is the object stream (#104).
- **R2007 (`AC1021`) container (§5.1-§5.4)** — Reed-Solomon
  de-interleave of the 0x400-byte file header, the §5.10 LZ variant
  (`src/r21_lz.rs`), the page map, the section map and per-page data
  assembly (#110). Every constant §5.2 documents comes back correct on
  all three corpus files, and all twelve tabulated section hash codes
  match. Password-protected files are refused, not mis-decoded.
- LZ77 decompression with spec-errata fixes + `DecompressLimits`.
- Section Page Map + Section Info walking.
- Sec_Mask layer-1 XOR masking.
- CRC-8 + CRC-32 verification (section + page + file-header).
- Reed-Solomon (255, 239) verify + multi-codeword stream decode.

### Metadata (Phase 2)
- `SummaryInfo`, `AppInfo`, `Preview` (PNG carve), `FileDepList`.
- Auto UTF-16 detection for R2007+ strings.

### Object stream (Phase 3)
- `ObjectWalker` with typed dispatch + `DispatchSummary`.
- Handle map + class map parsers (with writer-side inverses).
- Strict / lossy / count-cap variants.
- R2013/R2018 common-entity/body boundary pinned for LINE/CIRCLE/ARC
  with active real-DWG regression tests.

### Entity decoders (Phase 4, 59 modules under `src/entities/`)
- LINE, CIRCLE, ARC, ELLIPSE, POINT, LWPOLYLINE, POLYLINE 2D/3D.
- TEXT, MTEXT, INSERT, ATTRIB/ATTDEF, BLOCK/ENDBLK.
- SPLINE, HATCH, MLEADER, LEADER, TOLERANCE.
- DIMENSION base + linear / aligned / radial / diameter /
  angular 2-line / angular 3-point / ordinate subclass decoders.
- MESH (subdivision) / POLYLINE_PFACE / POLYGON MESH.
- 3DFACE, and the three §20.4.41 ACIS entities — 3DSOLID, REGION,
  BODY — with the full record (ACIS envelope, wireframe/isoline block,
  the R2007+ trailing `BL` and the R2013+ data-store revision GUID),
  closing on all three corpus records (ARCHITECTURE.md §7h).
- SURFACE: extruded / revolved / swept / lofted.
- CAMERA, LIGHT, SUN, HELIX.
- IMAGE / IMAGEDEF, UNDERLAY (PDF/DWF/DGN), GEODATA.
- OLE2FRAME, WIPEOUT, MLINE.
- PROXY entity / PROXY object (opaque pass-through).
- RAY, XLINE, VIEWPORT, TRACE, SOLID-2D.
- SEQEND, POLYLINE_3D, VERTEX_3D / VERTEX_MESH / VERTEX_PFACE,
  VERTEX_PFACE_FACE — wired in #63, each field list pinned to `delta 0`
  on every corpus record of its type and cross-checked against the
  object stream: the POLYLINE_PFACE at handle `0x422` of
  `sample_AC1032.dwg` declares 5 vertices, 2 faces and 7 owned objects
  and is followed by exactly 5 VERTEX_PFACE, 2 VERTEX_PFACE_FACE and a
  SEQEND; the POLYLINE_3D at `0x42B` declares 5 owned objects and is
  followed by exactly 5 VERTEX_3D and a SEQEND.
- `LINE` end coordinates use DD fields with each start coordinate as
  the default; `line_2013.dwg` now asserts the authored
  `(50, 50, 0) -> (100, 100, 0)` geometry.
- `LWPOLYLINE` vertices use first-point `RD/RD`, then subsequent
  `DD/DD` values with the previous point as default. The AC1032 sample
  now asserts at least 10 finite, nondegenerate LWPOLYLINE bodies.

### Symbol tables (Phase 6)
- LAYER, LTYPE, STYLE, VIEW, UCS, VPORT, APPID, DIMSTYLE.
- BLOCK_RECORD.
- R2004 (AC1018) object prologue: the `RL` object-data-size-in-bits
  field (§19.1) is read across the whole R2000..R2007 band, and the
  non-entity common object data (§19.4.2 — EED chain, `BL` reactor
  count, xdictionary flag) is consumed before every pre-R2007 table
  decoder. `RawObject::obj_size_bits` exposes the field as the
  pre-R2010 analogue of the string-stream start bit.
- R2007+ split-stream (`src/string_stream.rs` + `src/tables/modern.rs`)
  for LAYER / LTYPE / STYLE / UCS / VIEW / VPORT / APPID / DIMSTYLE /
  BLOCK_HEADER, plus the TEXT / ATTRIB / ATTDEF / MTEXT / TOLERANCE /
  HATCH / DIMENSION entities. Each modern
  decoder asserts its data fields end exactly on the string-stream
  start bit, so a wrong layout errors rather than returning garbage.
- Named-object dictionary, ACAD_GROUP, ACAD_SCALE, ACAD_VISUALSTYLE
  (R2010+, 58 properties on R2013/R2018), ACAD_PROPERTYSET_DATA, and
  LAYOUT + PLOTSETTINGS (one §20.4.84 field list, closing on all 31
  corpus LAYOUT records across R2004, R2010, R2013 and R2018).
- MLINESTYLE from the §20.4.73 prescription, closing on all 10 corpus
  records across R2004, R2010, R2013 and R2018, and MLEADERSTYLE,
  ACDBDETAILVIEWSTYLE and ACDBSECTIONVIEWSTYLE from the joint-boundary
  search — 33 further records, all four release bands, delta 0 on every
  one (ARCHITECTURE.md §7d.3, §7d.4).
  ACAD_MATERIAL reads only its strings and its measured bit budget —
  its data-field layout is not determined.

### Graph + geometry (Phase 5 + 8)
- `resolve_entity` / `owner_chain` / `reactor_chain`.
- `resolve_layer` / `resolve_linetype` / `resolve_text_style` /
  `resolve_dim_style`.
- Cycle detection + `WalkerLimits::max_handles` cap.
- Entity → curve/path/mesh adapters (27+ entity types).
- LWPOLYLINE bulge-to-arc, SPLINE NURBS, TEXT baseline,
  DIMENSION paths, HATCH multi-path fills, 3DFACE triangle/quad,
  3DSOLID bbox placeholder, INSERT transform composition.
- `BlockSpace::{Model, Paper, Custom}` classification +
  filtering + `ViewportTransform` for paper-space rendering.
- Block expansion (`block_expansion.rs`) with cycle + depth caps.

### Rendering (Phase 9 + 10)
- SVG writer: text, MTEXT (6 formatting codes), hatch (solid +
  patterns + dedupe), dimension (linear), layer visibility,
  linetype → stroke-dasharray, paper space + title block +
  viewport clip, paged-SVG PDF export.
- glTF 2.0 writer: per-layer PBR materials from ACI, entity →
  primitive, transform composition, .glb / .gltf formats.
- DXF writer: 8 target versions (R12..R2018), HEADER / TABLES /
  BLOCKS / ENTITIES / OBJECTS sections.

### CLI (Phase 11)
- 7 binaries (listed above).
- All behind `cli` feature flag.

### Writer (Phase 12)
- `BitWriter` with write_b / bb / bs / bl / bll / bd / rc / rs /
  rl / rd + position-bits fix + signed-MC i64::MIN edge-case fix +
  try_write_3b.
- LZ77 literal-only encoder.
- Reed-Solomon (255, 239) encoder.
- `WriterScaffold` for section-level framing.
- Version magic + file-header writer (`build_version_header`).
- `atomic_write` via temp + rename.
- `validate_section_name` against `KNOWN_SECTION_NAMES`.
- CRC-8 + CRC-32 embedders.
- `ElementEncoder` trait + Line/Circle/Arc/Point implementations.
- `HandleAllocator` for handle allocation strategy.
- `write_class_map` + `write_handle_map` inverses.
- `dwg-write` scaffolding CLI.
- Stage 3 page-map + Section Info assembly.
- Stage 4 CRC splicing.
- Stage 5 final byte buffer (`assemble_dwg_bytes`).
- Experimental `DwgFile::to_bytes()` for R2004-family section
  round-trips.

### WASM (Phase 13)
- `wasm/` subcrate with wasm-bindgen + js-sys + serde-wasm-bindgen.
- `DwgFile.open` / `versionMagic` / `versionName` / `sections` /
  `sectionMapStatus` (V-02).
- 2D SVG skeleton renderer (V-03).
- Viewer pan / zoom / fit-to-view (V-05).
- Layer panel + linetype → SVG stroke-dasharray (V-06, V-07).
- Hatch / text / dimension render helpers (V-08, V-09, V-10).
- Block expansion + space toggle + print preview stubs
  (V-11, V-12, V-13).
- Export buttons: SVG / DXF / glTF (placeholder) / PDF (V-14).
- Client-side-only attestation + CI enforcement (V-19).
- Sample DWG fixtures (V-20).
- SectionBox API stub (V-21) — 3D clipping deferred to V-04.
- Measurement tool: distance + polygon area (V-22).
- Selection + entityProperties stubs (V-23).
- URL-shareable ViewerState serialization (V-24).
- Drag-and-drop JS glue + MIME/extension constants (V-18).
- WebWorker readiness attestation (V-16).
- Progressive-open stub (V-17).
- Static-site GitHub Pages workflow (V-15).

### Quality + release (Q-series)
- Criterion benchmark suite: lz77, section_map, object_walk,
  metadata_parse, libredwg_compare (Q-03).
- LibreDWG compat baseline bench (Q-04).
- dhat memory profiling (Q-05).
- Perf regression gate in CI (Q-06).
- Release workflow with crates.io manual approval + 7 × binary
  matrix + PyPI scaffold (Q-07).
- docs.rs build validation pre-release (Q-09).
- Compatibility matrix landing page (Q-02).

### Documentation (DOC + DEV + L-series)
- 8 DOC artifacts (RELEASE, ROADMAP, SemVer policy, compat
  matrix, Python bindings, rvt-rs cross-link, recon §Q).
- 11 launch posts (blog, HN, r/rust, r/cad, LinkedIn, Reddit,
  Twitter, LibreCAD/FreeCAD/QCAD forums, ODA community).
- 11 DEV docs (CONTRIBUTING, CLEANROOM, THREAT_MODEL, EXTENDING_DECODERS,
  entity-decoder cargo-generate template, synthetic DWG generator,
  GitHub Discussions, issue + PR templates).

### Security (SEC-series)
- `DecompressLimits` + LZ77 output cap (SEC-01, 02, 03, 04).
- `Error::Lz77OutputLimitExceeded` and
  `Error::Lz77BackrefTooLong` variants (SEC-05).
- Compressed-bomb defense test (SEC-06).
- `OpenLimits` (file + section + decompress caps) (SEC-07, 08).
- `read_section_with_limit` per-call byte cap (SEC-09).
- Python binding cap exposure remains pending with the PyO3 package (SEC-10).
- `#![forbid(unsafe_code)]` crate-wide (SEC-11, 13).
- `THREAT_MODEL.md` (SEC-30) + `CLEANROOM.md` (SEC-31).
- Soft legal language (SEC-32).
- cargo audit + cargo deny CI (SEC-25, 26, 27).
- All third-party actions SHA-pinned (SEC-28).
- Top-level `contents: read` CI permissions (SEC-29).
- 9 fuzz targets (SEC-14..SEC-22) + nightly cargo-fuzz CI
  (SEC-24) + seed corpus (SEC-23).

## Pending — non-trivial work remaining

These have genuine open scope requiring focused work, not stubs.

- **Current real-file decode baseline:** the 2026-08-30
  `examples/coverage_report.rs ../../samples` run reports 2273 decoded,
  96 skipped, 9 errored, 95.6% aggregate coverage. This is the
  practical product-readiness blocker even though synthetic decoder
  tests are broad.

- **#63 which entity types are exact-checked — the honesty note.**
  Every entity type this crate decodes is now routed through
  `entities::dispatch::checked_inline`, so its field list must end on
  the record's own data-stream boundary — the string-stream start bit
  (or the bit before the `strings present` trailer flag) on R2010+, and
  the `RL` object-data-size on the earlier bands. **The number of entity
  types that decode unchecked is zero, on every release:**

  | Band | Boundary | Checked |
  |---|---|---|
  | R2010 / R2013 / R2018 | string-stream start, or handle-stream start − 1 | **all types** |
  | R2007 | string-stream start, located from the §19.1 `RL` (#65) | **all types** |
  | R2000 / R2004 | `RL` object-data-size-in-bits, from the object prologue | **all types** |
  | R13 / R14 | the same `RL`, which §20.4.1 places inside the common entity data instead | **all types** |

  Nine corpus records now fail the check. All nine are decoders whose
  field list is genuinely unfinished, and each is documented at the
  module that owns it:

  | Type | Records | Delta | Why |
  |---|---|---|---|
  | VIEWPORT | 6 (R2018) | −819 | the decoder reads 266 of the record's 1125 bits by design (`entities/viewport.rs`); six records of one identical shape give no variation to derive the rest from |
  | MESH | 2 (R2018) | −2 | two trailing bits reading `10` — the zero/default code of `BS`, `BL` and `BD` alike, so one file cannot say which (`entities/mesh.rs`) |
  | LEADER | 1 (R2018) | −12 | twelve bits several continuations of the field list would fit; one record cannot choose (`entities/leader.rs`) |

  Types whose check is wired but which no corpus record exercises:
  POLYLINE_2D, POLYLINE_MESH, VERTEX_2D, VERTEX_MESH, TRACE, OLE2FRAME,
  CAMERA, SUN, LIGHT, GEODATA, BODY, WIPEOUT, HELIX and the four
  SURFACE variants. Their field lists are unverified; the check is what
  will arbitrate them when a record appears.

- **#33 remaining non-entity objects.** DICTIONARY, DICTIONARYVAR,
  XRECORD, ACDB_PLACEHOLDER, the ten `*_CONTROL` owners, ACAD_GROUP and
  ACAD_SCALE now dispatch through `src/objects/modern.rs`, taking their
  `TV` fields from the R2007+ string stream and checking their data
  fields end exactly on the record's data-stream boundary.
  VISUALSTYLE dispatches on R2004, R2010, R2013 and R2018 — 240 of its
  240 corpus records, the R2004 band on a second, flag-less field list
  measured against all 72 of its records — LAYOUT (with its embedded
  PLOTSETTINGS block)
  dispatches on all 31 of its corpus records, and MLINESTYLE (10),
  MLEADERSTYLE (11), ACDBDETAILVIEWSTYLE (11) and
  ACDBSECTIONVIEWSTYLE (11) dispatch on all of theirs. Still
  unreached, in descending record count on the corpus (counts as of
  the 2026-08-30 run): MATERIAL (38), TABLESTYLE (10). MATERIAL and PROPERTYSET_DATA decode
  only a documented prefix of their fields, so they cannot satisfy the
  boundary check; TABLESTYLE's block structure is measured
  (ARCHITECTURE.md §7d.4) but a single record per corpus file cannot
  pin the token sequence inside its cell-style blocks.

- **#54 the R2013+ `has AcDs binary data` marker — settled.** The
  three ACIS entity records that set the bit (3DSOLID `0xD65` /
  `0xD6A`, REGION `0xD69` of `sample_AC1032.dwg`) now have a field
  list that closes, and it closes only with **zero** bits consumed
  after the flag. `common_entity.rs` and `objects/modern.rs` agree on
  that reading; the 16 bits four LAYOUT records need moved into
  `objects/acad_layout.rs` as LAYOUT's own data-store block.
  `tables/modern.rs` keeps its `RC` — that path also omits the
  `BL num_reactors` the object path reads, so its bit accounting has a
  second unresolved variable (see the next item) and was left alone.
  Evidence: ARCHITECTURE.md §7h, `examples/probe_acis_records.rs`.

- **R2007+ symbol-table common object prefix.** `tables/modern.rs`
  omits the `BL num_reactors` that `objects/modern.rs` measured as
  present on every non-entity object, and compensates with a different
  flag order, so both readings land on the string stream. Which one
  assigns the right values to the right fields is not determinable
  from the 16 bits an APPID record spends there; the names are
  unaffected because they come from the string stream either way.

- **#103 remaining real-file decoder alignment** (P0, unchanged scope,
  larger corpus). The R2007+
  symbol table, TEXT / ATTRIB / ATTDEF / MTEXT / TOLERANCE / HATCH /
  MULTILEADER / the DIMENSION family / INSERT / SPLINE / LWPOLYLINE /
  3DFACE / the UNDERLAY family read their `TV`s from the string
  stream, treat an `H` slot as consuming no data bits, and assert the
  data fields end exactly on the record's data-stream boundary — and
  since #63 so does every other entity type, on every band. What is
  left under this issue is mostly *coverage*: the 741 skipped records
  belong to types with no field list matched against real bytes yet,
  and 645 of them are in the three release bands that only became
  walkable with #104/#110. The alignment residue is the nine errored records
  tabulated in the #63 note above.

- **#109 Reed-Solomon FEC read-side wiring.** The multi-codeword
  stream decoder shipped (SEC-04, #279); wiring it into
  `section_map` as a fallback path when CRC-8 fails is a separate
  cut.
- **VISUALSTYLE on R14 / R2000 / R2007 — 216 records, layout not
  determined.** #64 measured the R2004 "flag-less" field list and #47
  the R2010+ `(value, flag)` one. Neither closes on the three earlier
  bands: all 24 records of each of those nine files miss their
  data-stream boundary under the R2004 list, so a third layout exists.
  The records stay unhandled rather than decoded from a list that does
  not close (`objects/acad_visual_style.rs`).
- **R14 DIMSTYLE.** §20.4.68 gives R13/R14 their own block — the flags
  come first as `B`s and `RC`s, then the `BD` variables, in an order
  that shares nothing with the R2000+ list. Two records per R14 file
  report unhandled. The R2000+ inline list itself is now complete and
  closes on delta 0 for R2000 and R2004
  (`tables::dimstyle::decode_r2000_inline`).
- **#136 Cargo workspace split.** Refactor the crate into
  `dwg-core` + `dwg-cli` + `dwg-fuzz` (+ keep `dwg-wasm`).
  Mechanical but affects every path reference.
- **#386 L12-13 cross-version write via DXF.** Would require a
  DXF parser; currently out of scope since DXF parsing is its
  own 3KLOC project.
- **#391 V-04 Three.js 3D viewer.** 3D rendering in the browser
  needs a JS dependency + 3DFACE/MESH→glTF/Three.js bridge. The
  SectionBox API stub (V-21) is in place for when this lands.

## How to contribute

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the source-provenance policy and
development workflow. Good first issues are labeled `good-first-issue`
in the GitHub tracker; the 7 items above are the "meaty" open scope.
