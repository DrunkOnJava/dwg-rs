# dwg-rs

> **Apache-2.0 Rust reader for Autodesk DWG files (R13 → R2018 / AC1032), built from the Open Design Alliance's published specification.** Pre-alpha; container-layer parsing shipping, per-entity decoders have documented coverage gaps.

[![CI](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/ci.yml)
[![Perf](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/perf.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/perf.yml)
[![Doc build](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/docs-rs.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/docs-rs.yml)
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

The container layer is the most mature part of the crate and is covered by the test suite (run `cargo test` for the current count; 645 unit + ~100 integration + 10 doctests as of 2026-04-20):

- Version identification across AC1014 (R14, 1997) → AC1032 (2018, 2024+)
- R13–R15 simple file header + R2004+ XOR-encrypted header
- Section Page Map + Section Info parsing
- LZ77 decompression — the ODA spec's offset-encoding description is ambiguous in one place; this crate's implementation was cross-checked against the algorithm-description comments in the MIT-licensed [ACadSharp](https://github.com/DomCR/ACadSharp) source (no executable code imported, comments-only). See [`CLEANROOM.md`](./CLEANROOM.md) for the specific scope of what was and wasn't consulted.
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

Seven binaries ship behind the `cli` feature flag. Inspection tools
work against any file the container layer can parse; export tools
(`dwg-to-*`) work to the extent that the per-entity decoders for
your file's version do — see the
[compatibility matrix](./docs/landing/compatibility.md).

```bash
# Inspection
dwg-info drawing.dwg                                        # version + section list
dwg-corpus /path/to/corpus/                                 # sweep a directory
dwg-dump drawing.dwg                                        # hierarchical dump
dwg-convert --extract AcDb:Preview -o preview.bmp x.dwg     # decompressed section
dwg-convert --verify drawing.dwg                            # all-sections decompress check

# Export (pre-alpha, spec-syntactic; real-app acceptance is manual)
dwg-to-dxf drawing.dwg out.dxf --version R2018              # ASCII DXF (R12..R2018)
dwg-to-gltf drawing.dwg out.glb                             # glTF 2.0 binary (.glb)
dwg-to-gltf drawing.dwg out.gltf                            # glTF JSON + sidecar .bin

# Write scaffolding (Stage 1 of 5 — does NOT emit valid DWG yet)
dwg-write --version R2018 \
  --section AcDb:Header=header.bin \
  --section AcDb:SummaryInfo=summary.bin \
  --report stage1.json
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
# 645 unit tests + integration suites (code_table, corpus_roundtrip,
# dispatch_roundtrip, dxf_roundtrip, entity_regression, fuzz_corpus,
# gltf, svg_goldens, write_roundtrip, mutation_failure, proptest,
# samples) + 10 doctests. Exact count grows with each commit; check
# the final `test result:` lines for current numbers.
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
| [LibreDWG](https://www.gnu.org/software/libredwg/) | C | **GPL-3** | The most complete open-source DWG reader; preferable to `dwg-rs` today for any stack that can take GPL-3. Its source was not consulted during this crate's implementation |
| [Teigha / ODA SDK](https://www.opendesign.com/) | C++ | Commercial | Proprietary; paid membership required |
| [dxf-rs](https://github.com/ixmilia/dxf-rs) | Rust | MIT | DXF (text companion format) only |
| [rvt-rs](https://github.com/DrunkOnJava/rvt-rs) | Rust | Apache-2.0 | Sibling project — Autodesk Revit (.rvt / .rfa) reader by the same author, same source-provenance policy. |

## Why this exists

DWG is Autodesk's proprietary format. Autodesk does not publish a specification. What's available to open-source implementers is:

- The **Open Design Alliance's** Open Design Specification — the result of the ODA's own long-standing reverse-engineering effort, made available publicly. `dwg-rs` is built from version 5.4.1 of that spec.
- **LibreDWG** (GPL-3) — the most complete open-source DWG reader today. If GPL-3 fits your project, it is almost certainly the better tool.
- **ACadSharp** (MIT, C#) — a mature .NET DWG reader for stacks that can take a C# dependency.
- **Teigha / the ODA SDK** — a commercial C++ SDK appropriate for production workloads that can afford the membership.

`dwg-rs` occupies a narrow niche: a permissively-licensed (Apache-2.0) Rust crate for reading the DWG *container* (file header, section map, LZ77 decompression, metadata sections, object stream), implemented from the ODA specification without linking against the ODA SDK and without reusing GPL-licensed source code. It is useful when your stack can't take GPL-3, can't justify an ODA membership, and needs a Rust dependency rather than an FFI binding.

It is **pre-alpha and not a finished DWG reader.** The container layer is shipping; per-entity decoders have known gaps documented in the coverage table above. See [`CLEANROOM.md`](./CLEANROOM.md) for the implementation discipline this project follows, including the honest scope of what "clean-room" means for a solo-developer project — it is a spec-only, no-reference-source posture, not a formal two-team protocol.

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
- **Source-provenance declaration**: confirm in the PR body that your contribution does not incorporate executable code (not just comments, not just API shapes) from any source whose license is incompatible with Apache-2.0 — in particular, no Autodesk SDK source, no ODA SDK / Teigha source, and no GPL-licensed DWG implementation source (LibreDWG). Reading algorithm-description comments in permissively-licensed projects (MIT / Apache / BSD) to resolve a spec ambiguity is allowed and should be disclosed in the PR body so we can record it in [`CLEANROOM.md`](./CLEANROOM.md). The PR template has a checkbox for this.
- Follow the [Contributor Covenant 2.1](https://www.contributor-covenant.org/version/2/1/code_of_conduct/) code of conduct.

Security vulnerabilities: report privately via [GitHub Security Advisories](https://github.com/DrunkOnJava/dwg-rs/security/advisories/new) — see [SECURITY.md](./SECURITY.md).

## Legal posture

"Autodesk", "AutoCAD", and "DWG" are trademarks of Autodesk, Inc. This crate is not affiliated with, authorized by, or endorsed by Autodesk.

`dwg-rs` is a spec-based interoperability implementation. The authoritative reference is the Open Design Alliance's freely-redistributable *Open Design Specification for .dwg files* (v5.4.1) — a document distinct from the ODA's Drawings SDK (Teigha) license. Executable code from the Autodesk SDK, the ODA SDK, and GPL-licensed DWG implementations (LibreDWG) was not consulted or imported at any point. One clearly-scoped exception is documented in [`CLEANROOM.md`](./CLEANROOM.md): algorithm-description comments (not executable code) from the MIT-licensed ACadSharp were consulted to resolve one LZ77 offset-encoding spec ambiguity.

Independent reverse engineering for interoperability is generally supported across jurisdictions by authorities such as *Sega v. Accolade* (9th Cir. 1992) and *Sony v. Connectix* (9th Cir. 2000) in the United States, Article 6 of the EU Software Directive (2009/24/EC), and comparable provisions elsewhere. See [`NOTICE`](./NOTICE) for a fuller reference set. Nothing in this repository is offered as legal advice; users with specific legal constraints should consult their own counsel.

## License

Apache-2.0. See [LICENSE](./LICENSE). Contributions land under the same terms per the standard inbound = outbound convention.
