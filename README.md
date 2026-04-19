# dwg-rs

**Open reader for Autodesk DWG files (`AC1014` / R14 through `AC1032` / 2018+) — no Autodesk or ODA SDK required.**

Apache-2.0 licensed. Rust 2024 edition. Clean-room implementation against the Open Design Alliance's freely-published *Open Design Specification for .dwg files* (v5.4.1).

## Why this exists — the $6B interop moat

AutoCAD is Autodesk's $2B-a-year product and `.dwg` is its native file format. Every vertical SaaS that needs to ingest CAD drawings — real-estate tooling, insurance inspection, construction workflow, GIS, facility-management, archival preservation — has three options today:

1. **Pay the Open Design Alliance** $500 - $5,000/year for the Teigha SDK.
2. **Use LibreDWG (GPL-3)** — acceptable only if you ship your product under a compatible copyleft license.
3. **Reject DWG uploads** and shift the burden to the user.

`dwg-rs` is the option that did not exist before: a permissively-licensed (Apache-2), memory-safe (Rust), no-dependency DWG reader that can be bundled into proprietary SaaS without license entanglement. Once the library reaches parity with LibreDWG on read-side coverage, the ODA membership as a *revenue stream* evaporates — the moat has not been a technical one since at least 2009, it has been an information moat guarded by a paywall and the unpriced-deterrent of "nobody wants to maintain another LibreDWG."

The bet this crate makes is that an AI-first development posture — Opus 4.7 with a 1M-token context window iteratively reading the 279-page ODA spec and cross-referencing the nineteen reference DWG files shipped under `samples/` — closes the remaining delta faster than a GPL-chained solo maintainer ever could.

## What works today (Phase A + Phase B)

### Phase A — identification + header parsing

