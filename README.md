# dwg-rs

> **Apache-2.0 Rust reader (and early writer) for Autodesk DWG files, R14 to R2018 (AC1014 to AC1032), built only from the Open Design Alliance's published specification.** Pre-alpha. Every pinned public drawing walks completely, every decoded record is checked against its own data-stream boundary, and the DXF output is compared field by field with AutoCAD's own exports.

[![CI](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/ci.yml)
[![Perf](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/perf.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/perf.yml)
[![Doc build](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/docs-rs.yml/badge.svg?branch=main)](https://github.com/DrunkOnJava/dwg-rs/actions/workflows/docs-rs.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Rust MSRV](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)

<!-- Crates.io and docs.rs badges will be added here once the crate is published. -->

---

## Pre-alpha status: read this first

This is **0.1.0-alpha.1**. Do not use it in production. Do not benchmark it against the ODA SDK. Do not tell your CAD team dwg-rs solves their interop problem today.

Entity and object decode coverage, measured in CI by the `Corpus (public DWG)`
job on `main` (<!-- corpus-run:begin -->[run 40573732934](https://github.com/DrunkOnJava/dwg-rs/actions/runs/40573732934) on 2026-10-27<!-- corpus-run:end -->) with
[`examples/coverage_report.rs`](./examples/coverage_report.rs), on the public
drawings fetched by pinned sha256
([`tools/corpus/pins.tsv`](./tools/corpus/pins.tsv); none is committed):

<!-- corpus-table:begin -->
| Version | Files tested | Decoded | Skipped | Errored | Success rate | Median drawing |
|---------|--------------|---------|---------|---------|--------------|----------------|
| R14 (AC1014) | 12 | 4605 | 151 | 0 | **96.8 %** | 98.6 % |
| R2000 (AC1015) | 9 | 2700 | 85 | 0 | **96.9 %** | 98.1 % |
| R2004 (AC1018) | 12 | 106900 | 94 | 0 | **99.9 %** | 98.0 % |
| R2007 (AC1021) | 8 | 2161 | 76 | 0 | **96.6 %** | 97.8 % |
| R2010 (AC1024) | 8 | 2111 | 77 | 0 | **96.5 %** | 97.8 % |
| R2013 (AC1027) | 8 | 1710 | 77 | 0 | **95.7 %** | 97.0 % |
| R2018 (AC1032) | 4 | 1552 | 63 | 0 | **96.1 %** | 97.0 % |
| **Aggregate** | **61** | **121739** | **623** | **0** | **99.5 %** | **97.8 %** |
<!-- corpus-table:end -->

A success rate counts records, and the sources differ in size by orders of
magnitude (four R2004 drawings can hold more records than every other source
together), so pinning a source of large drawings moves it without any decoder
changing. The median drawing weighs every drawing the same, and each source's
own row below stays put when another source is pinned, so these show whether
a change came from decoding or from the corpus:

<!-- corpus-sources:begin -->
| Source | Files tested | Decoded | Skipped | Errored | Success rate | Median drawing |
|--------|--------------|---------|---------|---------|--------------|----------------|
| nextgis | 42 | 8275 | 168 | 0 | **98.0 %** | 98.0 % |
| acadsharp | 10 | 7266 | 365 | 0 | **95.2 %** | 95.0 % |
| bookviewerapp3 | 4 | 1389 | 70 | 0 | **95.2 %** | 97.2 % |
| oshw-jetson | 4 | 104561 | 16 | 0 | **> 99.9 %** | > 99.9 % |
| planar-algorithm | 1 | 248 | 4 | 0 | **98.4 %** | 98.4 % |
| **All sources** | **61** | **121739** | **623** | **0** | **99.5 %** | **97.8 %** |
<!-- corpus-sources:end -->

Totals: <!-- corpus-summary:begin -->121,739 decoded / 623 skipped / **0 errored** / 99.5 % of all records on 61 drawings; the median drawing decodes 97.8 %. Per source (drawings): nextgis 98.0 % (42), acadsharp 95.2 % (10), bookviewerapp3 95.2 % (4), oshw-jetson > 99.9 % (4), planar-algorithm 98.4 % (1). Per version: R14 96.8 %, R2000 96.9 %, R2004 99.9 %, R2007 96.6 %, R2010 96.5 %, R2013 95.7 %, R2018 96.1 %.<!-- corpus-summary:end -->

[`tools/corpus/baseline.tsv`](./tools/corpus/baseline.tsv) pins each
drawing's decoded, skipped and errored counts, its class-census shortfalls
and its reference closure. The job fails on any difference, in either
direction, so a regression is red and an improvement is recorded in the pull
request that makes it. The tables above are rendered from that file by
[`tools/maintainer/readme_status.py`](./tools/maintainer/readme_status.py),
never typed by hand.

The drawings come from [nextgis/dwg_samples](https://github.com/nextgis/dwg_samples)
(seven drawings, each saved by AutoCAD 2016 as R14 to R2013; no licence is
stated, so they are fetched for the run and never shared) and from the
MIT-licensed `samples/` of [DomCR/ACadSharp](https://github.com/DomCR/ACadSharp)
(one drawing saved by AutoCAD 2025 as R14 to R2018, and three more R2018
drawings). Every save of those drawings except R14 has AutoCAD's own DXF
export of the same drawing beside it; of the three extra R2018 drawings only
`geoloc.dwg` has one. [`CLEANROOM.md`](./CLEANROOM.md) records the provenance.

**What "errored" and "skipped" mean.** Every decoder in this crate is
*self-validating*: its field list must end exactly on the record's
data-stream boundary (the string-stream trailer on R2010+, the `RL`
object-data size on R2000 to R2007, the size inside the common entity data
on R13/R14), or it returns an error instead of plausible-looking geometry.
An *errored* record is a field list that does not add up; there are none left
on the pinned corpus. A *skipped* record is a type with no decoder on `main`
yet. Every skipped type has a decoder in review:

- MATERIAL, 212 records (decoder in review: #325);
- the TABLE family: TABLESTYLE on R2010+ and CELLSTYLEMAP (#313), ACAD_TABLE
  (#319), TABLECONTENT (#317) and TABLEGEOMETRY (#316), 94 records;
- the dynamic-block evaluation objects (#328, #326, #339), 133 records;
- DIMASSOC (#329), the ACSH solid-history classes (#330), SHAPE (#310), the
  viewport entity header (#311), PDFUNDERLAY (#331), SUN (#332), DATATABLE
  (#335), ACDB_TEXTOBJECTCONTEXTDATA_CLASS (#337), WIPEOUTVARIABLES, DBCOLOR
  and PDFDEFINITION (#326).

Most of the classes in that list are not printed in the spec at all; their
field lists are learned from AutoCAD's own DWG and DXF output under the
method recorded in [`CLEANROOM.md`](./CLEANROOM.md), and each stays
provisional until a controlled AutoCAD drawing confirms it (#348).

**Values, not just widths.** A boundary that closes proves the field widths
add up, not that the values are right. The DXF value oracle
([`examples/dxf_oracle/`](./examples/dxf_oracle/main.rs)) pairs every decoded
record with the same handle in AutoCAD's DXF export and scores each field. On
`main` it scores <!-- oracle-summary:begin -->133,041 field comparisons on 56 types: 114,396 equal, 11,142 equal to the DXF default, 286 different and 7,217 present only in the DWG<!-- oracle-summary:end -->
([`tools/oracle/baseline.tsv`](./tools/oracle/baseline.tsv) pins the result,
and CI fails when it moves). Decoded types with no scorer yet are listed as
unscored in the oracle summary the Corpus job uploads.

**Not yet in the table:** R13 (AC1012). The reader for it is in review (#323),
pinned on 22 public R13 drawings, none of them written by AutoCAD R13 itself
(#345).

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
| File identification (AC1014 to AC1032) | Shipped | R13 (AC1012) in review, #323 |
| R13 to R15 file header | Shipped | Plain and XOR-encrypted variants |
| R2004 family and R2007 file headers | Shipped | Every release from R2004 to R2018 |
| LZ77 decompression with output caps | Shipped | §4.7 and R2007's §5.10 dialect; 256 MiB default, configurable |
| Page map and section map | Shipped | With a fallback path via `SectionMapStatus` |
| Sec_Mask (R2004 family) | Shipped | R2007 does not use it; see `src/r2007.rs` |
| CRC-8, CRC-32 and record CRCs | Shipped | |
| Reed-Solomon (255,239) | Shipped | §5.13's codes, as AutoCAD writes them; read-side error correction is not wired in |
| Metadata (SummaryInfo, AppInfo, Preview, FileDepList) | Shipped | UTF-16 auto-detect, PNG thumbnail carve |
| Handle map and class map | Shipped | |
| Header variables | Shipped | Table-driven by release range (#126) |
| Object-stream walker | Shipped | Every release from R14 to R2018; every handle-map entry resolves to a record |
| Entity and object decoders | Alpha | 0 errored records on the pinned corpus; every skipped type has a decoder in review (above) |
| Symbol tables | Alpha | Every table record in the pinned corpus decodes, on every release; values scored by the DXF oracle |
| Entity graph (owners, reactors, blocks, layers) | Partial | Resolver APIs exist; reference closure is measured per drawing in CI |
| Streaming reader | Alpha | Byte-demand state machines up to the object records; see below |
| DXF writer | Alpha | HEADER, CLASSES, the symbol tables and ENTITIES (15 kinds) written from decoded records and paired with AutoCAD's exports; BLOCKS and OBJECTS in review (#371 to #373) |
| DXF reader | Alpha | ASCII DXF keyed by handle, used by the value oracle |
| DWG writer | Alpha | Assembles an R2004-family file this crate reads back; no CAD application has been checked against it |
| SVG and PDF export | Alpha | SVG writer and a paged-SVG PDF path; output quality follows the decoded geometry |
| glTF 3D export | Alpha | glTF and GLB writer; surface tessellation is approximate |
| WASM viewer | Partial | Core APIs and 2D viewer pieces exist; selection, progressive and 3D APIs are stubs |
| Python bindings | Pending | A stub module only; see [`docs/python.md`](./docs/python.md) |

## Streaming reader

The [streaming core](docs/streaming/README.md) reads DWG through explicit
byte demands, fixed storage, pausable sources and portable checkpoints. Its
grammar is `no_std` with no allocator, compiled natively and for
`wasm32-unknown-unknown`, and CI enforces that seal
([`tools/streaming/check_invariants.py`](./tools/streaming/check_invariants.py)).
On `main` it covers:

- R13 to R2000: the file header and the whole container walk to every
  mapped record's framing, type and handle;
- the R2004 family: the file header, a bounded resumable LZ77 decoder, the
  page and section maps, the data pages, the whole-file walk and the object
  records, each record's CRC checked (the legacy walker never checked it);
- R2007: its own whole-file walk with every checksum checked before bytes are
  handed out, and its object records.

It does not decode object fields yet; the legacy `DwgFile` reader still loads
whole files and sections. That work, and large-model evidence, remain open
in [#205](https://github.com/DrunkOnJava/dwg-rs/issues/205). Comparisons of
success rate, coverage or memory against other DWG readers are **NOT
MEASURED**; no claim of superiority to another reader is made.

## What works today

The container layer is the most mature part of the crate. `cargo test --release --all-features`
ran 921 tests, all passing, on 2026-10-27 (run it for the current count).

- Version identification from AC1014 (R14, 1997) to AC1032 (2018 and later)
- R13 to R15 file header and the R2004-family XOR-encrypted header
- Page map and section map parsing, LZ77 decompression, Sec_Mask un-masking
- R2007's container: Reed-Solomon de-interleave, its LZ dialect, page and section maps
- `DwgFile::read_section(name)`: decompressed bytes for any named section
- CRC-8, CRC-32 and Reed-Solomon (255,239)
- Metadata parsers: `SummaryInfo`, `AppInfo` (R18 ANSI and R21+ UTF-16
  auto-detected), `Preview` (BMP, WMF, PNG), `FileDepList`
- Handle map, class map and header variables
- R14 to R2004 text in the code page the file header names, for pages 1251
  and 1252, through one shared decoder (#374)
- The object walk on every release from R14 to R2018, and the class census
  CI gate that holds it to what `AcDb:Classes` declares
- Entity and object decoding, boundary-checked on every release, with the
  values scored against AutoCAD's DXF exports
- `dwg-to-dxf`: HEADER, CLASSES, the symbol tables and ENTITIES from the decoded records
- An experimental R2004-family write path: bit writer, LZ77 encoder, section
  framer, Sec_Mask, the CRCs and checksums real files carry, and assembly via
  `DwgFile::to_bytes()`

The LZ77 offset encoding is ambiguous in one place in the spec; this crate's
implementation was cross-checked against the algorithm-description comments in
the MIT-licensed [ACadSharp](https://github.com/DomCR/ACadSharp) source
(comments only, no code). [`CLEANROOM.md`](./CLEANROOM.md) records that and
every later disclosure.

### What dwg-rs does not do today

- Read R13 (AC1012) files; the reader is in review (#323).
- Read R14 to R2004 text in code pages other than 1251 (Cyrillic) and 1252
  (Western European): text in any other page still decodes as lossy UTF-8.
  XRECORD and XDATA strings are not decoded by page yet, and the writers
  still emit UTF-8 (#375).
- Decode password-protected R2007 files: a section whose descriptor declares
  encryption is refused, not decoded.
- Write R13, R14 or R2007 files; the writer covers the R2004 family only.
- Byte-identical DWG round trips, or output checked by any CAD application.
- Python bindings, a non-technical end-user application, an installer or a
  drag-and-drop GUI.

## Install

```bash
# From git (currently the only distribution):
git clone https://github.com/DrunkOnJava/dwg-rs
cd dwg-rs
cargo build --release
```

The 0.1.0-alpha.1 crate has not been published to crates.io yet; publishing
is tracked in [#109](https://github.com/DrunkOnJava/dwg-rs/issues/109) and
[`RELEASE.md`](./RELEASE.md).

## Use the parts that work

```rust
use dwg::DwgFile;

fn main() -> dwg::Result<()> {
    let file = DwgFile::open("drawing.dwg")?;
    println!("version: {}", file.version());
    println!("sections: {}", file.sections().len());

    // Decompressed bytes for any named section.
    if let Some(Ok(bytes)) = file.read_section("AcDb:Preview") {
        println!("preview section: {} bytes", bytes.len());
    }

    // Structured metadata.
    if let Some(Ok(summary)) = file.summary_info() {
        println!("title:  {}", summary.title);
        println!("author: {}", summary.author);
    }

    // Handle-indexed object walk, on every release from R14 on.
    if let Some(Ok(objects)) = file.all_objects() {
        println!("raw objects: {}", objects.len());
    }

    // Entity decode: alpha quality. The DispatchSummary reports per-file
    // coverage; check it before relying on the output.
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
- [`coverage_report.rs`](./examples/coverage_report.rs): run this against your own files to see how much of your data dwg-rs can parse today

## CLI tools

Seven binaries ship behind the `cli` feature flag. Inspection tools work on
any file the container layer can parse; export tools (`dwg-to-*`) work as far
as the decoders for your file's release do (see the
[compatibility matrix](./docs/landing/compatibility.md)).

```bash
cargo run --release --features cli --bin dwg-info -- drawing.dwg

# Inspection
dwg-info drawing.dwg                                        # version and section list
dwg-corpus /path/to/corpus/                                 # sweep a directory
dwg-dump drawing.dwg                                        # hierarchical dump
dwg-convert --extract AcDb:Preview -o preview.bmp x.dwg     # one decompressed section
dwg-convert --verify drawing.dwg                            # decompress every section

# Export (alpha; checked against AutoCAD's DXF, not against a CAD application)
dwg-to-dxf drawing.dwg out.dxf --version R2018              # ASCII DXF (R12 to R2018)
dwg-to-gltf drawing.dwg out.glb                             # glTF 2.0 binary (.glb)
dwg-to-gltf drawing.dwg out.gltf                            # glTF JSON and a sidecar .bin

# Write (experimental; --dwg assembles an R2004-family file)
dwg-write --version R2018 \
  --section AcDb:Header=header.bin \
  --section AcDb:SummaryInfo=summary.bin \
  --report report.json --dwg out.dwg
```

## Architecture

See [`ARCHITECTURE.md`](./ARCHITECTURE.md) for the design deep-dive: format
primer, module responsibilities, the read pipeline, Sec_Mask, the LZ77 spec
errata, and a dated section with its evidence for every format finding.

Quick layer overview:

```
  DwgFile::open ─────────────────────────────────────────┐
         │                                               │
         ▼                                               ▼
   header + section map                       handle map + class map
         │                                               │
         ▼                                               ▼
   read_section("AcDb:*")                       all_objects()  ──► [shipping]
         │                                               │
         ▼                                               ▼
   metadata::* (SummaryInfo,                   decoded_entities() ──► [alpha]
   AppInfo, Preview, FileDepList)                       │
                                                        ▼
                                         dispatch on type code → entities::*,
                                         tables::*, objects::*
                                                        │
                                                        ▼
                                        dxf / svg / gltf exporters, writer
```

R13 to R15 files are the object stream, addressed by a flat locator list; the
R2004 family adds an encrypted header, a page map, Sec_Mask and LZ77; R2007
has its own Reed-Solomon pages, LZ dialect and maps. The three share no
mechanism.

## Verification

```text
$ cargo test --release --all-features                             # 921 tests on 2026-10-27
$ cargo clippy --all-targets --all-features -- -D warnings       # clean
$ cargo fmt --all -- --check                                      # clean
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features   # clean
$ cargo deny check advisories bans licenses sources               # clean
$ python3 tools/ci/spec_coverage.py --check                       # spec matrix agrees with its data
$ python3 tools/maintainer/readme_status.py --check               # this README agrees with the baseline
```

`main` requires twelve CI checks, including `Corpus (public DWG)`, which
fetches the 52 pinned drawings, measures them, runs the probes and the value
oracle, and fails on any change from the committed baselines. The older
real-file suites (`samples`, `block_names`, `corpus_roundtrip`,
`r2013_entity_values`) read a 19-file subset and skip silently when it is
absent, so a green local `cargo test` says nothing about real drawings unless
`python3 -I tools/corpus/fetch.py --legacy-samples ../../samples` has run.

## Safety

The whole crate is `#![forbid(unsafe_code)]`. Reed-Solomon, LZ77, GF(256), the
bit cursor and the bit writer are all safe Rust. Every parser returns
`Result<T, Error>`; malformed input returns an error and never panics.
Claimed counts and allocations are capped (`ParseLimits`, `WalkerLimits`). See
[`SECURITY.md`](./SECURITY.md) and [`THREAT_MODEL.md`](./THREAT_MODEL.md).

## MSRV policy

Rust 1.85 (for `edition = "2024"`). MSRV bumps are minor-version events announced in
the [CHANGELOG](./CHANGELOG.md). CI verifies MSRV on every PR.

## Related projects

| Project | Language | License | Notes |
|---------|----------|---------|-------|
| [ACadSharp](https://github.com/DomCR/ACadSharp) | C# | MIT | Permissive reference; `dwg-rs` cross-checked the LZ77 offset errata against its algorithm comments (not its code). Its MIT `samples/` are part of the CI corpus |
| [LibreDWG](https://www.gnu.org/software/libredwg/) | C | **GPL-3** | The most complete open-source DWG reader; preferable to `dwg-rs` today for any stack that can take GPL-3. Its source was not consulted during this crate's implementation |
| [Teigha / ODA SDK](https://www.opendesign.com/) | C++ | Commercial | Proprietary; paid membership required |
| [dxf-rs](https://github.com/ixmilia/dxf-rs) | Rust | MIT | DXF (the text companion format) only |
| [rvt-rs](https://github.com/DrunkOnJava/rvt-rs) | Rust | Apache-2.0 | Sibling project: an Autodesk Revit (.rvt / .rfa) reader by the same author, with the same source-provenance policy |

## Why this exists

DWG is Autodesk's proprietary format. Autodesk does not publish a specification. What's available to open-source implementers is:

- The **Open Design Alliance's** Open Design Specification, the result of the ODA's own long-standing reverse-engineering effort, made available publicly. `dwg-rs` is built from version 5.4.1 of that spec.
- **LibreDWG** (GPL-3), the most complete open-source DWG reader today. If GPL-3 fits your project, it is almost certainly the better tool.
- **ACadSharp** (MIT, C#), a mature .NET DWG reader for stacks that can take a C# dependency.
- **Teigha / the ODA SDK**, a commercial C++ SDK appropriate for production workloads that can afford the membership.

`dwg-rs` occupies a narrow niche: a permissively licensed (Apache-2.0) Rust crate for reading DWG, implemented from the ODA specification without linking against the ODA SDK and without reusing GPL-licensed source code. It is useful when your stack can't take GPL-3, can't justify an ODA membership, and needs a Rust dependency rather than an FFI binding.

It is **pre-alpha and not a finished DWG reader.** See [`CLEANROOM.md`](./CLEANROOM.md) for the implementation discipline this project follows, including the honest scope of what "clean-room" means for a solo-developer project: a spec-only, no-reference-source posture, not a formal two-team protocol.

## Contributing

New here? [`CONTRIBUTING.md`](./CONTRIBUTING.md) opens with a ten-minute
quickstart: clone, run the exact gate CI runs, pick a
[`good first issue`](https://github.com/DrunkOnJava/dwg-rs/labels/good%20first%20issue)
(each names the spec section, the file and the check that proves it), open a PR.
Coverage numbers in this README come from the `Corpus (public DWG)` CI job and
are never estimated; `tools/corpus/baseline.tsv` holds every drawing's row.

The project needs help, in rough order of impact:

1. **Drawings.** Public pre-R2007 drawings with non-ASCII text in code pages
   other than 1251 and 1252 (#375), an AutoCAD-written R13 drawing (#345), and
   controlled AutoCAD drawings for the classes the spec never prints (#348),
   each with AutoCAD's own DXF export.
2. **Values the oracle cannot score yet.** Types listed as unscored in the
   oracle summary need a scorer before their values count as proven.
3. **Python bindings**, planned in [`docs/python.md`](./docs/python.md).
4. **Fuzzing.** cargo-fuzz harnesses live in `fuzz/`; more targets and longer
   runs are welcome.

Before submitting a PR:

- Run `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test --release --all-features`.
- Cite the ODA spec section for any new decoder behavior, numbered as in v5.4.1 ([`docs/spec-coverage.md`](./docs/spec-coverage.md) lists them).
- **Source-provenance declaration**: confirm in the PR body that your contribution does not incorporate executable code (not just comments, not just API shapes) from any source whose license is incompatible with Apache-2.0: no Autodesk SDK source, no ODA SDK / Teigha source, and no GPL-licensed DWG implementation source (LibreDWG). Reading algorithm-description comments in permissively licensed projects (MIT / Apache / BSD) to resolve a spec ambiguity is allowed and must be disclosed in the PR body so it can be recorded in [`CLEANROOM.md`](./CLEANROOM.md). The PR template has a checkbox for this.
- Follow the [Contributor Covenant 2.1](https://www.contributor-covenant.org/version/2/1/code_of_conduct/) code of conduct.

Security vulnerabilities: report privately via [GitHub Security Advisories](https://github.com/DrunkOnJava/dwg-rs/security/advisories/new); see [SECURITY.md](./SECURITY.md).

## Legal posture

"Autodesk", "AutoCAD", and "DWG" are trademarks of Autodesk, Inc. This crate is not affiliated with, authorized by, or endorsed by Autodesk.

`dwg-rs` is a spec-based interoperability implementation. The authoritative reference is the Open Design Alliance's freely redistributable *Open Design Specification for .dwg files* (v5.4.1), a document distinct from the ODA's Drawings SDK (Teigha) license. Executable code from the Autodesk SDK, the ODA SDK, and GPL-licensed DWG implementations (LibreDWG) was not consulted or imported at any point. The scoped exceptions, algorithm-description comments (never executable code) in permissively licensed projects, are each recorded with a date in [`CLEANROOM.md`](./CLEANROOM.md).

Independent reverse engineering for interoperability is generally supported across jurisdictions by authorities such as *Sega v. Accolade* (9th Cir. 1992) and *Sony v. Connectix* (9th Cir. 2000) in the United States, Article 6 of the EU Software Directive (2009/24/EC), and comparable provisions elsewhere. See [`NOTICE`](./NOTICE) for a fuller reference set. Nothing in this repository is offered as legal advice; users with specific legal constraints should consult their own counsel.

## License

Apache-2.0. See [LICENSE](./LICENSE). Contributions land under the same terms per the standard inbound = outbound convention.
