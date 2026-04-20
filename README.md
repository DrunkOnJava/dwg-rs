# dwg-rs

> **Clean-room, Apache-2.0 Rust foundation for Autodesk DWG files (R13 → R2018 / AC1032).**
> No Autodesk SDK. No ODA SDK. No GPL-3 dependency. To our knowledge, the first Apache-2.0, clean-room Rust foundation focused specifically on DWG container parsing and interoperability.

[![CI](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Rust MSRV](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)

<!-- Crates.io and docs.rs badges will be added here once the crate is published. -->


---

## ⚠ Pre-alpha status — read this first

This is **0.1.0-alpha.1**. Do not use it in production. Do not benchmark it against the ODA SDK. Do not tell your CAD team dwg-rs solves their interop problem today.

Empirical entity-decode coverage as measured by
[`examples/coverage_report.rs`](./examples/coverage_report.rs) against the
19-file `nextgis/dwg_samples` corpus + a 1 MB AC1032 file, after the
dimension-subtype fix (task #71):

| Version | Files tested | Entities attempted | Decoded | Errored | Success rate |
|---------|--------------|--------------------|---------|---------|--------------|
| R14 / R2000 / R2007 | 7 | 0 | 0 | 0 | not supported (no handle-map for this layout yet) |
| R2004 (AC1018)      | 3 | 7 per file | 0 | 7 | **0 %** |
| R2010 (AC1024)      | 3 | 7 per file | 3 | 4 | **43 %** |
| R2013 (AC1027)      | 3 | 7 per file | 6 | 1 | **86 %** |
| R2018 (AC1032)      | 1 | 306 attempted (of 745 objects; 439 are non-entity controls/dictionaries) | 66 | 240 | **22 %** |
| **Aggregate** | **19** | **369 entities attempted** | **93** | **276** | **25 %** |

Per-entity-type error concentration in the R2018 sample (where most real data is):

| Type code | DXF name | Occurrences as error |
|-----------|----------|-----------------------|
| 0x13 (19) | `LINE` | 82 |
| 0x2C (44) | `MTEXT` | 33 |
| 0x1B (27) | `POINT` | 26 |
| ... | (long tail) | 99 |

**Translation:** the 27 entity decoders in [`src/entities/*.rs`](./src/entities/)
are verified against hand-crafted synthetic input (193 unit + proptest + sample tests pass)
but fail on real-world files because the common-entity preamble, extended-data loop, and
handle-stream layout in production DWG files has version-specific deviations this
crate doesn't yet fully model. This is the gap between "decoder functions exist" and
"decoders work end-to-end." Closing it is the 0.2.0 milestone.

## Capability matrix at a glance

| Layer | Status | Notes |
|-------|--------|-------|
| File identification (AC1014 → AC1032) | ✓ shipped | All 8 versions recognized |
| R13–R15 header parsing | ✓ shipped | Plain + XOR-encrypted variants |
| R18 / R21 / R24 / R27 / R32 header parsing | ✓ shipped | All shipped variants |
| LZ77 decompression + output-limit caps | ✓ shipped | 256 MiB default, configurable |
| Section Page Map + Section Info | ✓ shipped | Plus fallback path via `SectionMapStatus` |
| Sec_Mask layer-1 (R2004 family) | ✓ shipped | Layer-2 R2007 bookkeeping deferred |
| CRC-8 + CRC-32 verification | ✓ shipped | — |
| Reed-Solomon (255,239) FEC decoder | ✓ shipped | Encoder pending (#109) |
| Metadata (SummaryInfo / AppInfo / Preview / FileDepList) | ✓ shipped | UTF-16 auto-detect, PNG thumbnail carve |
| HandleMap + ClassMap parsing | ✓ shipped | — |
| Header variables | ✓ shipped | Strict + lossy variants |
| Object-stream walker (R2004+) | ✓ shipped | R14 / R2000 / R2007 pending (#104) |
| Per-entity field decoders | ⚠ alpha | 27 types defined; R2013 best ~86%, real-file coverage gap (#103) |
| Entity graph (owner / reactors / blocks / layers) | ⏳ pending | Phase 5 of roadmap |
| Symbol tables (LAYER / LTYPE / STYLE / DIMSTYLE / …) | ✓ dispatch shipped | Content-field decode pending |
| SVG / PDF export | ⏳ pending | Phase 9 of roadmap |
| DXF writer | ⏳ pending | Phase 11 of roadmap |
| DWG writer | ⚠ scaffold only | Stage 1 of 5; #105–#108 track the remainder |
| glTF 3D export | ⏳ pending | Phase 10 of roadmap |
| WASM viewer | ⏳ pending | Phase 13 of roadmap |
| Python bindings | ⏳ pending | No PyO3 crate yet |

✓ shipped · ⚠ alpha/partial · ⏳ pending

## What *does* work today

The container layer is the most mature part of the crate and is covered by 193 tests:

- Version identification across AC1014 (R14, 1997) → AC1032 (2018, 2024+)
- R13–R15 simple file header + R2004+ XOR-encrypted header
- Section Page Map + Section Info parsing
- LZ77 decompression — the ODA spec's offset-encoding description is ambiguous in one place; this crate's implementation matches a publicly-documented errata reading (algorithm description only, no implementation code consulted)
- Sec_Mask layer-1 un-masking for every R2004-family version
- `DwgFile::read_section(name)` — decompressed bytes for any named section
- CRC-8 + CRC-32 verification
- Reed-Solomon(255,239) FEC decoder over GF(256) (defensive path)
- Metadata parsers: `SummaryInfo`, `AppInfo` (R18 ANSI + R21+ UTF-16 auto-detected),
  `Preview` (BMP, WMF, modern PNG code-6 fallback), `FileDepList`
- Handle map parser, class map parser, header-variable bit-stream extraction
- Object-stream walker: `all_objects()` returns `Vec<RawObject>` with type codes,
  handles, and raw payload bytes — this part works on R2018 (745 objects enumerated
  cleanly from the sample) and gives you enough to build your own per-version entity
  dispatcher if you need one sooner than 0.2.0 ships
- Partial write path: bit-writer + LZ77 literal-only encoder + per-section framer
  with Sec_Mask and CRC; the file-level `WriterScaffold` scaffold is stage 1 of 5

### Hard-no list — what dwg-rs does NOT do today

- End-to-end entity decoding on most real R2004-family files (see coverage table above).
- R14 / R2000 object-stream walking (different layout from R2004-family; not yet implemented).
- R2007 section payloads (layer-1 Sec_Mask shipped, layer-2 bit-rotation scaffolded only).
- Full HATCH boundary path tree, full MLEADER leader-line list, full 75-field DIMSTYLE.
- `DwgFile::to_bytes()` — scaffolded, stages 2–5 (page-map + section-info + system-page + file-open-header rebuild) are a future release.

## Install

```bash
# From git (currently the only distribution):
git clone https://github.com/DrunkOnJava/dwg-rs
cd dwg-rs
cargo build --release
```

The 0.1.0-alpha.1 crate has not been published to crates.io yet, and won't be until
the entity-decoder coverage hits a responsible baseline.

## Use the parts that work

```rust
use dwg::DwgFile;

fn main() -> dwg::Result<()> {
    let file = DwgFile::open("drawing.dwg")?;
    println!("version: {}", file.version());
    println!("sections: {}", file.sections().len());

    // Decompressed bytes for any named section — this is fully reliable.
    if let Some(Ok(bytes)) = file.read_section("AcDb:Preview") {
        println!("preview section: {} bytes", bytes.len());
    }

    // Structured metadata — works on every corpus file we've tested.
    if let Some(Ok(summary)) = file.summary_info() {
        println!("title:  {}", summary.title);
        println!("author: {}", summary.author);
    }

    // Handle-indexed object walk — works on R2004+ but skips R14/R2000/R2007.
    if let Some(Ok(objects)) = file.all_objects() {
        println!("raw objects: {}", objects.len());
    }

    // End-to-end entity decode — alpha quality; check the returned
    // DispatchSummary's decoded_ratio() for honest per-file coverage
    // before relying on the output.
    if let Some(Ok((entities, summary))) = file.decoded_entities() {
        println!(
            "entities: {} decoded / {} skipped / {} errored ({:.1}% decoded)",
            summary.decoded,
            summary.unhandled,
            summary.errored,
            summary.decoded_ratio() * 100.0
        );
    }

    Ok(())
}
```

Other examples live in [`examples/`](./examples/):

- [`basic_open.rs`](./examples/basic_open.rs)
- [`walk_entities.rs`](./examples/walk_entities.rs)
- [`extract_preview.rs`](./examples/extract_preview.rs)
- [`dump_metadata.rs`](./examples/dump_metadata.rs)
- [`coverage_report.rs`](./examples/coverage_report.rs) — run this against your own files to see how much of your data dwg-rs can actually parse today

## CLI tools

```bash
dwg-info drawing.dwg                                        # version + section list
dwg-corpus /path/to/corpus/                                 # sweep a directory
dwg-dump drawing.dwg                                        # hierarchical dump
dwg-convert --extract AcDb:Preview -o preview.bmp x.dwg     # decompressed section
dwg-convert --verify drawing.dwg                            # all-sections decompress check
```

## Architecture

See [`ARCHITECTURE.md`](./ARCHITECTURE.md) for the design deep-dive — format primer,
module responsibilities, the four-phase read pipeline, Sec_Mask explanation, and
the LZ77 spec-errata corrections.

Quick layer overview:

```
  DwgFile::open ─────────────────────────────────────────┐
         │                                               │
         ▼                                               ▼
   header + section_map                      (R2004+ only) handle_map
         │                                               │
         ▼                                               ▼
   read_section("AcDb:*")                       all_objects()  ──► [shipping]
         │                                               │
         ▼                                               ▼
   metadata::* (SummaryInfo,                   decoded_entities() ──► [alpha]
   AppInfo, Preview, FileDepList)                       │
                                                        ▼
                                         dispatch on type_code → entities::*
                                                        │
                                                        ▼
                                                  per-entity struct
```

## Verification

```text
$ cargo test --release
156 unit tests    + 5 corpus + 9 proptest + 22 samples + 1 doctest = 193 tests, 0 failures
$ cargo clippy --all-targets --all-features -- -D warnings       # clean
$ cargo fmt --all -- --check                                      # clean
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features   # clean
$ cargo deny check                                                # no advisories, no disallowed licenses
```

Tests exercise the container layer end-to-end across all 19 corpus files and verify
bit-level round-trip properties for every primitive. They do **not** verify that every
entity decoder succeeds on every real-world drawing — that's what the 22 % / 43 % /
85 % coverage numbers above measure. Both classes of testing are needed.

## Safety

The whole crate is `#![deny(unsafe_code)]`. Reed-Solomon, LZ77, GF(256), bit-cursor,
and bit-writer are all safe Rust. Every parser returns `Result<T, Error>`. Defensive
caps bound runaway allocations (1 M dictionary entries, 16 MB XRECORDs, 1 M spline
control points). See [`SECURITY.md`](./SECURITY.md) for threat model + private
reporting.

## MSRV policy

Rust 1.85 (for `edition = "2024"`). MSRV bumps are minor-version events announced in
the [CHANGELOG](./CHANGELOG.md). CI verifies MSRV on every PR.

## Related projects

| Project | Language | License | Notes |
|---------|----------|---------|-------|
| [ACadSharp](https://github.com/DomCR/ACadSharp) | C# | MIT | Permissive reference — `dwg-rs` cross-checked LZ77 offset errata against it (not its source, just the algorithm in comments) |
| [LibreDWG](https://www.gnu.org/software/libredwg/) | C | **GPL-3** | Mature but copyleft; not consulted at any point during this crate's build |
| [Teigha / ODA SDK](https://www.opendesign.com/) | C++ | Commercial | Proprietary; paid membership required |
| [dxf-rs](https://github.com/ixmilia/dxf-rs) | Rust | MIT | DXF (text companion format) only |
| [rvt-rs](https://github.com/DrunkOnJava/rvt-rs) | Rust | Apache-2 | Sibling project — clean-room Autodesk Revit (.rvt / .rfa) reader by the same author. Same CLEANROOM posture. |

## Why this exists

For ~28 years the DWG format has been a moat. Autodesk never published a spec. The ODA's SDK requires membership and introduces licensing constraints. LibreDWG is GPL-3, which disqualifies it from most commercial stacks.

To our knowledge, `dwg-rs` is the first Apache-2.0, clean-room Rust foundation focused specifically on DWG container parsing and interoperability without Autodesk SDK, ODA SDK, or GPL-derived implementation code. It is **not** a finished DWG reader — it is a foundation other people can build on without paying SDK tolls or pulling GPL-3 into their dependency graph.

## Contributing

The project needs help, in rough order of impact:

1. **Per-version entity preamble fixes** — figuring out why LINE and MTEXT fail on R2004/R2010/R2018 real files. This is the single biggest gap between the current measured decode rate and a shippable reader.
2. **R14 / R2000 object-stream walker** — completely different layout from R2004-family.
3. **R2007 Sec_Mask layer-2 bookkeeping** — spec §5.2.
4. **Fuzz-testing targets** — cargo-fuzz harnesses for LZ77 decompress, bit-cursor, and object walker.
5. **Write-path stages 2–5** — page-map / section-info / system-page / file-open-header rebuild.

Before submitting a PR:

- Run `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- Cite the ODA spec section for any new decoder behavior.
- **Clean-room declaration**: confirm in the PR body that you have not consulted Autodesk SDK source, ODA SDK source, or LibreDWG (GPL-3) source for the contribution. The PR template has a checkbox for this.
- Follow the [Contributor Covenant 2.1](https://www.contributor-covenant.org/version/2/1/code_of_conduct/) code of conduct.

Security vulnerabilities: report privately via [GitHub Security Advisories](https://github.com/DrunkOnJava/dwg-rs/security/advisories/new) — see [SECURITY.md](./SECURITY.md).

## Legal posture

"Autodesk", "AutoCAD", and "DWG" are trademarks of Autodesk, Inc. This crate is not affiliated with, authorized by, or endorsed by Autodesk.

`dwg-rs` is intended as a clean-room interoperability implementation. It does not use Autodesk SDK source, ODA SDK (Drawings SDK / Teigha) source, or GPL-licensed DWG implementation source. The authoritative reference is the ODA's freely-redistributable *Open Design Specification for .dwg files* (v5.4.1) — a document distinct from ODA's SDK license.

Users with specific legal constraints should evaluate the project with their own counsel. `NOTICE` summarizes the relevant public authority that typically supports independent file-format reverse engineering for interoperability (e.g. *Sega v. Accolade*, *Sony v. Connectix*, EU Software Directive Art. 6, Australia Copyright Act §47D) — it is not offered as a legal opinion.

No Autodesk SDK, no ODA SDK, and no LibreDWG source was consulted at any point.

## License

Apache-2.0. See [LICENSE](./LICENSE). Contributions land under the same terms per the standard inbound = outbound convention.
