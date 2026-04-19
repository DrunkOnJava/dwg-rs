# dwg-rs

**Open reader for Autodesk DWG files (R13 → R2018) — Apache-2.0, no Autodesk or ODA SDK required.**

Clean-room Rust 2024 implementation against the Open Design Alliance's freely-published *Open Design Specification for .dwg files* (v5.4.1). Cross-verified against ACadSharp (MIT) for LZ77 off-by-one spec typos. Zero GPL-3 taint (no LibreDWG source was consulted).

## Current state — read coverage by phase

Four phases ship today across 7 commits, 5,500+ LOC, and 83/83 tests:

| Phase | Scope | Status |
|-------|-------|--------|
| **A — Identification** | Version magic, header parsing, bit-cursor primitives, CRC-8/CRC-32, XOR cipher | ✓ Shipped |
| **B — Section enumeration** | LZ77 decompressor, Section Page Map (§4.4), Section Info (§4.5) | ✓ Shipped |
| **C — Section extraction** | Data page header Sec_Mask decrypt (§4.6), `read_section(name)` API, `--extract` CLI | ✓ Shipped |
| **D-1 — Metadata parsers** | `SummaryInfo`, `AppInfo` (auto-detect R18 ANSI vs R21+ Unicode), `Preview` (with PNG-code-6 support), `FileDepList` | ✓ Shipped |
| **D-2 — Object type + walker** | `ObjectType` enum (80+ types), `ObjectWalker` with R2010+ type-code dispatch, per-object handle extraction | ✓ Shipped (first-object mode) |
| **D-3 — Handle + class maps** | `AcDb:Handles` → `HandleMap` for random-access object seek, `AcDb:Classes` → `ClassMap` for dynamic type dispatch | ✓ Shipped |

## Verified against `sample_AC1032.dwg` (AutoCAD 2018, 1 MB)

```text
version:         AC1032 (2018)           [Phase A]
stored CRC-32:   0x8f2b576f  CRC-32 check: PASS   [Phase A]
sections (13):                                     [Phase B]
  AcDb:Header              870 B           (LZ77)
  AcDb:AuxHeader           129 B
  AcDb:Classes            6296 B
  AcDb:Handles            2642 B
  AcDb:Template              6 B
  AcDb:ObjFreeSpace         89 B
  AcDb:AcDbObjects     1192851 B  (41 LZ77 pages reassembled)
  AcDb:RevHistory           16 B
  AcDb:SummaryInfo          76 B
  AcDb:Preview            1548 B  (PNG thumbnail, modern code-6)
  AcDb:AppInfo             718 B
  AcDb:AppInfoHistory     1478 B
  AcDb:AcDsPrototype_1b  46208 B
Preview BMP/PNG carve:    Valid                   [Phase D-1]
SummaryInfo author:       "alber..."              [Phase D-1]
AppInfo name:             "AppInfoDataList"       [Phase D-1]
AppInfo version:          "24.0.118.0.0"          [Phase D-1]
BLOCK_CONTROL (h=0x01):   type_code=0x30          [Phase D-2]
Handle map:               non-empty, h=1 present  [Phase D-3]
```

## API walkthrough

```rust
use dwg::{DwgFile, SectionKind};

let f = DwgFile::open("drawing.dwg")?;

// Phase A: identification
println!("version: {}", f.version());
println!("codepage: {}", f.r2004_header().map(|h| h.common.codepage).unwrap_or(0));

// Phase B: section enumeration
for section in f.sections() {
    println!("{} ({} bytes at 0x{:x}, compressed={})",
        section.name, section.size, section.offset, section.compressed);
}

// Phase C: extract raw decompressed bytes of any named section
if let Some(Ok(preview_bytes)) = f.read_section("AcDb:Preview") {
    std::fs::write("preview.bin", &preview_bytes)?;
}

// Phase D-1: structured metadata
if let Some(Ok(si)) = f.summary_info() {
    println!("title: {:?}", si.title);
    println!("author: {:?}", si.author);
    for (k, v) in &si.properties {
        println!("  custom: {k:?} = {v:?}");
    }
}
if let Some(Ok(preview)) = f.preview() {
    if let Some(bmp) = preview.bmp {
        std::fs::write("thumbnail.png", &bmp)?; // may be PNG despite field name
    }
}

// Phase D-3: handle map + class map
if let Some(Ok(hmap)) = f.handle_map() {
    println!("{} objects in handle table", hmap.entries.len());
    if let Some(offset) = hmap.offset_of(0x42) {
        println!("handle 0x42 is at offset {offset}");
    }
}
# Ok::<(), dwg::Error>(())
```

## CLIs

```bash
cargo build --release

# Human-readable report
./target/release/dwg-info path/to/file.dwg

# JSON output for piping
./target/release/dwg-info path/to/file.dwg --json

# Validate the R2004+ decrypted-header CRC-32
./target/release/dwg-info path/to/file.dwg --crc

# Extract any named section's decompressed bytes to a file
./target/release/dwg-info path/to/file.dwg --extract AcDb:Preview --out preview.bin
./target/release/dwg-info path/to/file.dwg --extract AcDb:AcDbObjects --out objects.bin

# Sweep a directory
./target/release/dwg-corpus path/to/samples --strict

# Probe metadata sections on one file (structured parse)
./target/release/examples/probe_metadata path/to/file.dwg

# Walk the object stream (first-pass — shows BLOCK_CONTROL root)
./target/release/examples/probe_objects path/to/file.dwg
```

