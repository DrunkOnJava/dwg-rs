# dwg-rs

> **Apache-2.0 Rust reader for Autodesk DWG files (R13 → R2018 / AC1032), built from the Open Design Alliance's published specification.** Pre-alpha; container-layer parsing shipping, per-entity decoders have documented coverage gaps.

[![CI](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/ci.yml)
[![Perf](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/perf.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/perf.yml)
[![Doc build](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/docs-rs.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/docs-rs.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Rust MSRV](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)

<!-- Crates.io and docs.rs badges will be added here once the crate is published. -->


---

## Streaming architecture work

The new [streaming core](docs/streaming/README.md) reads the complete
R13–R2000 **file header** with explicit byte demands, fixed storage, pausable
sources and portable partial-state checkpoints. Native E2E and the identical
no-allocator grammar compiled for native/WASM are measured in the linked
report. This is the first stage of [#204](https://github.com/DrunkOnJava/dwg-rs/issues/204),
not a streaming model reader: the existing `DwgFile` and browser APIs still
load full files/sections. R2004+ streaming, objects, decompression state, the
spec compiler and large-model evidence remain open. Competitive success-rate,
object-coverage and memory comparisons for the streaming reader are **NOT
MEASURED**; no claim of superiority to another DWG reader is made.

## ⚠ Pre-alpha status — read this first

This is **0.1.0-alpha.1**. Do not use it in production. Do not benchmark it against the ODA SDK. Do not tell your CAD team dwg-rs solves their interop problem today.

Entity and object decode coverage, measured in CI by the `Corpus (public DWG)`
job on every push to `main` ([run 40304926288](https://github.com/DrunkOnJava/dwg-rs/actions/runs/40304926288)) with
[`examples/coverage_report.rs`](./examples/coverage_report.rs), on 52 public
drawings fetched by pinned sha256
([`tools/corpus/pins.tsv`](./tools/corpus/pins.tsv); none is committed):

| Version | Files tested | Decoded | Skipped | Errored | Success rate |
|---------|--------------|---------|---------|---------|--------------|
| R14 (AC1014)        | 8 | 1841 | 1351 | 105 | **55.8 %** |
| R2000 (AC1015)      | 8 | 2337 | 145 | 51 | **92.3 %** |
| R2004 (AC1018)      | 8 | 2218 | 109 | 90 | **91.8 %** |
| R2007 (AC1021)      | 8 | 2090 | 132 | 15 | **93.4 %** |
| R2010 (AC1024)      | 8 | 2088 | 85 | 15 | **95.4 %** |
| R2013 (AC1027)      | 8 | 1693 | 85 | 9 | **94.7 %** |
| R2018 (AC1032)      | 4 | 1514 | 78 | 23 | **93.7 %** |
| **Aggregate** | **52** | **13781** | **1985** | **308** | **85.7 %** |

The drawings come from [nextgis/dwg_samples](https://github.com/nextgis/dwg_samples)
(seven drawings, each saved by AutoCAD 2016 as R14 to R2013; no licence is
stated, so they are fetched for the run and never shared) and from the
MIT-licensed `samples/` of [DomCR/ACadSharp](https://github.com/DomCR/ACadSharp)
(one drawing saved by AutoCAD 2025 as R14 to R2018, and three more R2018
drawings). Every save of those drawings except R14 has AutoCAD's own DXF
export of the same drawing beside it; of the three extra R2018 drawings only
`geoloc.dwg` has one. [`CLEANROOM.md`](./CLEANROOM.md) records the provenance.

**The 9 errors this README used to report were a property of the old
corpus.** The 19 local files every earlier number came from are a subset of
this one, and the same run still measures them at 3911 / 525 / 9 / 88.0 %.
The 33 drawings CI added are nextgis's other 24, which decode with no error,
and nine from ACadSharp. Six of those are the R14 to R2013 saves of the
drawing whose R2018 save is `sample_AC1032.dwg`; they hold record types the
old corpus never had on those releases and error on 285 records. Another
R2018 drawing, `export_sample.dwg`, errors on 14 more. Every errored type is
tracked:

- TEXT, MTEXT, ATTDEF, ATTRIB and TOLERANCE on R14 to R2004, 199 records (#226);
- every INSERT and DIMENSION on R14, and a 3DFACE, 24 records (#227);
- VIEW, DIMSTYLE, 3DSOLID, REGION and HATCH on some releases, 19 records (#228);
- VIEWPORT (45 records), MESH (14) and LEADER (7) on every release (#71).

[`tools/corpus/baseline.tsv`](./tools/corpus/baseline.tsv) pins each
drawing's decoded, skipped and errored counts, its class-census shortfalls
and its reference closure. The job fails on any difference, in either
direction, so a regression is red and an improvement is recorded in the
pull request that makes it.

**Every entity record in the corpus is checked against its own data-stream
boundary; the 308 errors are the records that fail it.** Until #63 the
POLYLINE family, MESH, IMAGE, VIEWPORT, LEADER, MLINE, the surfaces, RAY /
XLINE / POINT / CIRCLE / ARC / LINE / ELLIPSE / SOLID / TRACE / BLOCK /
ENDBLK and the whole R2000-R2004 band decoded with **no boundary check at
all**, so a zero error count was a property of the code rather than of the
bytes. An error that names an unfinished field list is worth more than a
zero that means nothing.

Every decoder in this crate is *self-validating*: its data fields must
end exactly on the first bit of the record's string stream, on the bit
before the `strings present` trailer flag when it carries no strings
(R2010+), or on the `RL` object-data-size from the object prologue
(R2000 / R2004). A field list that is wrong anywhere lands somewhere
else and reports an error rather than returning plausible-looking
geometry. On R2010+ there is no longer any entity type that decodes
unchecked, and since #104/#110 that holds on every other release too:
R2007's boundary is the same `RL` object-data-size read through the
§19.1 trailer, and R13/R14 state theirs inside the common entity data
(§20.4.1) rather than in the object prologue. **No entity in the corpus
decodes unchecked on any release.**

**Translation:** R2007+ objects store every `TV` field in a separate string
stream and every `H` object reference in a separate handle stream (ODA
§19.1); [`src/string_stream.rs`](./src/string_stream.rs) locates both, and
every modern decoder reads its text from the first, treats a handle slot as
consuming no data-stream bits, and then asserts it landed exactly on the
boundary. LAYER, LTYPE, STYLE, UCS, VIEW, VPORT, APPID, DIMSTYLE,
BLOCK_HEADER, TEXT, ATTRIB, ATTDEF, MTEXT, TOLERANCE, HATCH, MULTILEADER,
the DIMENSION family, INSERT, SPLINE, LWPOLYLINE, 3DFACE and the UNDERLAY
family all go through it, and since #63 so does **every other entity
type** — via the same check, on the R2000/R2004 band as well as R2010+.
The R2004 (AC1018) object prologue is read correctly too — an `RL` object
data size in bits sits between the object type and the object handle on
the whole R2000..R2007 band, and skipping it put every AC1018 record 32
bits out of phase; that `RL` is now also the boundary AC1018 entities are
held to.

Two gaps remain. The *skipped* list is the larger one: MATERIAL (203) and
TABLESTYLE have no field list matched against real bytes (XRECORD on R14,
1154 records, was the largest until #149 routed it), and LAYOUT, MULTILEADER, LWPOLYLINE and HATCH have
decoders that are never reached on R14 to R2007 because those releases give
them a class number dispatch does not route (#229). The *errored* list is
above.
Closing both is the 0.2.0 milestone.

## Roadmap

The plan to 1.0 is the [milestones](https://github.com/DrunkOnJava/dwg-rs/milestones)
and their epics, described in [`ROADMAP.md`](./ROADMAP.md).
[`docs/spec-coverage.md`](./docs/spec-coverage.md) gives every section of
ODA spec v5.4.1 its state on `main`, and
[`docs/release-1.0-criteria.md`](./docs/release-1.0-criteria.md) is the 1.0
release decision: no release tagged 1.0 until every requirement there has
passed.

## Capability matrix at a glance

| Layer | Status | Notes |
|-------|--------|-------|
| File identification (AC1012 → AC1032) | ✓ shipped | All 8 versions recognized |
| R13–R15 header parsing | ✓ shipped | Plain + XOR-encrypted variants |
| R18 / R21 / R24 / R27 / R32 header parsing | ✓ shipped | All shipped variants |
| LZ77 decompression + output-limit caps | ✓ shipped | 256 MiB default, configurable |
| Section Page Map + Section Info | ✓ shipped | Plus fallback path via `SectionMapStatus` |
| Sec_Mask layer-1 (R2004 family) | ✓ shipped | R2007 does not use Sec_Mask at all — see `src/r2007.rs` |
| CRC-8 + CRC-32 verification | ✓ shipped | — |
| Reed-Solomon (255,239) FEC | ✓ shipped | Decoder + writer-side encoder; read-side fallback wiring pending (#109) |
| Metadata (SummaryInfo / AppInfo / Preview / FileDepList) | ✓ shipped | UTF-16 auto-detect, PNG thumbnail carve |
| HandleMap + ClassMap parsing | ✓ shipped | — |
| Header variables | ✓ shipped | Strict + lossy variants |
| Object-stream walker | ✓ shipped | All eight releases — R13-R15 flat locator, R2004-family page map, R2007 §5 container |
| R2007 (AC1021) container | ✓ shipped | Reed-Solomon de-interleave, §5.10 LZ, page map, section map (#110) |
| Per-entity field decoders | ⚠ alpha | 85.7 % of 16,074 records on 52 public drawings, measured in CI; every entity boundary-checked; 308 errors in named types (#226, #227, #228, #71) |
| Entity graph (owner / reactors / blocks / layers) | ⚠ partial | Resolver APIs exist; trailing-handle/block traversal gaps remain |
| Symbol tables (LAYER / LTYPE / STYLE / DIMSTYLE / …) | ⚠ partial | R2007+ BLOCK_HEADER and simple LTYPE names decode; broader content fields pending |
| SVG / PDF export | ⚠ alpha | SVG writer + paged-SVG PDF path; output quality depends on decoded geometry |
| DXF writer | ⚠ alpha | Writer exists for R12..R2018 targets; DXF parser is not implemented |
| DWG writer | ⚠ alpha | `DwgFile::to_bytes()` and R2004-family byte assembly exist; external CAD acceptance not automated |
| glTF 3D export | ⚠ alpha | glTF/GLB writer exists; surface tessellation remains approximate |
| WASM viewer | ⚠ partial | Core APIs and 2D viewer pieces exist; selection/progressive/3D APIs still stubbed |
| Python bindings | ⏳ pending | Stub module only; no usable PyO3 package yet |

✓ shipped · ⚠ alpha/partial · ⏳ pending

## What *does* work today

The container layer is the most mature part of the crate and is covered by the test suite (run `cargo test` for the current count; 649 lib tests plus integration suites and 10 doctests as of 2026-04-29):

- Version identification across AC1012 (R13, 1994) → AC1032 (2018, 2024+)
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
- Experimental R2004-family write path: bit-writer + LZ77 literal-only encoder,
  section framer, Sec_Mask, CRC, page-map/section-info assembly, and final
  byte-buffer assembly via `DwgFile::to_bytes()` /
  `file_writer::assemble_dwg_bytes`

### Hard-no list — what dwg-rs does NOT do today

- End-to-end entity decoding on most real R2004-family files (see coverage table above).
  R2013/R2018 LINE/CIRCLE/ARC alignment is now pinned; many text,
  insert, dimension, hatch, LWPOLYLINE flag variants, and table-object paths still
  need real-file field-layout work.
- Password-protected R2007 files (a section whose descriptor declares
  encryption is refused, not decoded).
- Writing R14 or R2007 files (both containers are implemented read-only).
- Full HATCH boundary path tree, full MLEADER leader-line list, full 75-field DIMSTYLE.
- Byte-identical DWG round-trips or automated external CAD application
  acceptance for `DwgFile::to_bytes()` output.
- A non-technical end-user application, installer, or drag-and-drop GUI.

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

    // Handle-indexed object walk — works on every release from R14 on.
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
# 649 lib tests + integration suites (code_table, corpus_roundtrip,
# dispatch_roundtrip, dxf_roundtrip, entity_regression, fuzz_corpus,
# gltf, svg_goldens, write_roundtrip, mutation_failure, proptest,
# samples) + 10 doctests. Exact count grows with each commit; check
# the final `test result:` lines for current numbers.
$ cargo clippy --all-targets --all-features -- -D warnings       # clean
$ cargo fmt --all -- --check                                      # clean
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features   # clean
$ cargo deny check                                                # no advisories, no disallowed licenses
```

The real-file suites (`samples`, `class_census`, `block_names`, `corpus_roundtrip`,
`r2013_entity_values`) run in CI on the 19 drawings they are written against, and
verify bit-level round-trip properties for every primitive. They do **not** verify
that every entity decoder succeeds on every real-world drawing; the coverage table
above measures that, on all 52.

## Safety

The whole crate is `#![forbid(unsafe_code)]`. Reed-Solomon, LZ77, GF(256), bit-cursor,
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

New here? [`CONTRIBUTING.md`](./CONTRIBUTING.md) opens with a ten-minute
quickstart — clone, run the exact gate CI runs, pick a
[`good first issue`](https://github.com/DrunkOnJava/dwg-rs/labels/good%20first%20issue)
(each names the spec section, the file, and the test that proves it), PR.
Coverage numbers in this README come from the `Corpus (public DWG)` CI job,
never estimated; `tools/corpus/baseline.tsv` holds every drawing's row.

The project needs help, in rough order of impact:

1. **The pre-R2007 field lists the public corpus fails** — TEXT, MTEXT, ATTDEF, ATTRIB and TOLERANCE on R14 to R2004 (#226), INSERT and every DIMENSION on R14 (#227), and the decoders never routed on R14 to R2007 (#229). Every save but R14 has AutoCAD's DXF of the same drawing, so most expected values exist.
2. **XRECORD bodies** — every XRECORD now closes on its boundary,
   R14 included (#149), but its `(group code, value)` chain is still
   carried as bytes rather than typed pairs.
3. **R14 DIMSTYLE** — §20.4.68 gives R13/R14 their own field order,
   unmeasured.
4. **Fuzz-testing targets** — cargo-fuzz harnesses for LZ77 decompress, bit-cursor, and object walker.
5. **Write-path stages 2–5** — page-map / section-info / system-page / file-open-header rebuild.

Before submitting a PR:

- Run `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- Cite the ODA spec section for any new decoder behavior, numbered as in v5.4.1 ([`docs/spec-coverage.md`](./docs/spec-coverage.md) lists them).
- **Source-provenance declaration**: confirm in the PR body that your contribution does not incorporate executable code (not just comments, not just API shapes) from any source whose license is incompatible with Apache-2.0 — in particular, no Autodesk SDK source, no ODA SDK / Teigha source, and no GPL-licensed DWG implementation source (LibreDWG). Reading algorithm-description comments in permissively-licensed projects (MIT / Apache / BSD) to resolve a spec ambiguity is allowed and should be disclosed in the PR body so we can record it in [`CLEANROOM.md`](./CLEANROOM.md). The PR template has a checkbox for this.
- Follow the [Contributor Covenant 2.1](https://www.contributor-covenant.org/version/2/1/code_of_conduct/) code of conduct.

Security vulnerabilities: report privately via [GitHub Security Advisories](https://github.com/DrunkOnJava/dwg-rs/security/advisories/new) — see [SECURITY.md](./SECURITY.md).

## Legal posture

"Autodesk", "AutoCAD", and "DWG" are trademarks of Autodesk, Inc. This crate is not affiliated with, authorized by, or endorsed by Autodesk.

`dwg-rs` is a spec-based interoperability implementation. The authoritative reference is the Open Design Alliance's freely-redistributable *Open Design Specification for .dwg files* (v5.4.1) — a document distinct from the ODA's Drawings SDK (Teigha) license. Executable code from the Autodesk SDK, the ODA SDK, and GPL-licensed DWG implementations (LibreDWG) was not consulted or imported at any point. One clearly-scoped exception is documented in [`CLEANROOM.md`](./CLEANROOM.md): algorithm-description comments (not executable code) from the MIT-licensed ACadSharp were consulted to resolve one LZ77 offset-encoding spec ambiguity.

Independent reverse engineering for interoperability is generally supported across jurisdictions by authorities such as *Sega v. Accolade* (9th Cir. 1992) and *Sony v. Connectix* (9th Cir. 2000) in the United States, Article 6 of the EU Software Directive (2009/24/EC), and comparable provisions elsewhere. See [`NOTICE`](./NOTICE) for a fuller reference set. Nothing in this repository is offered as legal advice; users with specific legal constraints should consult their own counsel.

## License

Apache-2.0. See [LICENSE](./LICENSE). Contributions land under the same terms per the standard inbound = outbound convention.