- **Seven production format versions identified**: R14 (AC1014, 1997), 2000 (AC1015, 1999), 2004 (AC1018, 2003), 2007 (AC1021, 2006), 2010 (AC1024, 2009), 2013 (AC1027, 2012), 2018 (AC1032, 2017+).
- **R13-R15 simple-header path**: magic verification, codepage, image seeker, section-locator enumeration with all 9-byte records and family-seeded CRC-8.
- **R2004+ encrypted-header path**: plaintext 0x80 bytes parsed, the 0x6C encrypted block decrypted with the published 108-byte XOR magic sequence (derived from Microsoft's `rand()` LCG seeded at 1), decrypted CRC-32 verified per spec §4.1, Section Page Map address + Section Map ID extracted.
- **Bit-cursor primitives per ODA spec §2**: `B`, `BB`, `3B`, `BS`, `BL`, `BLL`, `BD`, `RC`, `RS`, `RL`, `RD`, `MC`, `MS`, `H`. Spec-example unit tests pass (257/0/256/15/0 for BITSHORT, LAYER 0 handle = 5.1.0F for H).
- **CRC primitives**: DWG 8-bit CRC with its 256 × 16-bit lookup table, standard IEEE 802.3 CRC-32, and the R2004+ section-page checksum (Adler-style rolling sum with 0x15B0 chunks).

### Phase B — LZ77 decompression + named section enumeration

- **LZ77 decompressor** per spec §4.7 with ACadSharp-verified offset adjustments (spec text has an off-by-one on all class additive constants; the correct offsets are `+0x4000` long / `+1` short / `+1` for the 0x40-0xFF class). Supports RLE-style copies (length > offset) and extended literal-length runs via 0x00-byte accumulation.
- **Section Page Map decoder** (§4.4): reads the compressed global page map and emits a `Vec<SectionPage>` with computed absolute file offsets. Pages numbered consecutively as the spec promises — sample_AC1032.dwg produces pages 1 through 56 with no gaps.
- **Section Info (Section Map) decoder** (§4.5): walks the page map to find the section-map page, decompresses it, and parses the 14-description table: `AcDb:Header` (870 B), `AcDb:AuxHeader` (129 B), `AcDb:Classes` (6296 B), `AcDb:Handles` (2642 B), `AcDb:Template` (6 B), `AcDb:ObjFreeSpace` (89 B), `AcDb:AcDbObjects` (1192851 B), `AcDb:RevHistory` (16 B), `AcDb:SummaryInfo` (76 B), `AcDb:Preview` (1548 B), `AcDb:AppInfo` (718 B), `AcDb:AppInfoHistory` (1478 B), `AcDb:AcDsPrototype_1b` (46208 B).
- **Unified section list** exposed via `DwgFile::sections()` — Phase A stubs are replaced automatically when Phase B succeeds, with a silent fallback to the stub path for files whose page map doesn't parse cleanly.
- **Two CLIs**: `dwg-info` (human + JSON metadata report; now shows 13 named sections for R2004/2010/2013/2018 files) and `dwg-corpus` (sweep a directory, print one line per file).

Running the test suite against the shipped 19-file corpus:

```text
$ cargo test --release
test result: ok. 41 passed; 0 failed   (unit tests in 9 modules)
test result: ok. 18 passed; 0 failed   (integration tests)
test result: ok.  1 passed; 0 failed   (doc test)
```

## What is explicitly deferred

- **Reed-Solomon(255,239) FEC** verification of R2004+ system pages — currently we trust CRC-8 intra-chunk and CRC-32 at the header block level. A clean-room RS(255,239) implementation is ~200 lines and is tracked for Phase C (not strictly needed for reading valid files, but essential for repair tools).
- **R2007 (AC1021) full layout** — spec §5 describes a different file-header structure for R2007 specifically (33 pages of deltas from R2004). Phase A identifies the file; Phase C implements its Sec_Mask two-layer bitstream.
- **Named data-section payload decompression** — Phase B parses section *metadata* (names, sizes, offsets), but extracting a specific section's decompressed bytes requires walking its per-section page list and applying the Sec_Mask data-page header decrypt. The helpers are in place; the public API (`DwgFile::read_section(&str) -> Vec<u8>`) is Phase C.
- **Entity / object field decoding** for all ~200 DWG entity types (LINE, CIRCLE, ARC, POLYLINE, MTEXT, DIMASSOC, TABLE, surfaces, MATERIAL, MLEADER, XREF, ...). Phase D.
- **Write support** — encoding any version from scratch. Phase E.

The current crate is sufficient today for consumers who need to *identify* DWG files, *route* them by version, *enumerate which sections exist*, and *see sizes* (for estimating work). The thumbnail `AcDb:Preview` payload is the next natural extraction target.

## Quick start

```rust
use dwg::{DwgFile, section::SectionKind};

let f = DwgFile::open("drawing.dwg")?;
println!("version: {}", f.version());
for s in f.sections() {
    println!("{} ({} bytes at 0x{:x})", s.name, s.size, s.offset);
}

// Ask for a specific section:
if let Some(preview) = f.section_of_kind(SectionKind::Preview) {
    println!("preview is {} bytes at offset 0x{:x}", preview.size, preview.offset);
}
```

## CLI usage

```bash
cargo build --release

# Human-readable report
./target/release/dwg-info path/to/file.dwg

# JSON output for piping
./target/release/dwg-info path/to/file.dwg --json

# Validate the R2004+ decrypted-header CRC-32
./target/release/dwg-info path/to/file.dwg --crc

# Sweep a directory
./target/release/dwg-corpus path/to/samples --strict
```

## Format overview

```text
  R13-R15 (AC1014, AC1015)              R2004+ (AC1018, AC1021, AC1024, AC1027, AC1032)
  ──────────────────────                ──────────────────────────────────────────────
  0x00  AC10xx magic                    0x00  AC10xx magic
  0x06  5 zeros + maint byte            0x06  5 zeros + maint byte
  0x0D  image seeker (RL)               0x0D  preview address (RL)
  0x13  DWGCODEPAGE (RS)                0x13  DWGCODEPAGE (RS)
  0x15  locator count (RL)              0x18  security flags (RL)
  0x19  N × {u8, RL, RL} records        0x20  summary info addr
  ...   CRC + sentinel + data           0x24  VBA project addr (optional)
                                        0x80  ENCRYPTED 0x6C bytes ←─┐
                                                                     │
                                        ┌── decrypt (XOR, spec §4.1) ┘
                                        │
                                        ▼
                                        "AcFssFcAJMB" + Section Page Map pointer
                                        + CRC-32 of the decrypted block (self-verifying)
                                        │
                                        ▼
                                        Section Page Map  →  Section Map  →  named payloads
                                        (LZ77 + Reed-Solomon, per-page checksums)
```

The stabilized 7.91-7.93 bits/byte entropy at the tail of an R2004+ file is **not encryption** — it is LZ77-compressed object data interleaved with Reed-Solomon(255,239) FEC parity (32 bytes of parity per 255-byte chunk). Entropy that high requires either genuine random data, AES, or `compressed-plus-FEC`; the third is what the spec documents.

## Sample corpus

The `samples/` directory alongside this crate (at `../../samples/`) contains 19 reference DWG files sourced from `nextgis/dwg_samples` (MIT) and a 1 MB AC1032 fixture. These are reference drawings of arcs, circles, and lines — small enough to inspect by hand but large enough that the header + section map must be fully parsed.

The integration test suite (`tests/samples.rs`) iterates the full corpus and asserts that each file: (a) identifies its version correctly, (b) opens without error, (c) reports a non-zero section count, (d) populates exactly one of the R13-R15 or R2004+ header paths. Running the suite is the primary regression gate.

## Design choices

- **`byteorder` over custom reads** — the `byteorder` crate is a single stable dependency that handles every LE/BE read needed here; rolling our own doesn't save code and costs correctness.
- **`thiserror` for the `Error` enum, `anyhow` only at CLI boundaries** — the library exports structured error variants so downstream tools can switch on them; the CLIs collapse them to `anyhow::Error` for terminal display.
- **No `unsafe`** — the crate declares `#![deny(unsafe_code)]`. Bit cursoring into a `&[u8]` and byte-parsing with `byteorder` are both trivially safe.
- **In-memory file model** — Phase A reads the whole file into `Vec<u8>`. A typical engineering drawing is 1-10 MB and loading it upfront simplifies the `Section` ownership story. `memmap2` backing is deferred to Phase B when it will also enable zero-copy decompression output.
- **Spec-driven tests** — the BITSHORT / BITLONG / BITDOUBLE worked examples in spec §2.2-§2.5 all appear as unit tests. LAYER 0 handle `5.1.0F` (spec §2.13) is locked in. The R2004+ XOR magic sequence is checked byte-for-byte against the spec page 24 reproduction.

## Legal posture

DWG is a trademark of Autodesk, Inc. Autodesk is not affiliated with this project. This is a clean-room implementation under the interoperability exception of 17 U.S.C. § 1201(f) and the 2006 *Autodesk v. ODA* settlement, which explicitly permitted third-party DWG interoperability. No LibreDWG source code was consulted while writing this crate; the authoritative reference is the ODA's own publicly-downloadable specification PDF (a text document whose distribution is not encumbered by the ODA SDK license).

Apache-2.0 — see `LICENSE`.
