# Changelog

All notable changes to `dwg-rs` will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
the project will adopt [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once the public API stabilizes.

## [Unreleased] — 0.1.0

First public release. Covers R13 (AC1014) through R2018 (AC1032):

### Added

**Core pipeline (Phases A–D)**
- Version identification for AC1014, AC1015, AC1018, AC1021, AC1024, AC1027, AC1032.
- R13–R15 simple file header + R2004+ XOR-encrypted header.
- Bit-cursor primitives: B, BB, 3B, BS, BL, BLL, BD, MC, MS, RC, RS, RL, RD, H, TV.
- CRC-8 (`crc::crc8`) + CRC-32 (`crc::crc32`) verification.
- LZ77 decompressor (ACadSharp-verified +1 offset dialect).
- Section Page Map + Section Info parsing.
- `DwgFile::read_section(name)` — decompressed section retrieval by name.

**Metadata (Phase D-1)**
- `SummaryInfo`, `AppInfo` (with R18 ANSI / R21+ UTF-16 auto-detect).
- `Preview` (including modern AutoCAD's undocumented PNG code-6 fallback).
- `FileDepList` (font, XRef, image, text dependencies).

**Object stream (Phase D-2 through D-4)**
- `ObjectType` enum covering 80+ built-in types.
- `ObjectWalker` for first-object reads and handle-driven full walks.
- `HandleMap` parser (big-endian sections + signed MC deltas).
- `ClassMap` parser for custom class codes (≥ 500).
- `DwgFile::all_objects()` — complete file object iteration.

**Entity decoders (Phase E)**
- Common entity preamble (§19.4.1).
- Primitives: LINE, POINT, CIRCLE, ARC, ELLIPSE, RAY, XLINE, SOLID, 3DFACE, TRACE, SPLINE.
- Text: TEXT, MTEXT, ATTRIB, ATTDEF.
- Block / polyline: INSERT, BLOCK, ENDBLK, VERTEX, POLYLINE, LWPOLYLINE.
- Advanced: DIMENSION (7 subtypes), LEADER, IMAGE, HATCH (header), MLEADER (header), VIEWPORT.

**Symbol tables (Phase F-1)**
- Shared entry header + name/xref/flags.
- LAYER, LTYPE, STYLE, VIEW, UCS, VPORT, APPID, DIMSTYLE, BLOCK_RECORD.

**Control objects (Phase F-2)**
- DICTIONARY (with `.get()` by name), XRECORD, `*_CONTROL` shared shape.

**Error correction (Phase G)**
- Reed-Solomon(255,239) over GF(256) with primitive polynomial 0x11D.
- Berlekamp-Massey + Chien search + Forney via Gaussian elimination over GF(256).
- Defensive-only: `verify()` is called on CRC failure.

**Write primitives (Phase H-1)**
- Bit-writer: inverse of every BitCursor primitive, round-trip tested.
- LZ77 literal-only encoder (correctness-first; matcher pass deferred).

**CLI tools (Phase I)**
- `dwg-dump` — hierarchical human-readable dump with per-section flags.
- `dwg-convert` — section extract + whole-file verify.
- `dwg-info` — compact one-line file summary (pre-existing).
- `dwg-corpus` — batch corpus scanner (pre-existing).

**Verification**
- 144 unit tests covering every module.
- 9 property-based round-trip tests (proptest): each primitive × 256 random cases.
- 5 cross-corpus integration tests over 19 sample files.
- 22 per-file sample-specific assertions.
- 1 doctest on the top-level module.

### Deferred

- **Phase H-2** — Full LZ77 back-reference encoder, section writer with CRC/Sec_Mask,
  `DwgFile::to_bytes()`.
- **R2007** — Full Sec_Mask 2-layer bitstream for R2007 specifically (R2010+ and
  the R2004 base both work today).
- **Per-entity advanced fields** — The full HATCH boundary path tree, the full
  MLEADER leader-line list, and the full 75-field DIMSTYLE record.
- **AcDb:Header variables** — targeted INSUNITS/DIMSCALE/CLAYER accessors;
  the raw bit-stream is captured, but individual-variable extraction is
  incremental work.

### Legal posture

Clean-room — no Autodesk SDK, no ODA SDK, no LibreDWG (GPL-3) source consulted.
Implemented against the ODA's freely-published *Open Design Specification for
.dwg files* (v5.4.1); cross-verified against ACadSharp (MIT) only for LZ77
offset-encoding spec typos.

### Not yet

Binary crate isn't published to crates.io yet. A dry-run (`cargo publish --dry-run`)
is the next release gate.
