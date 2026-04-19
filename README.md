# dwg-rs

**Open reader for Autodesk DWG files (R13 → R2018/AC1032) — Apache-2.0, no Autodesk or ODA SDK required.**

Clean-room Rust 2024 implementation against the Open Design Alliance's freely-published *Open Design Specification for .dwg files* (v5.4.1). Cross-verified against ACadSharp (MIT) for LZ77 off-by-one spec typos. Zero GPL-3 taint — no LibreDWG source was consulted at any point.

## Status

**Phases A through I ship today.** 181 tests (144 lib + 9 property-based + 5 cross-corpus + 22 sample + 1 doctest) all green against the 19-file DWG corpus.

| Phase | Scope | State |
|-------|-------|-------|
| **A — Identification** | Version magic, header parsing, bit-cursor primitives, CRC-8/CRC-32, XOR cipher | ✓ |
| **B — Section enumeration** | LZ77 decompressor, Section Page Map (§4.4), Section Info (§4.5) | ✓ |
| **C — Section extraction** | Data-page-header Sec_Mask decrypt (§4.6), `read_section(name)` API | ✓ |
| **D-1 — Metadata** | `SummaryInfo`, `AppInfo` (auto-detects R18 ANSI vs R21+ Unicode), `Preview` (PNG code-6 carve), `FileDepList` | ✓ |
| **D-2 — Object walker** | `ObjectType` (80+ types), R2010+ type-code dispatch, per-object handle extraction | ✓ |
| **D-3 — Cross-ref tables** | `AcDb:Handles` → `HandleMap`, `AcDb:Classes` → `ClassMap` | ✓ |
| **D-4 — Handle-driven iteration** | `DwgFile::all_objects()` — full file walk via handle map | ✓ |
| **E-1 — Common entity** | Shared preamble (§19.4.1) — XDATA, graphics preview, entity-mode, reactors, plotstyle, layer flags, lineweight | ✓ |
| **E-2 — Primitive entities** | LINE, POINT, CIRCLE, ARC, ELLIPSE, RAY, XLINE, SOLID, 3DFACE, TRACE, SPLINE | ✓ |
| **E-3 — Text entities** | TEXT, MTEXT, ATTRIB, ATTDEF | ✓ |
| **E-4 — Block / polyline** | INSERT, BLOCK, ENDBLK, VERTEX, POLYLINE, LWPOLYLINE | ✓ |
| **E-5 — Advanced** | DIMENSION family (7 subtypes), LEADER, IMAGE, HATCH (header), MLEADER (header), VIEWPORT | ✓ |
| **F-1 — Symbol tables** | LAYER, LTYPE, STYLE, VIEW, UCS, VPORT, APPID, DIMSTYLE, BLOCK_RECORD | ✓ |
| **F-2 — Control objects** | DICTIONARY, XRECORD, `*_CONTROL` | ✓ |
| **G — Error recovery** | Reed-Solomon(255,239) GF(256) decoder with Berlekamp-Massey + Chien + Forney | ✓ |
| **H-1 — Write primitives** | Bit-writer (B/BB/3B/BS/BL/BLL/BD/RC/RS/RL/RD/MC/MS/H), LZ77 literal-only encoder | ✓ |
| **I-1 — CLI tools** | `dwg-dump` (hierarchical dump), `dwg-convert` (section extract + verify) | ✓ |
| **H-2 — Section writer** | Back-ref LZ77 encoder, CRC/Sec_Mask re-emit, `DwgFile::to_bytes()` end-to-end | — deferred |
| **R2007 layout** | Full Sec_Mask 2-layer bit-stream for R2007 (R2010+ works; only R2007 itself is stubbed) | — deferred |

## Architecture

```
┌────────── Top-level ──────────┐
│  DwgFile::open(path)          │  — single entry point
│  .version() .sections()        │
│  .summary_info() .app_info()   │
│  .preview() .file_dep_list()   │
│  .handle_map() .class_map()    │
│  .header_vars() .all_objects() │
└──────────────┬────────────────┘
               │
  ┌────────────┼──────────────────────┐
  │            │                      │
┌─▼──────┐ ┌───▼────────┐ ┌──────────▼───────┐
│ reader │ │ section_   │ │ entities/* +     │
│        │ │  map       │ │ tables/* +       │
│        │ │            │ │ objects/*        │
└────────┘ └────────────┘ └──────────────────┘
  │ │                         │
  │ └──lz77 + cipher + crc    │ (bit-cursor + bit-writer)
  │                           │
  └──reed_solomon (defensive) └── common_entity preamble
```

## Use from Rust

```rust
use dwg::{DwgFile, entities::line};

let f = DwgFile::open("drawing.dwg")?;
println!("version: {}", f.version());

// Walk every object by handle.
for raw in f.all_objects().unwrap()? {
    if raw.is_entity() {
        println!("  handle 0x{:X} → {:?}", raw.handle.value, raw.kind);
    }
}
# Ok::<(), dwg::Error>(())
```

## CLI

```sh
cargo install --path . --bin dwg-dump --bin dwg-convert --bin dwg-info

# Hierarchical dump — version, sections, classes, metadata,
# handles, object histogram.
dwg-dump drawing.dwg

# Extract a single decompressed section to disk.
dwg-convert --extract AcDb:Preview -o preview.bmp drawing.dwg

# Open + decompress every section, report per-section pass/fail.
dwg-convert --verify drawing.dwg
```

## Verification

```text
$ cargo test --release
running 144 tests                                 (unit)
test result: ok. 144 passed; 0 failed

running 5 tests                                   (cross-corpus integration)
test result: ok. 5 passed; 0 failed

running 9 tests                                   (proptest round-trips)
test result: ok. 9 passed; 0 failed

running 22 tests                                  (per-sample assertions)
test result: ok. 22 passed; 0 failed

running 1 test                                    (doctest)
test result: ok. 1 passed; 0 failed
```

## Legal posture

DWG is a trademark of Autodesk, Inc. This crate is a clean-room implementation under the interoperability exception of 17 U.S.C. § 1201(f) and the 2006 *Autodesk v. ODA* settlement, which explicitly permits third-party DWG interop. The authoritative reference is the ODA's *Open Design Specification for .dwg files* (v5.4.1), a freely redistributable document separate from ODA's SDK license.

## Why this exists

For ~28 years the DWG format has been a monopoly. Autodesk never documented it; the Open Design Alliance's SDK requires a license fee and introduces GPL-adjacent constraints; LibreDWG is GPL-3, which is incompatible with most commercial code. `dwg-rs` is the first open-source, Apache-2, clean-room reader — fully parseable DWG interop without paying tolls or pulling GPL-3 code into your dependency graph.

## License

Apache-2.0. See `LICENSE`.
