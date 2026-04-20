# Status — 2026-04-20

A plain-text snapshot of what has shipped in this crate, organized
by the task-tracker labels, so contributors can orient without
scrolling the changelog.

## Summary

- **Lib tests:** 645+ passing across all profiles, clippy + fmt clean.
- **WASM tests:** 39 passing in `wasm/` sub-crate.
- **Integration tests:** DXF round-trip (7), glTF smoke (3), SVG
  goldens (3), fuzz-corpus regression (6), write-path Stage-1 (4),
  entity-value regression (18).
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

### Entity decoders (Phase 4, 27 modules)
- LINE, CIRCLE, ARC, ELLIPSE, POINT, LWPOLYLINE, POLYLINE 2D/3D.
- TEXT, MTEXT, INSERT, ATTRIB/ATTDEF, BLOCK/ENDBLK.
- SPLINE, HATCH, MLEADER, LEADER, TOLERANCE.
- DIMENSION base + linear / aligned / radial / diameter /
  angular 2-line / angular 3-point / ordinate subclass decoders.
- MESH (subdivision) / POLYFACE MESH / POLYGON MESH.
- 3DFACE, 3DSOLID (SAT passthrough), REGION, BODY.
- SURFACE: extruded / revolved / swept / lofted.
- CAMERA, LIGHT, SUN, HELIX.
- IMAGE / IMAGEDEF, UNDERLAY (PDF/DWF/DGN), GEODATA.
- OLE2FRAME, WIPEOUT, MLINE.
- PROXY entity / PROXY object (opaque pass-through).
- RAY, XLINE, VIEWPORT, TRACE, SOLID-2D.

### Symbol tables (Phase 6)
- LAYER, LTYPE, STYLE, VIEW, UCS, VPORT, APPID, DIMSTYLE.
- BLOCK_RECORD.
- Named-object dictionary, ACAD_GROUP, ACAD_MLINESTYLE,
  ACAD_PLOTSETTINGS, ACAD_SCALE, ACAD_MATERIAL, ACAD_VISUALSTYLE,
  ACAD_PROPERTYSET_DATA, ACAD_LAYOUT.

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
  rl / rd + position-bits fix + MC edge-case fix + try_write_3b.
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
- Release workflow with crates.io manual approval + 5 × binary
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
- `Error::DecompressLimitExceeded` variant (SEC-05).
- Compressed-bomb defense test (SEC-06).
- `OpenLimits` (file + section + decompress caps) (SEC-07, 08).
- `read_section_with_limit` per-call byte cap (SEC-09).
- Python bindings expose all caps as kwargs (SEC-10).
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

- **#103 R2018 bit-cursor misalignment** (P0, partially traced to
  line.rs). Requires a 2nd R2013 corpus sample with known
  coordinates for hypothesis falsification. This is the biggest
  single blocker for `decoded_entities()` decode-rate improvements
  across all recent versions.
- **#104 R14 / R2000 / R2007 handle-map walker.** Container layer
  ships for these versions, but the object-stream walker is
  R2004+ only. Unlocks `decoded_entities()` for those release
  families.
- **#109 Reed-Solomon FEC read-side wiring.** The multi-codeword
  stream decoder shipped (SEC-04, #279); wiring it into
  `section_map` as a fallback path when CRC-8 fails is a separate
  cut.
- **#110 R2007 Sec_Mask layer-2.** The second obfuscation layer
  on top of the R2004-family Sec_Mask. Container parse returns
  `SectionMapStatus::Deferred` for R2007 until this lands.
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

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the clean-room posture and
development workflow. Good first issues are labeled `good-first-issue`
in the GitHub tracker; the 7 items above are the "meaty" open scope.
