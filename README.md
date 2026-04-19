# dwg-rs

> **Clean-room, Apache-2.0 Rust reader for Autodesk DWG files (R13 → R2018 / AC1032).**
> No Autodesk SDK. No ODA SDK. No GPL-3 dependency. The first open-source DWG library with no strings attached.

[![CI](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/dwg.svg)](https://crates.io/crates/dwg)
[![Documentation](https://docs.rs/dwg/badge.svg)](https://docs.rs/dwg)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Rust MSRV](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-193%20passing-brightgreen.svg)](#verification)

---

## Table of contents

- [Why this exists](#why-this-exists)
- [Status](#status)
- [Install](#install)
- [Quick start — library](#quick-start--library)
- [Quick start — CLI](#quick-start--cli)
- [Feature coverage](#feature-coverage)
- [Architecture](#architecture)
- [Verification](#verification)
- [Safety](#safety)
- [MSRV policy](#msrv-policy)
- [Related projects](#related-projects)
- [FAQ](#faq)
- [Contributing](#contributing)
- [Legal posture](#legal-posture)
- [License](#license)

---

## Why this exists

For the past ~28 years, the DWG format has been a moat. Autodesk never published a spec. The Open Design Alliance's SDK requires a paid license and introduces GPL-adjacent constraints. LibreDWG is GPL-3, which is incompatible with most commercial code.

`dwg-rs` is the first open-source, Apache-2.0, clean-room DWG reader — a permissively-licensed foundation for CAD interoperability tooling without paying tolls or pulling GPL-3 into your dependency graph.

This crate was implemented exclusively against the Open Design Alliance's *Open Design Specification for .dwg files* (v5.4.1), a freely-redistributable document separate from ODA's SDK license. No Autodesk source, no ODA SDK source, and no LibreDWG source was consulted.

## Status

Pre-1.0, but feature-complete for read operations across every shipping DWG version from 1997 (R14) to 2024+. Partial write-path scaffolding is in place.

| Area | State |
|------|-------|
| Read — R14 / R2000 / R2004 / R2010 / R2013 / R2018 | ✓ Shipping |
| Read — R2007 Sec_Mask two-layer obfuscation | ⟳ Layer 1 done, Layer 2 scaffolded |
| Entity decoders (27 types) | ✓ Shipping |
| Symbol-table entry decoders (9 tables) | ✓ Shipping |
| Control objects (DICTIONARY, XRECORD, `*_CONTROL`) | ✓ Shipping |
| Reed-Solomon(255,239) FEC recovery | ✓ Shipping |
| Write — bit-writer + LZ77 encoder + section framer | ✓ Shipping (literal-only LZ77) |
| Write — full `DwgFile::to_bytes()` pipeline | ⟳ Stages 2–5 scaffolded |

See the [CHANGELOG](CHANGELOG.md) for release notes and the [ARCHITECTURE](ARCHITECTURE.md) document for design deep-dive.

## Install

From source:

```bash
git clone https://github.com/DrunkOnJava/dwg-rs
cd dwg-rs
cargo build --release
```

From crates.io (after 0.1.0 publish):

```bash
cargo add dwg
```

As a CLI:

```bash
cargo install --git https://github.com/DrunkOnJava/dwg-rs
# installs: dwg-info, dwg-corpus, dwg-dump, dwg-convert
```

## Quick start — library

```rust
use dwg::DwgFile;

fn main() -> dwg::Result<()> {
    let file = DwgFile::open("drawing.dwg")?;

    println!("version: {}", file.version());
    println!("sections: {}", file.sections().len());

    // Walk every drawing object by handle.
    if let Some(Ok(objects)) = file.all_objects() {
        for obj in objects.iter().filter(|o| o.is_entity()) {
            println!("  handle 0x{:X} → {:?}", obj.handle.value, obj.kind);
        }
    }

    // Read structured metadata.
    if let Some(Ok(summary)) = file.summary_info() {
        println!("title:  {}", summary.title);
        println!("author: {}", summary.author);
    }

    Ok(())
}
```

More examples live in [`examples/`](./examples/):

- [`basic_open.rs`](./examples/basic_open.rs) — open + print version + section list
- [`walk_entities.rs`](./examples/walk_entities.rs) — histogram of entity types
- [`extract_preview.rs`](./examples/extract_preview.rs) — pull the embedded thumbnail to disk
- [`dump_metadata.rs`](./examples/dump_metadata.rs) — print summary info, app info, dependencies

Run any of them with:

```bash
cargo run --release --example basic_open -- path/to/drawing.dwg
```

## Quick start — CLI

Four binaries ship with the crate:

```bash
# One-line summary of a file's version, size, and sections
dwg-info drawing.dwg

# Sweep a directory of .dwg files and report which ones open cleanly
dwg-corpus /path/to/corpus/

# Full hierarchical dump — sections, classes, metadata, handles, objects
dwg-dump drawing.dwg

# Extract a single section's decompressed bytes to disk
dwg-convert --extract AcDb:Preview -o preview.bmp drawing.dwg

# Verify every section in a file decompresses cleanly
dwg-convert --verify drawing.dwg
```

## Feature coverage

### Entity decoders (spec §19.4)

| Family | Types covered |
|--------|----------------|
| Lines & curves | LINE, POINT, CIRCLE, ARC, ELLIPSE, RAY, XLINE, SPLINE (fit + control), LWPOLYLINE, POLYLINE + VERTEX |
| Faces & solids | SOLID, 3DFACE, TRACE |
| Text | TEXT, MTEXT, ATTRIB, ATTDEF |
| Blocks | INSERT, BLOCK, ENDBLK |
| Dimensions | DIMENSION (Ordinate, Linear, Aligned, Angular-3Pt, Angular-2Line, Radius, Diameter) |
| Advanced | LEADER, MLEADER, IMAGE, HATCH, VIEWPORT |

### Symbol tables (spec §19.5)

LAYER, LTYPE, STYLE, VIEW, UCS, VPORT, APPID, DIMSTYLE, BLOCK_RECORD.

### Control objects

DICTIONARY (with `.get()` lookup), XRECORD, and the shared `*_CONTROL` shape for LAYER_CONTROL, STYLE_CONTROL, LTYPE_CONTROL, BLOCK_CONTROL, VIEW_CONTROL, UCS_CONTROL, VPORT_CONTROL, APPID_CONTROL, DIMSTYLE_CONTROL.

### Metadata sections

AcDb:SummaryInfo (title / author / keywords / comments), AcDb:AppInfo (writing application + version, R18 ANSI + R21+ UTF-16 auto-detected), AcDb:Preview (BMP, WMF, and modern AutoCAD's undocumented PNG code-6), AcDb:FileDepList (fonts, XRefs, images, external references).

## Architecture

```
 ┌────────────── DwgFile::open(path) ──────────────┐
 │  .version()  .sections()  .summary_info()       │  <- public API
 │  .handle_map()  .class_map()  .all_objects()    │
 └────────────┬────────────────────────────────────┘
              │
  ┌───────────┼─────────────────────────────────┐
  │           │                                 │
  ▼           ▼                                 ▼
┌────────┐ ┌──────────────┐         ┌────────────────────┐
│ reader │ │ section_map  │         │  entities/*        │
│ header │ │ section_     │         │  tables/*          │
│ cipher │ │  _writer     │         │  objects/*         │
│ crc    │ └──────┬───────┘         └────────┬───────────┘
└────┬───┘        │                          │
     │         ┌──▼───┐              ┌───────▼────────┐
     │         │ lz77 │              │ common_entity  │
     │         │  _en │              │ bitcursor /    │
     │         │ code │              │ bitwriter      │
     │         └──────┘              └────────────────┘
     │
     ▼
  reed_solomon (defensive recovery)
  r2007 (Sec_Mask layer 1 + 2 scaffold)
```

See [ARCHITECTURE.md](./ARCHITECTURE.md) for a full technical deep dive — format primer, module responsibilities, the four-phase read pipeline, Sec_Mask explanation, and the LZ77 spec-errata corrections.

## Verification

```text
$ cargo test --release
running 156 tests                                 (unit)
test result: ok. 156 passed; 0 failed

running 5 tests                                   (cross-corpus integration)
test result: ok. 5 passed; 0 failed

running 9 tests                                   (proptest round-trips)
test result: ok. 9 passed; 0 failed

running 22 tests                                  (per-sample assertions)
test result: ok. 22 passed; 0 failed

running 1 test                                    (doctest)
test result: ok. 1 passed; 0 failed

Total: 193 tests, 0 failures.
```

Each pull request runs on CI against:

- `stable` and MSRV (1.85) toolchains
- Linux, macOS, Windows (`ubuntu-latest`, `macos-latest`, `windows-latest`)
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `cargo doc`, `cargo deny check`

See [`.github/workflows/ci.yml`](./.github/workflows/ci.yml) for the workflow spec.

## Safety

```rust
#![deny(unsafe_code)]
```

The entire crate is safe Rust. There is no `unsafe` anywhere — Reed-Solomon, LZ77, GF(256), bit-cursor, and bit-writer are all implemented without raw pointers or undefined-behavior primitives.

Malformed-input posture:

- Every parser returns `Result<T, Error>` — no panics on adversarial input.
- Defensive caps bound runaway allocations (1M-entry dictionaries, 16 MB XRECORDs, 1M-vertex splines).
- Decompression-bomb mitigation: callers should pass `expected_size` to `lz77::decompress` when known.

See [SECURITY.md](./SECURITY.md) for the full threat model and private vulnerability reporting.

## MSRV policy

- **Minimum supported Rust version: 1.85** (for `edition = "2024"`).
- MSRV bumps are minor-version events and announced in the [CHANGELOG](./CHANGELOG.md).
- CI verifies MSRV on every PR — a patch that raises it will be caught before merge.

## Related projects

| Project | Language | License | Notes |
|---------|----------|---------|-------|
| [ACadSharp](https://github.com/DomCR/ACadSharp) | C# | MIT | Permissive reference; `dwg-rs` cross-checked LZ77 offsets against it |
| [LibreDWG](https://www.gnu.org/software/libredwg/) | C | **GPL-3** | Mature but copyleft; not consulted in the clean-room build of this crate |
| [dxf-rs](https://github.com/ixmilia/dxf-rs) | Rust | MIT | Handles DXF (the text companion format), not DWG |
| [ezdxf](https://ezdxf.readthedocs.io/) | Python | MIT | DXF-only |
| [Teigha / ODA SDK](https://www.opendesign.com/) | C++ | Commercial | Proprietary; requires membership + license fee |

## FAQ

**Why not just use LibreDWG?**
LibreDWG is GPL-3. If you're building proprietary software or distributing an Apache-2-licensed product, LibreDWG either forces you to relicense or it's legally off-limits. `dwg-rs` is Apache-2, no copyleft.

**Is this affiliated with Autodesk or the ODA?**
No. This is an independent clean-room implementation against a publicly-redistributable specification, protected by the 17 U.S.C. § 1201(f) interoperability exception and the 2006 *Autodesk v. ODA* settlement.

**Can it write DWG files?**
Partially. The inverse of the read path — bit-writer, LZ77 encoder (literal-only, correctness-first), per-section framer with Sec_Mask + CRC, and the `WriterScaffold` facade — all ship today. The final stage that rewrites the file-open header and the page-map / section-info system pages is scaffolded (see [`file_writer.rs`](./src/file_writer.rs)) and tracked in the CHANGELOG as the next release's primary feature.

**Is R2007 supported?**
Partially. R2007 uses a two-layer Sec_Mask obfuscation (spec §5) that no other version in the R2004 family uses. Layer 1 (byte-level XOR) is implemented; layer 2 (bit-level 7-byte window rotation with cumulative bookkeeping) is scaffolded but not wired into the reader yet. R14, R2000, R2004, R2010, R2013, R2018 are fully supported.

**How big are the sample files in the test corpus?**
The 19 sample files under `samples/` in the development tree are sourced from the public `nextgis/dwg_samples` repository. They are deliberately **not** included in the published crate — `tests/corpus_roundtrip.rs` skips gracefully when they are absent so this crate remains usable as a pure dependency.

## Contributing

Issues and pull requests are welcome. Before you submit:

- Read [ARCHITECTURE.md](./ARCHITECTURE.md) to understand the module boundaries.
- Run `cargo fmt --all`, `cargo clippy -D warnings`, and `cargo test` locally.
- Cite the ODA spec section for any new decoder you add.
- For PRs touching the decoder path: confirm in the PR body that you have **not** consulted Autodesk's, ODA's, or LibreDWG's source code. This is the clean-room requirement.
- Follow the [Contributor Covenant 2.1](https://www.contributor-covenant.org/version/2/1/code_of_conduct/) code of conduct.

Security vulnerabilities: do **not** open a public issue. Report privately via [GitHub Security Advisories](https://github.com/DrunkOnJava/dwg-rs/security/advisories/new) — see [SECURITY.md](./SECURITY.md).

## Legal posture

DWG is a trademark of Autodesk, Inc. This crate is not affiliated with, authorized by, or endorsed by Autodesk. It is a clean-room third-party implementation made for interoperability purposes, protected by:

- **17 U.S.C. § 1201(f)** — the Digital Millennium Copyright Act's reverse-engineering-for-interoperability exception.
- **Autodesk, Inc. v. Open Design Alliance**, N.D. Cal. 2006 (settled) — which explicitly permits third-party DWG implementations.

The authoritative reference is the Open Design Alliance's *Open Design Specification for .dwg files* (v5.4.1), a freely-redistributable document.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](./LICENSE) for the full text.

Contributions are accepted under the same terms per the standard Apache-2 inbound = outbound convention.