## Module architecture

```text
src/
├── error.rs          — thiserror Error enum, Result alias
├── version.rs        — AC10xx → Version mapping, family predicates
├── bitcursor.rs      — spec §2 bit primitives (B/BB/3B/BS/BL/BLL/BD/MC/MS/H + raw RC/RS/RL/RD)
├── cipher.rs         — R2004+ 108-byte XOR magic + Sec_Mask formula
├── crc.rs            — CRC-8 (256×u16 table) + CRC-32 (IEEE) + Adler-style page checksum
├── lz77.rs           — spec §4.7 LZ77 dialect (ACadSharp-verified offset formulas)
├── cipher.rs         — XOR cipher + Sec_Mask
├── header.rs         — R13-R15 header + R2004+ encrypted header parse
├── section.rs        — Section struct + SectionKind enum (names ↔ kinds)
├── section_map.rs    — Page map (§4.4) + section info (§4.5) + data-page reader (§4.6)
├── object_type.rs    — 80+ fixed object type codes + Custom(N) + is_entity/is_control
├── metadata.rs       — SummaryInfo / AppInfo / Preview / FileDepList byte-oriented parsers
├── object.rs         — AcDb:AcDbObjects stream walker (first-object mode)
├── handle_map.rs     — AcDb:Handles offset table parser (big-endian sections + signed MC deltas)
├── classes.rs        — AcDb:Classes custom class table parser
├── reader.rs         — DwgFile top-level API + all section convenience accessors
├── lib.rs            — public re-exports + module hub
└── bin/
    ├── dwg_info.rs   — human/JSON metadata + --extract CLI
    └── dwg_corpus.rs — directory sweep tool

tests/
└── samples.rs        — 22 integration tests: per-version open, named section enumeration,
                        extract round-trip, preview bytes, header/preview/handle-map parse

examples/
├── probe_metadata.rs — structured metadata dump
├── probe_objects.rs  — first-object type + handle dump
└── debug_section_map.rs — development harness for the page/section map parser
```

## What is explicitly deferred

The DWG format's full-write round-trip, every entity type's field decoder,
and the R2007-specific layout are large follow-on bodies of work. They
are tracked as future phases rather than dropped from scope:

- **Per-entity field decoding** (Phase E): LINE, CIRCLE, ARC, POINT, LWPOLYLINE, INSERT, TEXT, MTEXT, 3DFACE, SOLID, ELLIPSE, SPLINE, HATCH, POLYLINE/VERTEX, DIMENSION variants, VIEWPORT, LAYER/LTYPE/STYLE/VIEW/UCS/VPORT/APPID/DIMSTYLE table entries, DICTIONARY, XRECORD. The object walker already extracts each record's type code, handle, and raw bytes; what's missing is the per-class bit-layout decoder (spec §20.4.* entries — ~100 pages of field tables). Today's pipeline delivers the raw bytes; Phase E adds typed field extraction.
- **Object-stream iteration via handle map** (Phase E): the sequential walker reads one object; the handle map (D-3) has the offsets for the rest, but isn't wired into `DwgFile::objects()` yet.
- **R2007 (AC1021) full layout** (Phase F): spec §5 describes a 33-page delta where R2007 uses a distinct file-header structure. Today the reader identifies R2007 files but routes them to a stub (Phase A behavior).
- **Reed-Solomon(255,239) FEC verification** (Phase G): §4.1 repair-side mode. Today we trust CRC-8 intra-chunk and CRC-32 at the header block level. Valid files read fine without RS; repair tools would need it.
- **Write support** (Phase H): bit-cursor writer, LZ77 encoder, section writer, DwgFile::to_bytes(). The reader's bit-cursor primitives need a mirror-image writer module.

The remaining entity-field decoder work is *bounded* — each entity class is 20-50 lines of Rust and pegged against a specific spec §20.4.* table. It's implementation volume, not unknown-unknowns.

## Legal posture

DWG is a trademark of Autodesk, Inc. This crate is a clean-room implementation under the interoperability exception of 17 U.S.C. § 1201(f) and the 2006 *Autodesk v. ODA* settlement.

- **No ODA SDK dependency** — SDK licensing explicitly avoided.
- **No LibreDWG source consulted** — GPL-3 would taint Apache-2.0 downstream.
- **ACadSharp (MIT) referenced** for LZ77 offset formulas where the ODA spec has off-by-one typos. Reading MIT-licensed code to resolve a spec ambiguity is compatible with clean-room discipline (no code copied).
- **ODA Open Design Specification v5.4.1 PDF** is the primary reference. It is published separately from ODA's SDK license and is freely redistributable.

Apache-2.0 — see `LICENSE`.

## Sample corpus

19 DWG files under `../../samples/` spanning R14 (1997) → AC1032 (2018+):
- `arc_*.dwg`, `circle_*.dwg`, `line_*.dwg` at each R14/2000/2004/2007/2010/2013 version
- `sample_AC1032.dwg` — 1 MB AutoCAD 2018 fixture

Integration tests run against all 19 files; R2007 (AC1021) tests assert the stub path (version identified, full parse deferred).

## Running the tests

```bash
cargo test --release
# 60 unit tests + 22 integration tests + 1 doc test = 83 passing
```
