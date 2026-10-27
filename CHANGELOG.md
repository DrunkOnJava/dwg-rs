# Changelog

All notable changes to `dwg-rs` will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
the project adopts [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once the public API stabilizes at 0.1.0.

## [Unreleased]

### Changed — the merge train builds and tests nothing locally (2026-10-27, #408)

Every build, test and corpus measurement is a GitHub Actions run on the pull request. The train merges main in and pushes, then commits the corpus and oracle baselines and README coverage that the PR's own CI run measured, refusing anything worse than main. A PR stacked on another waits until GitHub retargets it to main.

### Added — the pre-R2007 code pages real drawings prove: 1250, 1253, 1254, 1257 and the double-byte 932, 936, 949 and 950 (2026-10-27, #384, #375)

The file header's short at 0x13 (§3.2.5) is AutoCAD's own enumeration, not a Windows code page number, so each new row is backed by a drawing: the `$DWGCODEPAGE` of AutoCAD's DXF export of the same file, or a known text found in its bytes. Shorts 28, 32, 33, 36, 38, 39, 40 and 41 join 29 and 30. Nine public drawings from three MIT and Apache-2.0 repositories are pinned for it, and `tools/corpus/fetch.py` percent-encodes paths with spaces and Chinese. A short no drawing names still decodes as lossy UTF-8.

### Added — XRECORD values and extended entity data are read, each string in the code page it names (2026-10-27, #380, #375)

`extended_data::read` (a walked record's extended data chain) and `extended_data::xrecord_values` (an XRECORD) turn R13 to R2004 bytes into values, decoding each string in the code page its own page byte names. R2007 and later return `Error::Unsupported`.

### Changed — dwg-to-dxf and the DWG writer write a pre-R2007 drawing's own code page, not UTF-8 or bare escapes (2026-10-27, #382, #375)

`CodePage::encode` turns a character the page holds into its byte (two on a double-byte page) and any other into AutoCAD's `\U+XXXX`. `convert_dwg_to_dxf_bytes` writes R12 to R2004 in that page, `classes::write_tv` encodes through the page in force, and `DwgFile::to_bytes` writes the drawing's page into the header, which used to be 0.

### Added — dwg-to-dxf writes VISUALSTYLE objects (2026-10-27, #413, #66)

The 24 built-in visual styles of every pinned drawing are written in the three layouts AutoCAD's own exports use, and `probe_dxf_objects` pairs each with the export: in the Corpus job, 1,008 written and paired, 61,944 of 62,208 groups equal, 264 that differ only by a stated relation, and none that disagree.

### Added — dwg-to-dxf writes LAYOUT, GROUP and MLINESTYLE objects (2026-10-27, #373, #126)

LAYOUT (its plot settings, then the layout and its handles), GROUP (description, flags and one `340` per member the output holds) and MLINESTYLE (name, flags, description, fill colour, angles and each line's offset, colour and linetype), each paired with AutoCAD's export by `probe_dxf_objects`. A layout with a non-finite coordinate, or a plot view named only by a handle the output lacks, is refused or resolved before it is written.

### Fixed — write_class_map reproduces AutoCAD's AcDb:Classes byte for byte (2026-10-27, #405, closes #45)

The section's size word, end sentinel, the R2004+ zero bytes, the CRC's starting point, pre-R2007 name terminators, padding bits and the long-string trailer now match AutoCAD's. The Corpus job's new step reads every pinned drawing's stored section, parses it and writes it back under the drawing's code page: 61 of 61 identical, R14 to R2018.

### Added — the double-byte code pages round-trip (2026-10-27, #414, #375)

`probe_code_page` decodes every pre-R2007 string and encodes it back, and fails when a cleanly decoded string returns other bytes; it runs over the pinned Shift_JIS and GBK drawings in the Corpus job.

### Added — dwg-to-dxf writes the OBJECTS section (2026-10-27, #372, #126)

The dictionary family (DICTIONARY, ACDBDICTIONARYWDFLT, DICTIONARYVAR), XRECORD, SCALE and ACDBPLACEHOLDER are written in AutoCAD's order with their owners, reactors and extension dictionaries, and `probe_dxf_objects` in the Corpus job pairs every record with AutoCAD's export. An object is written only when its whole owner chain is, so no group 330, 340 or 360 names a record the output lacks; XRECORD application data in groups 100 and 102 is escaped and written in the drawing's code page. The placeholder records the converter used to write for every object are gone.

### Added — dwg-to-dxf writes the BLOCKS section (2026-10-27, #371, #126)

One block per `BLOCK_RECORD`, in the table's order, with its `BLOCK`, its entities and its `ENDBLK`, paired with AutoCAD's export by `probe_dxf_entities --section=BLOCKS` in the Corpus job. INSERTs, ATTRIBs and ATTDEFs are written once the block they name is, and the tables name only blocks the output holds.

### Added — the value oracle scores MULTILEADER (2026-10-27, #385)

90 records, R2000 to R2018, each part of the nested `CONTEXT_DATA{` / `LEADER{` / `LEADER_LINE{` groups scored within its own groups.

### Changed — README.md shows decode coverage per pinned source and for the median drawing (2026-10-27, #404)

The record-weighted aggregate moves when a source of large drawings is pinned, so `readme_status.py` also renders a per-source table and a median-drawing column, and no longer rounds a rate with records left undecoded to 100.0 %.

### Added — ACDBASSOCPERSSUBENTMANAGER and ACDBPERSSUBENTMANAGER decode on every release from R14 to R2018 (2026-10-27, #379, #152)

The last two skipped types that had no decoder in progress. The spec prints neither, so their field lists are learned from AutoCAD's own DWG and DXF output under the 2026-10-26 `CLEANROOM.md` method. ACDBPERSSUBENTMANAGER is a `BL` version and three `BL`-counted `BL` arrays. ACDBASSOCPERSSUBENTMANAGER is a `BL` version, an id array, one array per id, a further array when the version is 2, and a closing `B` (group 290). All 14 records in the pinned corpus close on their data-stream boundary, and the value oracle scores both classes with 0 differences. The corpus moves from 15,527 to 15,541 decoded and from 547 to 533 skipped. Every field rests on one drawing (ACadSharp's `sample_base`), so the layout stays provisional until a controlled AutoCAD drawing confirms it (#348). A version word the corpus does not hold is reported as unhandled, never guessed.

### Added — the value oracle scores SORTENTSTABLE (2026-10-27, #351)

SORTENTSTABLE's decoded fields are compared with AutoCAD's DXF export of the same handle (§20.4.93).

### Changed — README.md and STATUS.md render their measured numbers from the committed baselines (2026-10-27, #376)

`tools/maintainer/readme_status.py` writes the coverage table, the totals and the DXF value oracle's totals from `tools/corpus/baseline.tsv` and `tools/oracle/baseline.tsv`. `--check` fails when a document disagrees with a baseline or has lost a block, and `tools/maintainer/audit.py` runs that check. README.md and RELEASE.md were brought up to `main` in the same change.

### Fixed — text before R2007 is decoded in the code page the header names (2026-10-27, closes #370)

Pre-R2007 text was read as lossy UTF-8 at each reader. It is now decoded in the code page the file header names (§3.2.5) through one shared path, `CodePage::decode` (#374). v5.4.1 prints no table of the code page values, so the table is measured: the header short against the `$DWGCODEPAGE` of AutoCAD's own export of the same drawing (`examples/probe_code_page.rs`). It names two rows, 29 = ANSI_1251 (14 drawings) and 30 = ANSI_1252 (2 drawings); any other number is `CodePage::Unknown(n)`, whose text is read exactly as before, and the other Windows pages are not covered because no pinned drawing gives a number for them (`CLEANROOM.md` has the entry). `RawObject::code_page` is stamped by the walker from `DwgFile::code_page()`, and the shared `TV` readers (the tables, TEXT, ATTRIB, ATTDEF, MTEXT, TOLERANCE, IMAGEDEF, BLOCK and every object through `objects::modern::read_tv`), the class table, the header variables and the metadata sections read it back. `dxf_header`'s `$DWGCODEPAGE` name comes from the same table.

The pinned corpus holds no pre-R2007 string with a byte above 0x7F (0 of 9,076 strings decoded), so the path is proved on real drawings altered in memory: 153 strings in 24 kinds of record of the 16 uncompressed drawings had their first four characters overwritten with `E9 B0 C0 FC` and were read back under the drawing's own header short and under 29, 30 and 31. 612 of 612 reads are what the Windows-1251, Windows-1252 and unknown-page rules say, and 51 of 51 table-entry names reach `dwg-to-dxf` as the matching `\U+XXXX` escapes (they were `\U+FFFD`). The decoded, skipped and errored counts do not move (15,527 / 547 / 0 over the 52 drawings). The writers still emit strings as UTF-8, and XRECORD and extended entity data strings, which carry a code page of their own, are not decoded.

- **Added:** the `code_page` module (`CodePage`, `with_code_page`, `observe`), `RawObject::code_page` and `RawObject::with_code_page`, `DwgFile::code_page`, and a dependency on `encoding_rs` 0.8.35 (MIT or Apache-2.0, with BSD-3-Clause index data), which builds on MSRV 1.85 and for wasm32.

### Changed — the streaming walks run 2.3 to 4.25 times faster, and where their time went is on record (2026-10-27, #205)

The streaming walks were 5 to 18 times slower than the legacy reader, which checks no CRC, and nobody had profiled them. #343 added the harness that measures the gap (`examples/stream_throughput.rs` times a walk against `DwgFile::from_bytes` plus `all_objects` and fails if the two disagree on the record count), and #367 records the profile (Instruments' Time Profiler on release builds) with each finding and the pull request that fixed it. None of the five fixes changes an output byte or a checkpoint:

- **CRC-8** (#360): `crc::crc8`, the DWG CRC of every record, object map chunk and R13 to R2000 section (§2.14.1), folds eight bytes per step with slicing tables derived from the published table at compile time. It was 81.7 % of the samples of the R2000 walk.
- **§4.2 page checksum** (#362): `crc::fold_section_checksum` runs rows of eight bytes as eight independent lanes of the same recurrence and combines them once per slice, with the modulo reduction at the same 0x15B0-byte boundaries, so `(sum1, sum2, in_chunk)` equals the byte-at-a-time loop's after every slice.
- **Record head** (#364): the object framer copies the first bytes of a record, which it keeps to read the type and handle, as one slice instead of one byte per call.
- **R2007 literal copy table** (#365): §5.10.1's copy table is flattened at compile time into one permutation per block size, replacing a recursive walk that was 25.8 % of the R2007 walk.
- **R2007 checks** (#366): the two CRC-64s (§5.12), the §5.4.1 page checksum and the Reed-Solomon syndromes (§5.13) take whole blocks and table lookups instead of one byte at a time.

Best of 40 reads (15 for R2007), in memory, Apple M2 Max under load, before (`a315f04`) and after the five together (#367):

| Release | Before | After | |
|---|---|---|---|
| R14 `sample_AC1014` | 3.27 ms, 401 MB/s | 0.79 ms, 1,661 MB/s | x4.14 |
| R2000 `sample_AC1015` | 3.27 ms, 397 MB/s | 0.77 ms, 1,685 MB/s | x4.25 |
| R2004 `sample_AC1018` | 5.72 ms, 192 MB/s | 2.41 ms, 455 MB/s | x2.37 |
| R2010 `sample_AC1024` | 7.01 ms, 160 MB/s | 2.99 ms, 375 MB/s | x2.34 |
| R2013 `sample_AC1027` | 5.45 ms, 200 MB/s | 2.34 ms, 467 MB/s | x2.33 |
| R2018 `sample_AC1032` | 5.44 ms, 199 MB/s | 2.34 ms, 462 MB/s | x2.32 |
| R2007 `sample_AC1021` | 20.24 ms, 53 MB/s | 7.95 ms, 134 MB/s | x2.55 |

What is left, from the R2010 profile of the five together: the LZ77 decoder is 45 % of the samples, about 0.8 ns per output byte, so it is not the gap; the record CRC (17 %) and the page checksum (about 9 %) are checks the legacy reader does not make. Two attempts were measured and dropped: slice copies in the LZ77 decoder (1 to 2 %, within noise) and a weighted-sum form of the page checksum (0.63 times as fast as the loop it replaces). Before the fixes (#343, best of 40, machine loaded) the walk was 5 to 6 times slower than the legacy decode on the R2004 family and 16 to 18 times slower than the legacy bulk copy on R14 and R2000, while it verifies what the legacy reader does not.

- **Added:** `crc::fold_section_checksum`; `stream_throughput --streaming-only` (#367), which skips the legacy reader so a sampling profiler sees only the walk.

### Added (breaking) — an R2007 file is streamed, from its header to its object records (2026-10-27, #205)

`streaming::R2007Walker` walks a whole R2007 (AC1021) file in one fixed-size, resumable, `no_std` walker: the file header, the page map, the section map and every data page of every section, with every Reed-Solomon codeword, CRC-64 and page checksum (§5.1 to §5.4, §5.10 to §5.13) checked before any byte of a page is handed out, which the legacy reader never does (#340). The section map is the outer loop (R2007 data pages have no header), so pages come out in section order; a page is never held, and a `Gather` demand names the 255 strided bytes of one codeword. The `DWGR` checkpoint is self-contained and restores in the wasm32 kernel. On 11 files (the 8 pinned R2007 drawings and 3 files `tools/streaming/r2007_ref.py` builds from the spec: 113 sections, 152 pages, 4,966 data codewords, 3,067,696 bytes) every output line and byte equals an independent Python parse and `r2007::Container`'s. One finding: data pages are not bounded by the section's 0xF800 maximum, `sample_AC1021.dwg` has a page of 70,912 bytes and 273 codewords (ARCHITECTURE.md §7aa).

`streaming::R2007ObjectWalker` frames `AcDb:AcDbObjects` of an R2007 file into records in stream order with no handle map or index held, as the R2004-family walker below does (#363). An R2007 record is R2000's (`MS` size, a payload that opens with the `BS` type, the `RL` size and the handle, then a CRC; no `MC`), so the framer is shared. On the eight pinned R2007 drawings its 2,237 records tile their sections exactly, every record CRC and every (handle, offset) of `AcDb:Handles` agrees, and 34 records straddle a page boundary (the largest takes 150,151 bytes); read as R2010 records, the same bytes fail the first record's CRC in all 8 (ARCHITECTURE.md §7al). A stretch of the section that no page covers arrives as a hole and ends the walk with `ObjectsOrder` instead of being framed as zeros. The R2007 string and handle streams are bytes to the framer. Not measured: a drawing over 1.1 MB, a stored header written by AutoCAD, a non-zero CRC seed.

- **Added:** `streaming::{R2007Walker, R2007Checkpoint, R2007ObjectWalker, R2007ObjectCheckpoint}` with their demand and step types, `byte_source::{poll_r2007_walk, poll_r2007_objects, gather_by_reads}`, and the `StreamError` variants `R2007Header`, `R2007Parity`, `R2007Crc64`, `R2007Checksum`, `R2007Page` and `R2007PageMissing` (a breaking change for an exhaustive `match` on the enum, as in #333).

### Added (breaking) — dwg-to-dxf writes the HEADER, TABLES, ENTITIES and CLASSES sections from the decoded drawing (2026-10-27, #126)

`dwg-to-dxf` wrote a minimal header, a single default layer and the two space-block placeholders around entities converted from their geometry. It now writes what the decoders read, section by section, under AutoCAD's own group codes and in AutoCAD's order as measured on the DXF exports of the public corpus. Every section has a probe, wired into the Corpus job, that pairs each written record with the export's record of the same handle and compares it group by group; on the corpus every written record pairs with the export's, and none disagrees.

- **HEADER** (#320): the decoded `AcDb:Header` (#302) under AutoCAD's `$NAMES`. A variable AutoCAD's export omits is omitted, and nothing is written with a default; R12 and R14 targets keep `$ACADVER` alone, because the corpus holds no AutoCAD export of either to measure against. `probe_dxf_header` over the 42 drawings that have an export: 9,459 variables written, 9,286 agree, 173 differ by a stated rule (the twin was saved later), 0 disagree, 0 extra. One finding: from R2007 the `AcDb:Template` description length counts UTF-16 units, where §22 counts bytes.
- **Symbol tables** (#347): VPORT, LTYPE, LAYER, STYLE, UCS, APPID, DIMSTYLE and BLOCK_RECORD, each record under its own handle and owner. `probe_dxf_tables`: 1,209 records written and paired, 8,434 groups, 8,425 agree, 9 differ by a stated rule, 0 disagree, 0 extra. UCS is written as AutoCAD's empty table when the drawing has none and left out when it has one, because no corpus drawing holds a UCS record. With the tables written, the header names `$DIMSTYLE` and `$DIMBLK`.
- **VIEW** (#357), with its associated UCS: all 6 corpus views pair, group for group and in AutoCAD's order.
- **ENTITIES** (#358): the model-space and paper-space entities of 15 kinds, LINE, POINT, CIRCLE, ARC, ELLIPSE, SOLID, RAY, XLINE, 3DFACE, TEXT, LWPOLYLINE, MTEXT (before R2018: no background fill, columns or embedded-object form), POLYLINE with VERTEX, and SEQEND. `probe_dxf_entities`: 732 written, 732 paired, 5,099 groups, 5,099 agree, 0 disagree, 0 extra. An INSERT with its ATTRIBs is skipped, and counted, until BLOCKS writes the block it names; DIMENSION, MULTILEADER, HATCH and the other kinds are not written yet and the probe counts them by kind.
- **CLASSES** (#368), from `AcDb:Classes`: the DXF name, C++ name, application, proxy flags, from R2004 the instance count, the was-a-proxy flag and the entity flag, for an R2000 or later target. Three differences between AutoCAD's export and the class table were measured and applied: from R2004 the export does not list LAYOUT or ACDBPLACEHOLDER, an R2000 export names the `AcDbDataTable` class `ACDBDATATABLE` where the table and every later export say `DATATABLE`, and the flags are unsigned (`0x8401` is 33793, not -31743). `probe_dxf_classes`: 0 disagree; the 67 relations are all the instance count (group 91), which the export takes from the objects it holds after load.

What the writer needed from the decoders, kept where they were read and dropped: the entity colour's two optional `BL` words, true colour and transparency, which `read_common_entity_data` read in the wrong order and discarded; they are now read RGB word first as §2.11 lists them and kept as `CommonEntityData::{color_raw, color_rgb, color_alpha}` (#359; the two words are 32 bits each, so the cursor ended in the same place and no decoded count moved), the linetype scale and flags, `Nolinks` and the visual style flags (#358), a table entry's reactors, extension dictionary and own handles (#347), and a view's associated UCS and R2007 camera-plottable flag (#357).

- **Added:** `dxf_header`, `dxf_tables`, `dxf_entities`, `dxf_classes`, `entity_refs` and `tables::handles`; `CommonEntityData::{nolinks, ltype_scale, ltype_flags, visual_style_flags}` and the three colour fields; `CommonObjectData::no_xdictionary`; `LtypeEntry::text_area`; `ViewEntry::{ucs, camera_plottable}`; `BlockRecord::{insert_count, preview, insert_units, explodable, block_scaling}`; `VportEntry::{circle_zoom, fast_zoom, ucs_icon, snap_on, grid_on, snap_style, snap_isopair, ucs, lighting}`; `dimstyle::decode_with_vars`, which returns every dimension variable of the record.
- **Changed:** `dxf_sections::HeaderValue` gains `Groups(Vec<HeaderGroup>)` for a variable whose group codes the other variants cannot say (`$CLAYER` is group 8, `$HANDSEED` group 5), a breaking change for an exhaustive `match`; `dxf_header::header_entries` takes the defined symbol tables (`&Defined`) instead of a list of layer names.

### Changed — maintainer tooling, documentation and dependencies (2026-10-27)

No library behaviour changes in this entry.

- **CI gates.** Perf benches the PR's merge-base and the PR in one job on one host, instead of comparing against a baseline cached from another runner, which had measured hardware and flagged PRs that touched no benchmarked code (#217). A Spec coverage job runs `tools/ci/spec_coverage.py`, so `docs/spec-coverage.md` keeps one row for every section of ODA spec v5.4.1 and never drifts from `docs/data/spec-coverage.tsv` (#219). The PII guard also refuses merge-conflict markers (#309), and the Spec coverage job refuses ARCHITECTURE.md sections that repeat or skip a letter (#334). The nightly fuzz run covers its whole budget and fails on a crash (#232). Every cargo workflow sets `CARGO_NET_RETRY=10` (#268), and the wasm-pack install retries up to five times on a transient download failure (#342). `main` requires Corpus and Spec coverage (#292); docs/maintaining.md describes the `main-protection` ruleset, with no bypass, as it is (#220). `tools/maintainer/clean_caches.py`, which `audit.py` already pointed to, removes the Actions caches of closed pull requests and the superseded ones per key family (#361), the cache having stood at 9.95 GB of its 10 GB.
- **Merge train.** `tools/maintainer/train/train.sh` lands queued decoder PRs one at a time, regenerating the generated files (`tools/corpus/baseline.tsv`, `docs/spec-coverage.md`, the spec-example fixtures) and renumbering colliding ARCHITECTURE.md sections instead of leaving every open PR stale after each merge (#298). It ratchets the DXF oracle baseline as well (#300), defers a PR whose build is refused a governor lease instead of dropping it from the queue (#304), resolves both-sides appends to CLEANROOM.md (#327), never leaves auto-merge on a PR it stops landing (#336), resets its worktree before each checkout (#338), ignores oracle rows the PR itself renamed (#344) and, after main carried three `7n` sections, numbers ARCHITECTURE.md's newer sections in landing order (#322).
- **Documentation.** CLAUDE.md for coding agents and contributor docs brought up to date, including the decoder registration steps and the rule that evidence is real-file, not unit tests (#218, #257); a 10-minute contributor quickstart at the top of CONTRIBUTING.md (#18); a CLEANROOM.md entry that a class layout the spec never prints is derived from AutoCAD's own DWG and its DXF export, accepted only when it closes every record's boundary on every release and every decoded value equals the twin's group (#321); AGENTS.md gives a reviewer the clean-room rule (#369).
- **Fixtures and benchmarks.** `examples/build_fixtures` writes the four canonical R2004-family fixtures, five entities each, byte-deterministically, so the coverage-smoke job has a corpus to run against instead of skipping, with a 95 % floor on the decoded ratio and a noise floor for the Criterion job (#19).
- **Dependencies.** thiserror 1.0.69 to 2.0.20 (#3) and 2.0.21 (#93), criterion 0.5.1 to 0.7.0 (#4), the cargo patch group (anyhow, clap, serde, serde_json) in #11 and #93, libfuzzer-sys 0.4.13 in `/fuzz` (#200), js-sys and serde in `/wasm` (#201); the Actions `upload-artifact` 7.0.1 (#5), `configure-pages` 6.0.0 (#6), `download-artifact` 8.0.1 (#7) and 8.0.2 (#202), `cache/save` 6.1.0 (#85), `softprops/action-gh-release` 3.0.3 (#86), `gitleaks-action` (#87, #92 to 3.0.0), `deploy-pages` 5.0.1 (#90), `checkout` 7.0.1 (#91) and `upload-pages-artifact` 5.0.0 (#203).
- **Direct commits before the PRs, 2026-04-20 to 2026-04-25.** Pre-alpha claims toned down across the public surfaces and the uniqueness framing removed (58753a1, 566711b, 7ec595b), HATCH path coverage documented (fc3ee5c), rustdoc intra-doc links fixed and two let-chains in `svg.rs` split so the crate compiles on the 1.85 MSRV (de37ead), wasm-opt run with bulk-memory enabled so `wasm-pack build` works (b0f49b9), and the wasm attestation (V-19) scoped to the Import section with Pages auto-enabled (be3a71d).

### Added (breaking) — the DXF value oracle: every decoded value is compared with AutoCAD's own export (2026-10-26, #99, #100, #101, #102, #103)

A record that closes on its data-stream boundary proves that the widths add up, not that the values are right. AutoCAD's own DXF export of the same drawing says which values are right, and the oracle compares against it.

- **Reader** (#223): `dxf_reader`, behind the new default-off `dxf-reader` feature, parses ASCII DXF, keeps every group of every record in file order, types each value by its group code and indexes records by handle. It is self-validating and capped (`DxfLimits`); library consumers never compile it, and `--all-features` CI builds, lints and documents it.
- **Oracle** (#248): `examples/dxf_oracle` pairs every DWG record of the corpus with the record of the same handle in the export (#101) and scores the decoded fields of 44 spec sections against the DXF group that carries each (#102): equal, default (the export omitted a default-valued group), different, dwg-only or not-decoded, never counting a one-sided field as equal. Reals agree within half a unit of AutoCAD's printing plus 4 ulps; everything else is exact. `--tsv`, `--summary` and `--baseline` (the #103 ratchet: it fails when a row's agreeing count falls or a baseline row is absent). `tools/oracle/spec_notes.py` writes each measured section's note into `docs/data/spec-coverage.tsv`.
- **CI** (#277): the Corpus job builds the oracle and runs it over the fetched drawings against `tools/oracle/baseline.tsv` (2,723 rows over the 49 drawing pairs); the Measure workflow reports its drops against the base.
- **More scorers**: MLEADERSTYLE (§20.4.87; 49 records, 1,914 values equal, 36 default, 0 different), VIEW (§20.4.60; 6 records, 120 equal, 0 different) and VISUALSTYLE (1,008 records, 58,104 equal, 264 different: `format_version` on R2013 and R2018, DWG 2 against export 3, and Realistic's face lighting quality on R2000 to R2007, filed as #250) in #279; TABLESTYLE (§20.4.101.1; 24 records, 1,896 values, all equal) in #349; IMAGEDEF_REACTOR (6 records, all equal) and RASTERVARIABLES (6 records, 24 values equal) in #350.

- **Added:** the `dxf-reader` feature and the `dxf_reader` module (`DxfDocument`, `DxfRecord`, `DxfGroup`, `DxfValue`, `DxfLimits`, `DxfError`), `examples/dxf_oracle`, `tools/oracle/`.
- **Changed:** `DxfValue` gains `CodePageText(Vec<u8>)` (#248): text before AC1021 whose bytes leave 7-bit ASCII is kept as bytes instead of refused, because its code page is `$DWGCODEPAGE` or undeclared (a breaking change for an exhaustive `match`, on an enum #223 introduced in the same window).

### Added — CI reads 52 public drawings (102 files) fetched by pinned hash, and the README quotes what it measures (2026-10-26, closes #96, closes #154)

The real-file test suites skipped silently when `../../samples` was absent, so a green CI said nothing about real drawings. The `Corpus (public DWG)` job (#225) fetches 102 public files by pinned sha256, 42 DWG and 42 DXF exports of `nextgis/dwg_samples` at `ff05d5a8` (AutoCAD 2016, no licence stated) and 10 DWG and 8 DXF of `DomCR/ACadSharp` `samples/` at `c30a97ce` (MIT), runs the five real-file test suites and fails if one prints `skipping`. It ratchets every drawing's decoded, skipped and errored counts, walk skips, class-census shortfalls and reference closure against `tools/corpus/baseline.tsv` and fails on any difference in either direction, so a PR that moves a number commits the regenerated baseline. `tools/corpus/fetch.py` keeps a file only if its size and sha256 match; `measure.yml` compares any ref against a base on the same corpus. Nothing is committed: nextgis declares no licence (`CLEANROOM.md` has the entry).

The published numbers follow the job (#237). README, STATUS, ROADMAP, `docs/release-1.0-criteria.md` and `docs/spec-coverage.md` quote its run on `main`: 52 drawings, 13,781 decoded, 1,985 skipped, 308 errored, 85.7 %, in place of the 2026-08-30 local measurement of 19 drawings (3,911 / 525 / 9 / 88.0 %, which the same run still measures on that subset). On the evidence of the run, 21 spec-coverage rows move from `closes` to `partial` and GEODATA from `decoder` to `partial`, each naming its failing records and issue: the page counts 65 `closes` (was 86), 33 `partial` (11), 6 `decoder` (7) and 34 `missing`.

- **Fixed:** `coverage_report`, `probe_data_page_header`, `probe_acis_records`, `dwg-corpus` and `probe_material` matched only a lower-case `dwg` extension and skipped `X.DWG` without a word (26 of the 122 files of an R13 corpus end in `.DWG`; `coverage_report` measured 222,143 of their 345,000 records) (#353); `probe_data_page_header` with an argument that holds no drawing measured the default set and exited 0, and now exits 1 (#354).

### Added (breaking) — the streaming reader: bounded, resumable walks of the R13 to R2004-family containers (2026-10-26, #205, #206, #207, #210)

The legacy reader loads whole files and sections. `streaming` and `byte_source` are a second reader whose grammar holds fixed storage, allocates nothing, owns no transport and compiles with no `std` and no allocator, natively and for `wasm32-unknown-unknown` (`tools/streaming/check_invariants.py`, docs/streaming/README.md). A step says how many bytes it needs next; every check passes before a byte is committed; and a checkpoint taken at any step restores in a fresh process, or in a wasm32 kernel, against a caller-supplied source identity. The Streaming E2E job, a required check on `main` since #213, runs it on real drawings fetched by hash.

- **R13 to R2000 file header** (#213): `FlatHeaderParser` reads the §3.2 header with strict CRC and sentinel checks and explicit unsupported-layout errors; `ByteSource` with `FileSource` and `SliceSource`, `SourceIdentity` and `Checkpoint`. The E2E checks 12 synthetic version and count combinations, 1,002 truncations and two hash-pinned real headers.
- **R13 to R2000 container** (#240): `FlatWalker` continues through the §9 header-variable section, the §10.1 class section and the §23.1 object map, following every map entry to its §20.1 record and checking framing, CRC, type and handle (`WalkCheckpoint`). On the canonical R2000 fixture and the R14 and R2000 files of the pinned nextgis set, 3,546 records equal an independent Python framing parse and the legacy reader's handle-map walk.
- **LZ77** (#246): `Lz77Decoder` decodes one §4.7 page from any prefix of its bytes into an output slice of any size, pauses between any two calls and checkpoints mid-page, in a fixed window of `0xC000` bytes.
- **Asynchronous sources** (#255): `byte_source::AsyncSource` serves a host that answers later (a worker, an HTTP range fetcher, an event loop) through the same `ByteSource` contract, without the library owning a thread, runtime or connection.
- **R2004-family file header** (#260): `R2004HeaderParser` verifies the §4.1 header of AC1018, AC1024, AC1027 and AC1032.
- **Page map and section map** (#276): `R2004MapWalker` walks §4.4 and §4.5, verifying the checksum of each system page before deriving anything from it.
- **Data pages** (#299): `R2004SectionWalker` assembles a section from its §4.6 data pages: each page's masked header and both checksums are checked, and the stored bytes go through `Lz77Decoder` or are copied for a stored section.
- **Whole file** (#312): `R2004Walker` walks the header, the maps and every data page in page-map order behind one self-contained checkpoint (`R2004WalkCheckpoint`), with a section table of at most 64 slots (`MAX_SECTIONS`) and each section's pages accounted against its section map by count and digest. The records of its object section are framed by the R2004 object walker described below.
- **wasm32** (#301): a `no_std` wasm32 kernel built from the same grammar files restores a checkpoint made by a native build and finishes the walk, and the reverse; `tools/streaming/verify_wasm.py` joins the Streaming E2E job.

- **Added:** `streaming::{FlatHeaderParser, FlatWalker, Lz77Decoder, R2004HeaderParser, R2004MapWalker, R2004SectionWalker, R2004Walker}` with their checkpoints, demand and step types, `byte_source::{ByteSource, FileSource, SliceSource, AsyncSource}` and the `poll_*` pumps.
- **Changed:** `StreamError` gains variants in each of #240, #246, #260, #276, #299 and #312 (a breaking change for an exhaustive `match` on the enum, as in #333).

### Added (breaking) — every AcDb:Header variable is decoded from one table (2026-10-26, #126)

`header_table` lists every slot of `AcDb:Header` (§9) in stream order, with its bit code, its first and last release and AutoCAD's variable name (`UNKNOWN_<code>_<n>` where none is known), and `HeaderVars::decode` reads them from R14 to R2018, with the R2007+ string and handle streams, into typed `Value`s (#302). The frame's sentinels, size and CRC are checked, the section is bounded by the new `ParseLimits::max_header_bytes`, every length is checked before use and every failure is an `Error`. `HeaderVars::{get, get_f64, get_i64, get_text, get_handle, entries, unknowns}` and `DwgFile::header_vars_strict` read the result.

`probe_header_vars` over the 52 pinned drawings, R14 to R2018: every drawing closes, 11,544 slots agree with AutoCAD's DXF export, 316 differ by a stated rule (the twin was saved later, a per-save GUID, case-insensitive names and the like, listed in ARCHITECTURE.md), 0 disagree, and the padding after the last slot is all ones on 52 of 52. §9 differs from the files in several places, recorded in ARCHITECTURE.md: MENUNAME is read on every release (§9 says R13 to R18), four unknown `BS` end the section on every release from R14 and five `B` follow them from R2004, and from R2000 the fields §9 calls TDCREATE and TDUPDATE hold universal time (named `$TDUCREATE` and `$TDUUPDATE`). The spec's own R14 header dump now decodes in full (199 slots) in `spec_examples`. R13 has no file in the corpus, so its rows are the table's and unmeasured. `fuzz/fuzz_targets/header_vars.rs` wraps its input in a valid frame so it reaches the slot decoder; 47,191 runs over seeds of the 52 real sections found nothing.

- **Changed:** `HeaderVars::size_in_bits` was a `u32` that held the section's size word, which §9 gives in bytes; that is now `HeaderVars::size`, and `size_in_bits` is an `Option<u32>`, present from R2007. `HeaderVars` has a private field now, so a struct literal no longer builds one.
- **Added:** `header_table`, `HeaderVars::decode`, the `Value`, `Entry` and `Color` types of `header_vars`, and `ParseLimits::max_header_bytes` (a `ParseLimits` struct literal without `..Default::default()` stops compiling).

### Fixed (breaking) — entity field lists are read as the spec prints them, on every release (2026-10-26, closes #71, closes #226, closes #227, #228, #229)

At #237 the public corpus held 308 errored records, and each errored type was filed under #71, #226, #227 or #228. Each fix below was found against the corpus and the spec's own worked examples, and its values equal the AutoCAD DXF twin wherever the drawing has one.

- **LEADER** (§20.4.47, #249): the decoder read four `3BD`s after the points where the section has five, then stopped, and called the origin the end projection. With the fifth (the R14+ end point projection) and the R13 to R14 and R2000+ branches read, all 7 LEADER records close at delta 0, R14 to R2018. One measured departure from the spec: from R2010 the record does not write the box height and width.
- **VIEWPORT** (§20.4.38, #247): only the first three fields were read, so every R2000+ record reported delta -819. The whole record is read with every version branch; all 51 VIEWPORT records close, R14 to R2018, and on the 36 that have a DXF export every value equals it.
- **MESH** (#252): the trailing field is a `BL` (the export names it: a group 90 equal to 0 after the creases), and MESH decodes before R2010. All 14 MESH records close; the 12 with a twin equal it group by group.
- **TEXT, ATTRIB, ATTDEF** (§20.4.3 to §20.4.5, #258, closes #62): the R2000 to R2004 reader had the data flags inverted (the spec says "present if !(DataFlags & bit)"), R13 and R14 had no branch of their own, oblique, rotation and width factor were `BD` where the spec has `RD`, and the ATTDEF prompt was read before the tag. All 118 erroring text records close (114 on R14 to R2004, 4 on R2018) and every value agrees with the twin.
- **MTEXT** (§20.4.46, #261): line spacing is R2000+, and R2004+ adds the background flags and block. The 72 MTEXT records that errored on ACadSharp's R14 and R2004 saves close, and all 257 MTEXT records of the public corpus close and agree with their twins, backgrounds included.
- **TOLERANCE** (§20.4.49, #262): the unknown short, height and dimgap are R13 to R14 only, and the DIMSTYLE handle is not a data-stream field. The 9 TOLERANCE records that errored on ACadSharp's R14, R2000 and R2004 saves close, all 21 close, and on R2000 to R2018 their values equal the twin.
- **INSERT, the dimensions and 3DFACE on R14** (§20.4.9, §20.4.22, §20.4.32, #267): the R13 to R14 branches the section prints. All 24 R14 records that errored close (12 INSERT, the 11 dimensions of all seven types, 1 3DFACE) and each equals the R2000 save of the same drawing, apart from the four dimension fields R14 does not store, which read 0, and one last-digit difference in a stored double.
- **LWPOLYLINE and HATCH on R14** (§20.4.85, §20.4.75, #264): the R14 class names are routed to their decoders, and LWPOLYLINE's R13 to R14 vertex branch, where every vertex is a `2RD`, is read. All 24 R14 LWPOLYLINE and 8 R14 HATCH records decode and close, equal to the R2000 save of the same drawing.
- **MULTILEADER on R14 to R2007** (§20.4.48, §20.4.86, #278): the `-R2007` branches, the pre-R2004 `CMC` and the inline `TV`s. All 60 R14 to R2007 multileaders were skipped and now close, every value compared equals the export, and the R2010 to R2018 records the crate already decoded show 0 differences.
- **ACIS version 2** (§20.4.41, #283): a version-2 binary (SAB) envelope is read byte by byte to its fixed terminator, because the spec gives no length. The two 3DSOLIDs and one REGION of each of ACadSharp's R2004, R2007 and R2010 saves errored; all 9 close. A version-2 text (SAT) file is refused with `Unsupported`.
- **HATCH spline edge** (§20.4.75, #294): the start and end tangents are stored only when the edge has fit points. The one spline edge of the corpus has none, and HATCH `0xAA8` of `export_sample.dwg` closes: no release has an errored HATCH record now.
- **VERTEX (2D)** (§20.4.11, closes #272, #287): the tangent is stored on every vertex, not only when flag `0x02` is set, and no end width follows a negative start width.
- **POLYLINE (MESH)** (§20.4.34, closes #273, #288): the six `BS` are read in the spec's order (flags, curve type, M and N vertex counts, M and N densities) plus the R2004+ owned-object count; the decoder read five, without the curve type and with the densities before the counts.

- **Changed:** `entities::leader::decode`, `entities::three_d_face::decode` and `entities::polygon_mesh::decode` take the `Version`; `entities::viewport::decode` takes the `Version` and the string reader.
- **Removed:** `Leader::end_projection` and `Leader::horiz_direction` (the record's fields are `origin`, `x_direction` and `end_point_projection` now); `entities::viewport::ViewportSkipped`.
- **Added:** `Viewport::view` (`ViewportView`, `ViewportLighting`), `Leader` fields for the R13 to R14 and R2000+ tails (`dimgap`, `box_height`, `box_width`, `hookline_on_x_dir`, `arrowhead_on`, `r14`, `unknown_r2000`), `MText::{background_flags, background}`, `MLeader::legacy` (`MLeaderPreR2010`) and `PolygonMesh::{curve_type, num_owned_objects}`.

### Fixed (breaking) — symbol-table entries are read as the spec and AutoCAD's export give them (2026-10-26, closes #274, closes #74, closes #40, closes #35)

- **Entry header** (§20.4.54 and the other table sections, #235): `TableEntryHeader` called the first flag bit `is_xref_dependent` and the last `is_xref_resolved`, so on R2000 and R2004 `is_xref_dependent` held the 64-flag, and the R2007+ decoders stored the same two bits the other way round. The fields now carry the spec's names on every release, `flag_64` and `is_xref_dependent` (the Xdep bit), and the R2007+ reader orders the pair by release as AutoCAD's exports show. The export leaves bit 16 of group 70 clear on every table entry of the corpus and never writes bit 64.
- **Reactor count** (#293, closes #40, closes #35): the R2007+ path skipped the `BL` reactor count of the entry's common data and compensated with a flag order that changed by release. Entries without reactors read the same bits and closed, but with the xdictionary bit in their 64-flag; entries with reactors failed, the six R2007 and R2010 DIMSTYLE errors. They close now, and every value the new probe compares equals the export.
- **VPORT** (§20.4.64, #238): the stored second `BD` is the aspect ratio times the view height, so it is `view_width` now, and `aspect_ratio()` divides it by the height; `view_mode` is the group-71 value assembled from the four stored bits and `UCSFOLLOW`.
- **VIEW** (§20.4.60, #263, #284): the pre-R2007 record was read with the centre as two `BD`s, the view mode as a `BS` and no associated UCS block; ACadSharp's R14, R2000 and R2004 VIEW records now close and every surfaced field equals the export. The view mode is surfaced as group 71, as VPORT's is.
- **UCS on R13 and R14** (§20.4.62, closes #274, #289): the record ends after its Y axis, because the elevation and the orthographic fields are R2000+. The decoder read them on R14 and failed on the handle bytes. The corpus holds no UCS record; the spec's own example, §20.4.62.1, closes.
- **DIMSTYLE on R14** (§20.4.68, closes #74, #297): the "R13 & R14 Only" block the section prints was never measured, so every R14 DIMSTYLE was unhandled. All 20 R14 DIMSTYLE records close on their `RL` boundary and equal the R2000 saves of the same drawings; with #293, no table entry of the 52-file corpus is skipped or errors.

- **Changed:** `TableEntryHeader::is_xref_resolved` is gone, `flag_64` is new and `is_xref_dependent` holds the Xdep bit; `VportEntry::aspect_ratio`, a field, is `view_width`, and `aspect_ratio()` is a method that returns `Option<f64>`; the `view_mode` of VPORT and VIEW is the DXF group 71 value.
- **Added:** `tables::dimstyle::decode_r13_r14`.

### Fixed (breaking) — object decoders read LAYOUT, VISUALSTYLE, MLEADERSTYLE and GEODATA as the export has them (2026-10-26, refs #229)

- **LAYOUT** (§20.4.84, #236): there is no "R2013+ data-store block" after the `has AcDs binary data` bit; the 16 bits `objects::acad_layout` read there are the first 16 of `plot_layout_flags` in its full `BS` form. Both readings closed on the data-stream boundary, because a `BS` of 256 or more is `00` and 16 bits, exactly the old block plus a `10` (a `BS` of 0); only the export could tell them apart, and `plot_layout_flags` now equals it on all nine records the oracle pairs (11952 for `Model`, 688 for the paper layouts). #67's `AcadLayout::ds_block` is gone.
- **LAYOUT on R14, R2000 and R2007** (§20.4.84, #270): the R13 to R2000 plot view name, the R2004+ shade plot mode, resolution level and custom DPI, and the viewport count are read, the R14 and R2000 class number is routed, and the stale R2007 refusal is dropped. The 75 LAYOUT records of the public corpus that were skipped close, each equal to the R2004 save of the same drawing.
- **VISUALSTYLE** (#251): the 30 R2013+ tail properties have the types AutoCAD's export lists for them (slot 10 a real, 24 a real, 25 an integer, 26 a string, `strokes_ogs.tif`, which the decoder kept as an unplaced trailing string); every tail value and flag then equals the export on all 240 R2013 and R2018 records. The boundary cannot choose: a `BS` 0 and a `BD` 0.0 are the same two bits.
- **MLEADERSTYLE** (§20.4.87, #253): `is_annotative` (296) is stored before `overwrite_property_value` (295), a departure from the section, which lists them the other way round; both orders consume the same two bits, and on the 14 of 49 records where the two differ the decoded values were swapped. `CLEANROOM.md` gets a dated correction, since §20.4.87 is a prescription after all.
- **GEODATA** (§20.4.78, #295): the crate had an entity decoder with an invented field list, keyed on fixed code `0x4FB`, so the corpus's one GEODATA record, which carries a class number, was never routed. It is now routed by its DXF class name to a decoder of the §20.4.78 object, which closes, and all 77 values compared equal the export. One departure from the spec: the north direction is a `2RD`, not a `3RD`.

- **Changed:** `VisualStyleValue` gains `Text(String)`, a breaking change for an exhaustive `match`.
- **Removed:** `AcadLayout::ds_block`; `GeoData`'s fields `version`, `block_handle`, `origin`, `design_to_ref_x_axis`, `design_to_ref_y_axis`, `reference_north_dir`, `design_to_ref_x_scale`, `design_to_ref_y_scale` and `coordinate_system_def`, and `entities::geodata::decode_modern_split_stream` (the struct now carries `object_version`, the design and reference data of §20.4.78 and `coordinate_system_definition`).
- **Added:** `AcadPlotSettings::plot_view_name`.

### Added — SPATIAL_FILTER, SORTENTSTABLE, IMAGEDEF_REACTOR, RASTERVARIABLES and TABLESTYLE decode, and the R14 XRECORD and ACDBPLACEHOLDER classes are routed (2026-10-26, #149, #150, #151)

- **XRECORD** (§20.4.104, #254, #222): R14 writes XRECORD under a class number from `AcDb:Classes` rather than the fixed code `0x4F`, so every R14 XRECORD was skipped. The class name now reaches the existing decoder: all 1,154 R14 XRECORD records of the public corpus decode and close, and no XRECORD in the 52-file corpus is skipped or errors. `probe_xrecord` (#222) shows the field list closing on all 1,117 XRECORD records of the corpus it ran on, R14 included, and `probe_material` pins the data-field budget every MATERIAL record leaves after its common object data.
- **ACDBPLACEHOLDER** (#306): R14 and R2000 write it under a class number too, and the 16 records were skipped; they route to the placeholder decoder R2004 and later already use, since the record is the common object data and nothing else.
- **SORTENTSTABLE** (§20.4.93, #307): `BL` numentries and that many sort handles, typed `H` but written with handle code 0 in the main bit stream. All 27 corpus records, skipped on every release, decode.
- **SPATIAL_FILTER** (§20.4.94, #308): the clip boundary of an xref or block insert, with its two matrices of 12 `BD` kept in stream order. The one record of each release was skipped.
- **IMAGEDEF_REACTOR and RASTERVARIABLES** (§20.4.82, §20.4.91, #305): the spec spells the first `IMAGEDEFREACTOR`; both spellings route.
- **TABLESTYLE on R14 to R2007** (§20.4.101.1, #303): the notes that declined it said §20.4 has no entry for it, but §20.4.101 prints it in two layouts, and this is the "until R21" one; R2010 and later (§20.4.101.2) stay `Unhandled`. Two readings come from the bits and not the spec's table: `Data type`, `Data unit type` and `Format string` are R2007+ together, and every colour is the 44-bit `TC` on R2000 and R14 too.

- **Added:** `objects::{clip_spatial_filter::SpatialFilter, block_sortents_table::SortEntsTable, image_def_reactor::ImageDefReactor, image_raster_variables::RasterVariables, acad_table_style::AcadTableStyle}` and the matching `DecodedEntity` variants (the enum is `#[non_exhaustive]`).

### Added (breaking) — the object walk reports its reference closure, and RawObject says how long it is (2026-10-26, closes #81, closes #83)

- **ObjectWalkSummary** (§2.13, closes #81, #282): the lossy handle-driven walk, `DwgFile::all_objects_lossy`, now resolves every record's handle stream against the handle map. The summary carries the references it decoded, every unanswered `(from, to, code)` triple (capped by the new `WalkerLimits::max_retained_references`, with the overflow counted) and how many records expose no handle-stream boundary, with `dangling_soft_pointers()` and `unanswered_hard_references()` splitting them by §2.13 code: soft pointers (2, 4) may name an object no longer in the drawing, hard ones (3, 5) may not. `probe_reference_closure` prints these fields instead of computing them, with its output unchanged.
- **RawObject** (§2.6, closes #83, #281): `raw.len()` and `size_bytes` disagree on R2010+, because the `MS` does not count the record's own leading `MC` (#77). The type now documents its framing, `size_bytes` stays the `MS` verbatim, and `byte_len()` and `mc_width()` say the rest, so a caller never has to know: `mc_width()` is 0 on every R14, R2000, R2004 and R2007 record of the public corpus and 1 or 2 on the others.

- **Added:** `ObjectWalkSummary::{references_decoded, unanswered_references, unanswered_references_suppressed, unanswered_hard_count, references_unchecked}`, `WalkerLimits::max_retained_references` (a `WalkerLimits` struct literal without `..Default::default()` stops compiling), `object::{handle_references, for_each_handle_reference}` and `RawObject::{byte_len, mc_width}`.

### Added — the specification's own worked examples run through the crate (2026-10-26, closes #107)

`examples/spec_examples.rs`, a new step in CI's Test job, runs every worked example and printed table of the ODA specification v5.4.1 through the crate's public code, on fixtures quoted verbatim from the PDF with their page numbers (`tests/fixtures/spec-examples/`), audited against it by `tools/spec/extract_examples.py` (#280). 91 worked examples and 6 tables: the 8 bit streams of §2.2 to §2.14.1 each consume exactly the printed bits, the CRC-8 and CRC-32 tables of §2.14 reproduce 256 of 256 entries each, and the 78 R13 and R14 object records of the §20.4.N.1 examples reproduce their type, length, handle and CRC 78 of 78; 44 close on their own `RL` boundary, 17 error, 4 have no decoder and 13 are class-numbered. `cargo run --release --example spec_examples -- --update` rewrites the moved `expect` cells and notes when a decoder change moves an example, and CI fails until it is run.

Decoding each failing record with the spec's own R13 to R14 field list closed 16 of the 18 that errored, so those were crate bugs, and filed or fixed: #287, #288 and #289 above, with #258, #261, #262, #263 (VIEW), #267, #249 and #297. The other two are errata in the spec itself, recorded with three more in ARCHITECTURE.md §7k: §2.3 says "reading 5 longs" where the stream holds four, §4.1 prints 92 of the 0x6C bytes it says to XOR, §20.1 states no seed for the record CRC (all 78 examples use `0xC0C1`), §20.4.28 RADIUS omits the leader length `BD` that DIAMETER prints, and §20.4.49 TOLERANCE's text is a `TV`, not a `BS`, with one more leading `BD`.

### Fixed — the writer emits the checksums and header fields real files carry (2026-10-26, closes #214, closes #216, closes #233, closes #265)

The legacy reader verifies neither the object-map CRCs nor the record CRCs, so a round trip through it could not see that the writer left them out. The writer and the canonical fixture generator now emit all of the following, and the canonical fixtures are regenerated:

- **R2000 file header** (§3.2.6, closes #214, #215): the file-header CRC (`crc::crc8` over bytes 0 to 52, XOR `crc::r13_locator_seed(3)`) and the 16-byte end sentinel; `streaming::END_SENTINEL` is public. Only bytes 52 to 68 of `synthetic_2000.dwg` change.
- **Object map and record CRCs** (§23.1, §20.1, closes #216, #221): each object map section is a big-endian size that counts its own two bytes, the `MC` pairs and a big-endian CRC seeded `0xC0C1` over the size bytes and the pairs, and the map ends with an empty section of size 2; `handle_map::write_handle_map` wrote the CRC over the pairs only, little-endian, and ended the map with a size-0 header, and `build_fixtures` wrote every record CRC as zeros.
- **R2004-family file header** (§4.1, closes #233, #259): the fixed fields every real file carries (plain `0x28` is `0x80`, the decrypted fields at `0x0C`, `0x10`, `0x14`, `0x44`, `0x48`, `0x4C`) and the 20 trailing bytes copied from the magic sequence; `cipher::header_trailer` is new.
- **System and data page checksums** (§4.2, §4.3, §4.6, closes #265, #266): `file_writer::build_system_page` wrote 0, and the section writer wrote a 32-bit start offset and two rotate-and-add sums. Pages now carry the §4.2 page checksum at the spec's offsets (the §4.3 two-stage form, the §4.6 start offset as 8 bytes, header checksum at `0x18` seeded with the data checksum at `0x1C`).

### Fixed — three panics on malformed files, and the library builds where `usize` is 32 bits (2026-10-26, refs #187)

The nightly fuzz workflow could not fail and did not run for its time budget (`continue-on-error`, `| head -n 500 || true`, and a `-runs=100000` that ended a run after about 11 seconds of 300); #232 removes all three, and its first strict runs found these:

- **`HandleMap::parse`** (§23.1, #231) panicked on an `AcDb:Handles` section whose big-endian size is 1: the size counts the section's own 2-byte CRC, so the payload slice was `[2..1]`. A size below 2 is an `Error::SectionMap`.
- **The R2007 container** (§5.3, §5.4, §5.10, §5.13, #271): a header offset or length that overflows panicked in debug and fuzz builds (`attempt to add with overflow`) and wrapped silently in release. All five `start + len` sites use `checked_range` and fall into the `Truncated` or `SectionMap` error they already reported, and `align_up` saturates.
- **The R2007 LZ back-reference** (§5.10, #291) read `out[out.len() - offset + i]`, so a decoded offset of 0 indexed one past the end. A real stream never encodes a distance of zero; the decoder returns `Lz77InvalidOffset`.

The crash inputs are committed as named seeds under `fuzz/corpus/`.

`WalkerLimits::permissive().max_scan_bytes` and `OpenLimits::permissive().max_section_bytes` were written as `4 * 1024 * 1024 * 1024` in `usize`, which overflows on a 32-bit target, so checking or building the library for wasm32, i686 or armv7 failed (#346). Both go through `saturating_bytes(4 << 30)`: 4 GiB where it fits, `usize::MAX` where `usize` is 32 bits, and the values on 64-bit targets are unchanged. The Clippy job now also runs `cargo check --lib --target wasm32-unknown-unknown`.

### Added (breaking) — the R2004-family object records are streamed (2026-10-26, #205)

`streaming::R2004ObjectWalker` is the whole-file walker with `AcDb:AcDbObjects`
framed into records in section order: each record's `MS`, R2010 `MC`,
payload, CRC, type and handle, handed out as `Payload` steps and committed by
an `Object` step after its checks (docs/streaming/README.md). Every pinned
R2004, R2010, R2013 and R2018 drawing's records equal the legacy
`ObjectWalker`'s, and a record's CRC (seed `0xC0C1`, `MS` through the last
payload byte, `MC` included) is checked, which the legacy walker never did
(ARCHITECTURE.md §7ac). A checkpoint (`R2004ObjectCheckpoint`, `DWGO`) is
self-contained and restores in the wasm32 kernel.

The R13 to R2000 container walk refused an object map chunk of more than
2,032 bytes. A chunk's size counts its own two bytes, so the spec's 2,032 bytes
of entries make 2,034, and AutoCAD's first chunk in the 1.3 MB
`sample_AC1014.dwg` and `sample_AC1015.dwg` is 2,033 and 2,034: the walk
failed on both with `MapChunk`. The limit is 2,034 now; both drawings walk,
1,242 and 1,047 records equal to the legacy reader's.

- **Added:** `streaming::{R2004ObjectWalker, ObjectStep, ObjectDemand,
  OBJECTS_SECTION, R2004ObjectCheckpoint, RecordFormat}`,
  `byte_source::{poll_r2004_objects, ObjectPoll}`, and the
  `StreamError` variants `ObjectPrologue`, `RecordSize`, `RecordRange`,
  `RecordCrc`, `RecordHeader`, `ObjectsOrder`, `ObjectsIncomplete` and
  `ObjectsMissing` (a breaking change for an exhaustive `match` on the enum).
- **Changed:** `FlatWalker` accepts an object map chunk of 2,033 and 2,034
  bytes (it refused them with `MapChunk`).

### Changed — an encrypted section is refused with a typed error by every reader (2026-10-26, #205)

The R2004-family section map carries an encryption flag (§4.5: 0 none, 1
encrypted, 2 properties only). The legacy reader ignored it, so a
password-protected section's ciphertext was decompressed and returned as if
it were the section; the streaming walker (`R2004SectionWalker`) already
refused it. `section_map::read_section_payload`, and so every
`DwgFile::read_section` and the readers built on it, now returns the new
`Error::SectionEncrypted { name }` for a section whose flag is 1, the same
refusal as `streaming::StreamError::SectionEncrypted`. A flag of 2 marks only
the properties (`AcDb:FileDepList`, `AcDb:AppInfo` in real files) and is
still read. The R2007 refusal, which was `Error::Unsupported`, returns the
same variant. No pinned drawing has the flag set to 1, so the corpus counts
do not move; `tools/streaming/verify_sections.py` repackages the sections of
four real drawings with one flagged (and a control without) and checks that
both readers refuse the one and read the other equal to the drawing.

- **Added:** `Error::SectionEncrypted { name: String }`.
- **Changed:** a section with encryption flag 1 is `Err(SectionEncrypted)`
  from `read_section_payload` instead of its undecrypted bytes, and an R2007
  section with encryption other than 0 or 2 is `Err(SectionEncrypted)` instead
  of `Err(Unsupported)` (its unit test is adapted).

### Changed (breaking) — `section_map::DataPageHeader` carries the measured §4.6 layout (2026-10-26, #205)

The struct labelled its eight words after the §4.6 table, and from `0x14`
on the labels did not match any data page of the pinned corpus. After the
Sec_Mask XOR, on all 580 data pages of the 28 pinned R2004-family drawings
and the four canonical fixtures (`examples/probe_data_page_header.rs
--strict`, run by the Corpus job; ARCHITECTURE.md §7n): `0x08` data size
(the stored bytes after the header), `0x0C` page size (the page map's size
for the page, not the decompressed size), `0x10` start offset (8 bytes),
`0x18` header checksum (§4.2 over the header with its own field zeroed,
seeded with the data checksum) and `0x1C` data checksum (§4.2 over the
stored bytes, seed 0).

- **Changed:** `DataPageHeader::start_offset` is `u64` (was `u32`).
- **Renamed:** `data_checksum` read `0x18` and now reads `0x1C`; the old
  `page_header_checksum` (`0x14`, the high half of the start offset) is gone,
  and `_unknown` (`0x1C`) is now `data_checksum`. New field: `header_checksum`
  (`0x18`).
- `read_section_payload` takes its LZ77 capacity hint from the section's
  declared page size instead of the page's size on disk (a hint only; the
  assembled bytes are unchanged).

### Fixed — Reed-Solomon uses §5.13's codes, as AutoCAD writes them (2026-10-26, closes #275)

`reed_solomon` decoded R2007 system pages with `0x11D`, which §5.13 gives
to data pages, and read each codeword back to front; no codeword of a real
file verified. It now implements both §5.13 codes as `Code::SYSTEM_PAGE`
((255, 239) over `0x169`) and `Code::DATA_PAGE` ((255, 251) over `0x11D`),
with byte *i* the coefficient of x^i, roots α^1 … α^(n-k) and the message
first, all measured: on the eight R2007 drawings of the pinned corpus,
every one of 187 system and 4,425 data codewords verifies, re-encodes to
AutoCAD's own parity bytes and is restored after (n-k)/2 injected errors
(`examples/probe_r2007_rs.rs`, run by the Corpus job).

- `reed_solomon::verify`, `syndromes`, `codewords_for_message` and the
  `CODEWORD_BYTES` / `PARITY_BYTES` / `MESSAGE_BYTES` constants keep their
  signatures and now mean the system-page code; `Code::verify` returns the
  number of bytes corrected, and `Code::verify_interleaved` handles §5.13.2.
- `reed_solomon_encode::encode_with(code, message)` encodes for either code.
- **Removed:** `reed_solomon::verify_stream` and `decode_stream`, which
  treated a block as back-to-back 255-byte codewords, a layout §5.13 does
  not use (it interleaves, §5.13.2, or puts every data block before every
  parity block, §5.13.1).

### Added — the roadmap to 1.0 as milestones, epics and a spec coverage matrix (2026-10-21)

The plan is now on GitHub: seven milestones from 0.2.0 to 1.0.0, eighteen
epics and their sub-issues, every open issue labelled and placed. 1.0 has no
gaps or limitations: every release from R13 to R2018 and every section of
ODA spec v5.4.1, values equal to AutoCAD's own DXF export, a writer AutoCAD
opens, and complete exports, viewer and Python bindings.

- `docs/spec-coverage.md` (data in `docs/data/spec-coverage.tsv`) lists all
  138 sections of the spec with their state on `main`: 86 close on every
  corpus record, 7 have an unmeasured decoder, 11 are partial, 34 are
  missing, and none is complete, because no value has yet been compared
  with AutoCAD's export.
- `docs/release-1.0-criteria.md` lists each 1.0 requirement with its bar,
  the value measured now and a status (passed, failed, not measured).
- `docs/maintaining.md` sets out the tracker: labels, milestones, epics,
  triage and the monthly housekeeping that `tools/maintainer/audit.py`
  checks.
- `ROADMAP.md` is rewritten around the milestones.
- CodeQL scans the workflows and the Rust sources (`.github/workflows/codeql.yml`),
  and the `main-protection` ruleset is kept as code in `.github/rulesets/`.
- The issue forms apply labels that exist.

### Fixed — a record's `MS` excludes its own `MC`, and the class table is not always a live census (2026-08-30, closes #77, closes #76)

**The walker's raw slice ended one `MC` width short on every R2010+
record.** #75 measured that each inter-record run in
`AcDb:AcDbObjects` equals the width of the *preceding* record's leading
`MC` handle-stream-size field — 841 of 841 on `sample_AC1032.dwg` and
100 % on three more files, against 734 of 841 for the "following
record" reading, with only 8 of the 916 bytes zero. That settles the
question the module docs left open: **the `MS` does not count the
record's own `MC`**, so a record occupies `MS field | MC field | MS
bytes | CRC` and [`RawObject::raw`] is now sized `mc_width +
size_bytes`. `size_bytes` still reports the `MS` verbatim.

The correction that `string_stream::data_section_end` carried —
`payload_bits - handle_bits + mc_field_bits` — was compensating for
exactly that short slice, and is removed: the rule is now
`payload_bits - handle_bits`, and the two edits cancel to the same bit
offset. **No decoder changes behaviour**: full-corpus coverage is
byte-identical before and after — 3695 / 741 / 9 / **83.1 %** on this
change's pre-#79 base, and re-measured after merging #79's VISUALSTYLE
work the combined tree reproduces main's 3911 / 525 / 9 / **88.0 %**
exactly. All 19 test binaries pass.

Two things the short slice was hiding:

- **The sequential (handle-map-less) walk drifted** by one `MC` width
  per record on R2010+, because `pos` advanced to the short `crc_end`.
  `DwgFile::objects()` uses that mode.
- **Every R2004+ corpus file now tiles its object stream exactly.**
  `sample_AC1032.dwg` goes from 916 unclaimed bytes to 4 — the
  `0x0dca` prologue — with zero bytes between records and zero
  trailing, and so do all eighteen other files.

`examples/build_fixtures` writes the same rule (the emitted `MS` is
`payload.len() - mc_width`) and now also writes the §19.1 *strings
present* trailer bit with the `MC` recording the padding that stands in
for the handle stream. The R2010/R2013/R2018 fixtures are regenerated;
declaring a zero-bit handle stream had put the data boundary past the
end of the record, which silently disabled every decoder's
boundary check on those three files.

**DICTIONARYVAR / CELLSTYLEMAP are not under-walked (#76).** The new
`examples/probe_reference_closure` runs three measurements that never
consult the class table, and all three are closed on all nineteen
corpus files:

- the walked records **tile the whole section** — zero bytes between
  records on every R2004+ file, so an unreferenced record has nowhere
  to sit;
- **every hard handle reference** (§2.13 codes 3 and 5) in every
  record's handle stream resolves to a map entry. The only unanswered
  references in the whole corpus are six code-4 *soft* pointers — the
  class the spec permits to dangle — from BLOCK_HEADER `0xA0B` of
  `sample_AC1032.dwg` to erased INSERTs (`0xD17`, `0xD18`, `0xD40`,
  `0xD41`, `0xD95`, `0xD96`), in a drawing whose handle space is sparse
  by 2,841 of 3,683 values. This refines #56's "no reference the walk
  cannot answer", which predates the soft/hard split;
- the **`AcDbVariableDictionary`** — the sole owner of a drawing's
  DICTIONARYVAR objects — holds exactly as many keys as the walk finds
  DICTIONARYVAR records on **every** file: 10 / 6 / 5 on the R2004 /
  R2007 / R2010 bands that declare 16 / 11 / 10, and 5 and 11 on
  `arc_2013.dwg` and `sample_AC1032.dwg`, whose class tables agree with
  the walk. The class table is the outlier, not the walk.

`arc_2010.dwg` settles CELLSTYLEMAP the same way: it declares one,
contains none, and carries no
`ACAD_ROUNDTRIP_2008_TABLESTYLE_CELLSTYLEMAP` dictionary key for one to
hang from — a live census cannot declare an instance with no owner
slot. The `AcDb:Classes` parse is not in doubt either: on
`arc_2004.dwg` the ten records consume 5161 of the declared 5168 bits
with 7 bits of byte padding, class numbers run 500..509 with no gap,
and eight of the ten declared counts match the walk exactly (VISUALSTYLE
24, SCALE 17, MATERIAL 3, …).

So the `KNOWN_SHORTFALL` pins in `tests/class_census.rs` keep their
counts — nothing was under-walked to recover — and are re-documented as
files that over-declare rather than as an open walker gap. They still
gate: a *drop* in any `walked` figure fails the ratchet.
### Added — VISUALSTYLE on R14, R2000 and R2007: one field list, four releases (2026-08-30, closes #73)

#72 made the R14 / R2000 / R2007 object walk work and their 216
VISUALSTYLE records reached a decoder for the first time. Neither
shipped field list closed on any of them, and #73 opened on the obvious
reading — a third generation of the layout. **There is not one.** All
four pre-R2010 releases write the same 30 flag-less fields in the same
order; the 216 misses have two causes that are not field order.

- **The record distribution, measured first.** Every one of the
  nineteen corpus files holds exactly 24 VISUALSTYLE records, R14
  included — so the built-in styles are not a 2007-era addition and the
  216 split **72 / 72 / 72** across R14 / R2000 / R2007, not 108 / 108
  / 0. `examples/probe_visualstyle_layout.rs` with no `--spec` is the
  census.
- **R14 and R2000: §2.11's older colour form.** Before R2004 a colour
  is the bare `BS` index; the `BL` true-colour word and `RC` colour
  byte are R2004's addition, and five colour slots account for most of
  the gap. Read the colours that way and the same 30 fields land **all
  144 records** of the six R14 / R2000 files exactly on their boundary,
  delta 0. The two bands are bit-identical in shape — an R14 file and
  the R2000 file built from the same drawing give the same 24 budgets
  (388, 476, 412, 500, 396, 444, 428, 484, 508, 436, 420, 436, 420,
  452, 452, 460, 540, 484, 412, 524, 428, 436, 452, 460 bits).
- **The two colour encodings agree through the change.** Where R2004
  writes the "none" method `0xC8000000`, R14 / R2000 write index `257`;
  where R2004 writes `0xC3000007` they write index `7`; and where
  `ColorChange` writes the grey `0xC2808080` for its face and edge
  colour and `Shaded` writes `0xC2787878` for its silhouette, they
  write ACI index `8` — in exactly those three slots of exactly those
  two records. `internal_style_type` (`0` `Flat` … `27` `Shaded`), the
  signed `face_opacity` (`-0.6`, `+0.5` on `X-Ray`),
  `edge_crease_angle` (`40` / `179` / `1`), the `BL`
  `display_brightness` (`-50` / `50` / `0`) and the final-bit
  ten-of-twenty-four `is_internal_use_only` split all agree with R2004
  record for record.
- **R2007: the same list plus one 2-bit slot.** R2007 keeps the R2004
  colour form and lands 2 bits short on all 72 records; the leftover
  bits are `10` then `is_internal_use_only` — `101` on the fourteen
  hidden styles, `100` on the ten listed ones, on every record of all
  three files. One more 2-bit read before that final `B` closes all 72,
  delta 0. `Dim` pins the position: it is the one record whose
  `display_brightness` is the full 34-bit `BL` rather than the 2-bit
  zero form, and putting the slot before the brightness closes 23 of 24
  and fails on exactly `Dim`. Putting it in the head, or after
  `is_internal_use_only`, fails on all 24.
- **What is declined.** The new slot's token type and its side of
  `display_shadow_type` are not determined — both are two bits reading
  `0` on all 72 records — so it is surfaced as
  `AcadVisualStyle::display_unknown_short` rather than named after a
  guess. It is not R2010's `BS format_version`, which decodes `2`.
- **On width-only searches.** `probe_visualstyle_layout --search`
  measures every one-token neighbour (insert / delete / substitute over
  `B BS BL BD RC CMC`) against every record at once. It reports 33
  closing neighbours on R2007 and 64 on the R14 / R2000 band, all of
  them encoding aliases rather than rival layouts: pre-R2004 a colour
  *is* a `BS`, and `BS` / `BL` / `BD` coincide on the small-value forms
  these records use. Width alone does not pick the layout; the
  cross-release value agreement does.
- `examples/probe_visualstyle_layout.rs` is new — census, joint-boundary
  check and uniqueness search over every VISUALSTYLE record of every
  file named on the command line.
- `tests/samples.rs` gains a corpus-gated pin over all 456 records: 24
  per file, the ten/fourteen internal-use split, the dense `0`…`27`
  enumeration, `X-Ray`'s opacity, the crease angles, the brightness
  triple and `ColorChange`'s grey in both encodings.
- Six new BitWriter tests in `objects::acad_visual_style`, one per band
  boundary: an R14 record closing on the `RL` inside its own common
  object data, an R2000 record closing on bare-index colours, an R2007
  record closing on its string stream, and three negative directions.

**Measured effect** on the 19-file corpus
(`examples/coverage_report.rs`):

| Corpus slice | Before (`6cc9b13`) | After |
|---|---|---|
| R14 (AC1014) × 3 | 426 / 450 / 0 / 48.6 % | **498 / 378 / 0 / 56.8 %** |
| R2000 (AC1015) × 3 | 534 / 99 / 0 / 84.4 % | **606 / 27 / 0 / 95.7 %** |
| R2007 (AC1021) × 3 | 462 / 96 / 0 / 82.8 % | **534 / 24 / 0 / 95.7 %** |
| **Aggregate** | **3695 / 741 / 9 / 83.1 %** | **3911 / 525 / 9 / 88.0 %** |

VISUALSTYLE leaves the unhandled histogram entirely; XRECORD (345) is
now the largest unmatched class. The error count is unchanged at nine.

### Fixed — the BLOCK_HEADER name is a stem; the BLOCK sentinel is the authority (2026-08-30, closes #70)

`BLOCK_RECORD` decoded `*D` / `*T` / `*Paper_Space` where the adjacent
`BLOCK` entity of the same definition decoded `*D3` / `*T9` /
`*Paper_Space0`. Both records closed their own boundary, so nothing
errored — eight BLOCK_HEADERs of `sample_AC1032.dwg` simply came back
named `*D`, which no symbol table can hold.

**Neither reader was on the wrong slot.** Read positionally from the
bit `string_stream::locate` reports, BLOCK_HEADER `0x74` of
`arc_2013.dwg` holds one 12-unit string and two empty `TV` slots in a
string stream whose §19.1 trailer declares exactly 206 bits — there is
no room for a 13-unit `*Paper_Space0` (222) and no second copy
anywhere in the record. Handles `0x6C` and `0x74` are byte-identical
apart from their own handle and their handle streams, yet AutoCAD's
own DXF twin of that drawing (`arc_2013.dxf`, public
`nextgis/dwg_samples`) names them `*Paper_Space` and `*Paper_Space0`.
For a block AutoCAD names itself, the BLOCK_HEADER carries only the
stem and the generated numeric suffix lives solely on the `BLOCK`
sentinel. The split is not an artefact of the R2007+ string stream:
the R14, R2000 and R2004 files store their names inline and disagree
in exactly the same way.

- `tables::block_record::block_sentinel_handle_of` reads the `BLOCK`
  handle out of a BLOCK_HEADER's own handle stream. §20.4.52 puts a
  NULL reference (hard pointer, zero-byte counter) immediately before
  it, which makes the NULL a self-identifying anchor — no reactor
  count and no xdictionary flag needed. 27 of 27 on
  `sample_AC1032.dwg`, 3 of 3 on every R2000-R2018 corpus file.
- `graph::resolve_block_names` performs the join and is what callers
  should use for a block table. It resolves 27 of 27 definitions on
  `sample_AC1032.dwg` to 27 distinct names where the stored stems
  collapse to 20. R13/R14 records carry neither the §19.1 trailer nor
  the `RL` object-data size, so they stay unresolved rather than
  guessed.
- The R2007+ BLOCK_HEADER decoder moves onto the shared
  `tables::modern::SplitStream` path every other table entry already
  uses, and is now held to its string-stream boundary by
  `SplitStream::finish`. It replaces a bespoke scanner that searched
  the record for the best-scoring plausible-looking block name — a
  heuristic that could land on any `TV`-shaped bits in the payload.
  All 27 records close on delta 0. The record's `description` field is
  surfaced (it was read and discarded), and the entry flags now use
  the measured `B, B, BS` order rather than the inline `B, BS, B`,
  which had `is_xref_dependent` and `is_xref_resolved` swapped.

### Added — the class census is a CI gate, and the object stream is 100 % accounted for (2026-08-30, closes #56)

`examples/probe_class_census` exits non-zero on any under-walked
class; CI now runs it with `--strict` over every canonical fixture
(`coverage-smoke` job), `tests/canonical_corpus.rs` asserts the same
census **exactly** rather than `>=`, and `tests/class_census.rs`
ratchets the real corpus: every under-walk that exists today is pinned
by file, class and count, so a new one fails and a fixed one also
fails.

**The 916 unclaimed object-stream bytes of `sample_AC1032.dwg` are not
alignment padding.** They are not zero, and each inter-record run
equals the width of the *preceding* record's leading `MC`
handle-stream-size field — 841 of 841 agreements against 734 of 841
for the "following record" reading, on four files with no exceptions.
772 one-byte and 70 two-byte `MC` fields plus a 4-byte section
prologue account for all 916. So a record's `MS` object size excludes
its own `MC` field, which settles the open question in
`src/string_stream.rs` about why `data_section_end` needs its
`+ mc_field_bits` correction, and the stream is 100 % accounted for
with zero padding.

**TABLECONTENT / TABLEGEOMETRY (#56) is not decidable from this
corpus.** `sample_AC1032.dwg` declares 5 of each against 2
`ACAD_TABLE` entities and contains 2 of each. Three measurements place
the missing six records outside the drawing: the object stream is
99.92 % covered with no unclaimed run over 4 bytes; decoding every one
of the 842 records' handle streams yields no reference the walk cannot
answer; and ACAD_TABLE itself declares 2 and walks 2, so a 1:1 content
object cannot have 2.5 instances per table. Whether the surplus three
are a stale registration count or copies embedded in the two
ACAD_TABLE records needs a corpus with more than one table-bearing
file, so the gate carries an explicit allowlist with that reason.

### Added — R14, R2000 and R2007 files now walk their object streams (2026-08-30, closes #104, #110, #65)

Nine of the nineteen corpus files reported `n/a (no-handle-map)` in
every coverage run this project has published. Not "decoded badly":
**not walked at all** — three whole release bands, and with them the
pre-2004 half of every field list in the crate, had no evidence behind
them. They all decode now, with zero errored records.

**R13-R15 (`AC1014`, `AC1015`)** need no new container: §3.1 puts the
object records loose in the file and §3.2.6's object map addresses them
by *absolute file offset*, so `DwgFile::object_stream` hands back the
whole file and the existing §4.4 handle-map parser and
`ObjectWalker::with_handle_map` do the rest. The locator records are
given the canonical `AcDb:` names, which is what makes `handle_map()`,
`class_map()` and `header_vars()` version-agnostic. Every one of the
292 (R14) / 211 (R2000) handle-map entries per file resolves to a record
whose own handle field matches the map.

**R2007 (`AC1021`)** gets a real container (`src/r2007.rs`,
`src/r21_lz.rs`): Reed-Solomon de-interleaving of the 0x400-byte file
header, the §5.10 LZ variant, the page map, the section map and
per-page data assembly. The decoded file header reproduces every
constant §5.2 documents on all three corpus files, its `File size`
field equals each file's own byte count, and all twelve tabulated
section hash codes match. `SectionMapStatus::Deferred` is gone for
AC1021.

The `src/r2007.rs` this replaces described R2007 as a two-layer
"Sec_Mask" — a byte XOR plus a 7-byte bit rotation. **R2007 uses
neither**; that module described a transformation the format does not
have, and it is removed rather than kept as a scaffold. `xor_layer_1`
and `rotate_layer_2` are gone from the public API.

**#65** falls out of the same work: §19.1's `RL` object-data-size *is*
R2007's data-and-string end, so `string_stream::data_section_end`
answers for AC1021 and every R2007+ decoder in the crate — the whole
`tables::modern` / `objects::modern` family — runs on R2007 without a
line of new decoder code.

### Changed — every entity is boundary-checked on every release

`entities::dispatch::checked_inline` previously returned `None` for the
boundary on R13/R14 and R2007. Both bands state one: R2007 through the
§19.1 trailer located from its `RL`, R13/R14 through the same `RL`,
which §20.4.1 places *inside* the common entity data rather than in the
object prologue. The pre-R2007 symbol-table path is checked too. **No
entity or table entry in the corpus decodes unchecked on any release.**

### Fixed — four pre-R2007 field lists the new files exposed

Each was wrong on R2000 and R2004 as well; only files that reach a
decoder could show it. All four now close on `delta 0`, and each is
corroborated by the same drawing decoded on a later release:

- **LAYER** read `BS values`, a `B` plot flag, a `BS` lineweight and a
  `BS` colour. §20.4.54 has `BS values` — which already *contains* the
  plot flag and the lineweight index — then the colour. `line_2004.dwg`
  had been reporting lineweight −32765 and colour −31231; layer `0` now
  reads colour 7 on every release from R14 to R2018.
- **LTYPE** read an `RC flags` and an `RS used_count` that §20.4.58 does
  not list, pushing the description three bytes late: every pre-R2007
  `Continuous` record reported `lid line` instead of `Solid line`. The
  dash-record field order and the `X 9` 256-byte text area are corrected
  to the spec at the same time.
- **BLOCK_HEADER** stopped after the xref pathname, 12 bits short on all
  six R2000/R2004 records — §20.4.52 adds an insert-count run, a
  description `TV` and a preview blob on R2000+, and puts the loaded
  bit there too (R14 has none).
- **DIMSTYLE** decoded 15 of ~70 fields and could not close. The whole
  §20.4.68 R2000+ body is now read inline; `ISO-25` decodes to the
  published ISO-25 defaults on R2000 and R2004, matching what the
  R2007+ path reads from the same drawing.

Also fixed for R14 specifically: the §20.1 / §20.4.1 `RL` after the EED
chain (which every DICTIONARY, APPID, LTYPE, DIMSTYLE and BLOCK_HEADER
record needed), DICTIONARY's `RC` where R2000+ write `BS` + `RC`,
DIMSTYLE_CONTROL's absent trailing `RC`, VPORT's absent render mode and
per-viewport UCS block, LINE's `3BD` + `3BD` body (§20.4.21), and the
`BB` linetype and plot-style flags that are R2000+ only.

### Added — an R2000 canonical fixture

`examples/build_fixtures.rs` now emits `synthetic_2000.dwg` through a
flat §3.2.6 assembler, so the committed corpus gates a container whose
handle-map offsets are file offsets rather than section offsets. Five
files, 25 entities, 100 % decoded. (#21, R2000 half.)

### Changed — the exact-boundary check now covers every entity type (2026-08-30, closes #63)

Until now only the entity types that *had* to know about the R2007+
split streams asserted anything about where their fields end. The
POLYLINE family, MESH, IMAGE, WIPEOUT, VIEWPORT, LEADER, MLINE, the
four SURFACE variants, RAY, XLINE, POINT, CIRCLE, ARC, LINE, ELLIPSE,
SOLID, TRACE, ENDBLK and BLOCK decoded with **no boundary check at
all** — and on the R2000/R2004 band, so did every entity type. Their
zero error count was a property of the dispatcher, not of the bytes.

Every fixed-code and custom-class entity decoder now runs through
`entities::dispatch::checked_inline`, which requires the field list to
end exactly on the record's own data-stream boundary: the string-stream
start bit (or the handle-stream start minus the one `strings present`
trailer bit) on R2010+, and the `RL` object-data-size-in-bits on
R2000 / R2004. **On R2010+ the number of entity types that decode
unchecked is now zero.** R2007 and R13/R14 state no boundary this crate
can read, and no record of those releases reaches a decoder.

Three field lists the check caught, each fixed to `delta 0` with an
independent value corroborating it:

- **BLOCK** (45 records, R2010/R2013/R2018) read its one `TV` inline.
  On R2010+ the name lives in the string stream and the record's data
  fields have a budget of **zero bits** — 27/27 on `sample_AC1032.dwg`
  and 3/3 on each of `arc_2010.dwg` / `arc_2013.dwg`. Names now decode
  as `*Model_Space`, `_ArchTick`, `MyBlock`, matching the BLOCK_RECORD
  entries they pair with. The R2004 records were already correct and
  now close on their `RL`.
- **POLYLINE_PFACE** (1 record) claimed five `BS` counts and two inline
  `H` handles, overrunning by 52 bits. The record holds `BS 5`, `BS 2`,
  `BL 7` in 30 bits — and the file carries exactly 5 VERTEX_PFACE and 2
  VERTEX_PFACE_FACE records, summing to 7.
- **MLINE** (3 records) read `num_lines` as a `BS` where the record
  writes one `RC`, decoding `lines = 521` and `verts = -11716`. One
  token changed; all three now close at `delta 0` with `num_lines = 2`.

Five types that previously came back `Unhandled` are now decoded,
because the check can prove their field lists right rather than assume
them — handles `0x422`..`0x431` of `sample_AC1032.dwg` read back as a
self-consistent chain:

- **SEQEND** — no fields at all; budget 0 on all four records.
- **POLYLINE_3D** — `RC`, `RC`, `BL 5`; followed by exactly 5 VERTEX_3D.
- **VERTEX_3D / VERTEX_MESH / VERTEX_PFACE** — `RC flag`, `3BD`; the
  flags read `0x20` ("3D polyline vertex") and `0xC0` ("polyface mesh
  vertex" + "3D polygon mesh vertex"), the crate's own flag table.
- **VERTEX_PFACE_FACE** — four `BS` indices, `[1, 2, 3, -4]` and
  `[-1, 4, 5, 0]`: all inside the mesh's declared 5-vertex range, with
  the negative-index invisible-edge convention and the `0` terminator
  of a three-corner face.

Three decoders fail the check and are reported rather than patched —
documented offsets, and stop:

| Type | Records | Delta | Why it is not fixed |
|---|---|---|---|
| VIEWPORT | 6 | −819 | the decoder reads 266 of 1125 bits by design; six records of one identical shape give no variation to derive the rest from |
| MESH | 2 | −2 | the two trailing bits read `10` on both records — the zero/default code of `BS`, `BL` and `BD` alike |
| LEADER | 1 | −12 | twelve bits several continuations of §19.4.19 would fit, on one record |

Also in this change: `POLYGON_MESH` stops reading two `H` handles from
the data stream (an object reference has never occupied data-stream
bits from R2000 on), and `examples/probe_entity_budgets.rs` prints the
bit budget every entity record's field list has to fill.

Measured coverage on the local 19-file corpus, before → after:

| Version | Decoded | Skipped | Errored | Ratio |
|---|---|---|---|---|
| R2004 (AC1018) | 582 → 582 | 15 → 15 | 0 → 0 | 97.5 % → **97.5 %** |
| R2010 (AC1024) | 531 → 531 | 12 → 12 | 0 → 0 | 97.8 % → **97.8 %** |
| R2013 (AC1027) | 384 → 384 | 12 → 12 | 0 → 0 | 97.0 % → **97.0 %** |
| R2018 (AC1032) | 765 → 776 | 77 → 57 | 0 → 9 | 90.9 % → **92.2 %** |
| **Aggregate** | **2262 → 2273** | **116 → 96** | **0 → 9** | 95.1 % → **95.6 %** |

The three unchanged slices are the point: their counts did not move,
but every entity record in them is now checked where none was before,
and every one passes.

### Added — VISUALSTYLE on R2004: the flag-less generation (2026-08-30, refs #48)

`VISUALSTYLE` now dispatches on R2004 as well as R2010+, taking all 72
of its `AC1018` corpus records — the whole of the largest remaining
`Unhandled` block. R2004 stores the same 24 shipped styles on a
**second field list**: 30 fields, no `(value, flag)` pairs, one fewer
property and a different order. It closes with delta 0 on all 24
records of each of `arc_2004.dwg`, `circle_2004.dwg` and
`line_2004.dwg`.

- **Both ends of the record are anchored, not assumed.** The front
  comes from `examples/probe_token_scan.rs` — the full-form `BD`s and
  method-carrying `CMC`s that cannot survive a one-bit shift. The back
  comes from a reverse scan for the unique offset at which
  `BS, BL, BS, B` lands exactly on the `RL` object-data-size boundary;
  it returns exactly one position per record, and its values reproduce
  R2010's `edge_style_apply`, `display_shadow_type` and
  `is_internal_use_only` on all 24.
- **Two structural findings.** `is_internal_use_only` is the record's
  **final bit** on R2004, not part of its head — and it splits the 24
  styles into precisely the ten AutoCAD's Visual Styles Manager lists
  and the fourteen it hides, the same partition R2010 makes 500-odd
  bits earlier. `display_brightness` is a `BL` where R2010 spends a
  `BD`, decoding `-50` / `50` / `0` against R2010's `-50.0` / `50.0` /
  `0.0`; that `BL` is the whole reason `Dim`'s record runs 32 bits
  longer than its neighbours' and `Brighten`'s 8.
- **Cross-file value corroboration.** Twelve `BS` slots, all five
  `CMC`s and both remaining `BD`s decode the same value as the *same
  style* on `arc_2010.dwg`, on all 24 records — `edge_modifier`
  `0`/`8`/`10`/`11`/`12`, `edge_silhouette_width` `3`/`5`/`6`,
  `edge_obscured_linetype` `2` on `Hidden`, `7` on `Linepattern`,
  `edge_crease_angle` `40`/`179`/`1`, `ColorChange`'s grey
  `0xC2808080` and `Shaded`'s `0xC2787878` silhouette included.
  `face_opacity` and `face_specular` are **signed** on R2004: their
  magnitudes match R2010 everywhere, and the sign tracks whether the
  property applies to the style.
- **One 13-bit run is declared, not invented.** Between
  `edge_silhouette_width` and `edge_intersection_linetype` sit 13 bits
  that are constant on every one of the 72 records, so only their total
  width is evidence. The module reads them as `RC BS B BS`, names the
  last three for the three R2010 properties that occupy the same
  position (all zero), and surfaces the leading byte as
  `AcadVisualStyle::edge_unknown_byte`.
- **R2007 stays declined and the reason is upstream.** The container
  parse is `Deferred` for `AC1021` and the split-stream trailer is
  located from a leading `MC` field only R2010+ writes, so no R2007
  object of any type reaches a decoder. `decode_object` returns
  `Error::Unsupported` there rather than shipping an unmeasured
  reading, and the dispatcher maps that to `Unhandled` — #48 stays open
  for that band.

Measured coverage on the local 19-file corpus, before → after:

| Version | Decoded | Skipped | Errored | Ratio |
|---|---|---|---|---|
| R2004 (AC1018) | 510 → 582 | 87 → 15 | 0 → 0 | 85.4 % → **97.5 %** |
| R2010 (AC1024) | 531 → 531 | 12 → 12 | 0 → 0 | 97.8 % → **97.8 %** |
| R2013 (AC1027) | 384 → 384 | 12 → 12 | 0 → 0 | 97.0 % → **97.0 %** |
| R2018 (AC1032) | 762 → 762 | 80 → 80 | 0 → 0 | 90.5 % → **90.5 %** |
| **Aggregate** | **2187 → 2259** | **191 → 119** | **0 → 0** | 92.0 % → **95.0 %** |

(Re-measured on `main` after #67 (ACIS record) with this change merged: R2018 765 / 77 / 0 / 90.9 %, aggregate **2262 / 116 / 0 / 95.1 %**.)

### Added — the §20.4.41 ACIS record decodes, and the R2013+ `has AcDs binary data` marker is settled at zero bits (2026-08-30, #67, refs #54)

`REGION` (37), `3DSOLID` (38) and `BODY` (39) share one record shape in §20.4.41, and §24.2.2.3 explains what R2013 changed: the geometry payload leaves the entity and becomes a data record in `AcDb:AcDsPrototype_1b`, flagged by the record's `has AcDs binary data` bit (#67). The corpus holds exactly three ACIS records, all in `sample_AC1032.dwg`, and all three set that bit. Measured from bit 82, where the common entity preamble ends on all three, the §20.4.41 list closes each record exactly on its data-stream boundary, delta 0: the wireframe and point flags, the point, `num isolines` (4 on all three, AutoCAD's default `ISOLINES`), the counts, and a 16-byte revision GUID that is a valid RFC 4122 version-4 UUID on all three. The inline ACIS envelope is absent from these records. Only the width of the seven bits before the GUID is measured, so the module names them as a labelling, not a measurement.

The same three records settle #54. The marker had been read three different ways: 16 bits after the flag by LAYOUT, an `RC` by the table entries, and nothing by the common entity data. `examples/probe_acis_records.rs` re-reads the preamble at each width: only 0 bits reproduces the values every other entity of the file carries (BYLAYER colour `0x0100`, linetype scale 1.0, invisibility 0, lineweight `0x1D`) and closes the §20.4.41 list. The flag is a flag, and nothing follows it. The 16 bits the four R2018 LAYOUT records need moved into LAYOUT's own record as `AcadLayout::ds_block` (#236 later found them to be the first 16 bits of `plot_layout_flags`), and all 31 corpus LAYOUT records still close.

Two departures from §20.4.41 in the envelope reader, which no corpus record had reached, are corrected: the `B unknown` bit between `ACIS empty` and `BS version` was not read at all, and the block loop read a `B has_more_blocks` flag before each `BL block size`, where the spec ends the loop on a block size of 0 and has no such flag. Version 2, where the spec gives no length, is refused instead of mis-stepped (#283 reads it later).

Measured on the 19-file corpus (`examples/coverage_report.rs`, decoded / skipped / errored / ratio): R2018 goes from 762 / 80 / 0 / 90.5 % to **765 / 77 / 0 / 90.9 %**, the aggregate from 2187 / 191 / 0 / 92.0 % to **2190 / 188 / 0 / 92.1 %**, and the R2004, R2010 and R2013 bands do not move. Zero errored records, before and after.

- **Added:** `entities::modeler::{ModelerTail, ModelerRecord}`, `Body`, `Region` and `ThreeDSolid` fields `in_data_store` and `tail`, `AcadLayout::ds_block`, the `DecodedEntity` variants `ThreeDSolid`, `Region` and `Body` (the enum is `#[non_exhaustive]`), and `examples/probe_acis_records.rs`.

### Added — MLINESTYLE (§20.4.73) and three style classes the spec does not prescribe (2026-08-30, closes #55)

`MLINESTYLE`, `MLEADERSTYLE`, `ACDBDETAILVIEWSTYLE` and
`ACDBSECTIONVIEWSTYLE` now dispatch to typed decoders on every release
band this crate walks. All 43 records of the local 19-file corpus close
exactly on their own data-stream boundary — the first bit of the string
stream on R2010+, the `RL` object-data-size on R2004 — so a wrong field
list errors rather than returning a plausible struct.

- **MLINESTYLE comes from the spec.** §20.4.73 prescribes the whole
  record; the module previously cited a "§19.6.4 (L6-13)" that does not
  exist and read `BS` where the spec prints `CMC` for both colour
  fields. Read straight, it closes on all ten records: R2004 budget 526,
  R2010 and R2013 442, R2018 406 — the R2018 drop being exactly the two
  18-bit `BS ltindex` words that release moves into the handle stream.
- **The other three have no §20.4 entry** and were derived by the
  joint-boundary search over `B/BS/BL/BD/RC/CMC/TV`, with the pre-R2007
  band — where a `TV` costs real data-stream bits — pinning the string
  positions. 33 records, four release bands, delta 0 on every one.
  Budgets: MLEADERSTYLE 744 / 692 / 693 / 693 · 749;
  ACDBDETAILVIEWSTYLE 1209 / 667 / 667 / 677 · 605;
  ACDBSECTIONVIEWSTYLE 2325 / 1301 / 1301 / 1263 · 1367.
- **Values corroborate the layouts**: MLEADERSTYLE's `Standard` decodes
  AutoCAD's shipped defaults (landing gap `0.09`, dogleg `0.36`,
  arrowhead `0.18`, text height `0.18`, break gap `0.125`, block scales
  `1,1,1`, ByBlock colours, lineweight `-2`); the view styles decode
  `5` / `0.24` text heights for `Metric50` / `Imperial24`, lineweights
  `25` and `50`, and ACDBSECTIONVIEWSTYLE ends in five doubles that are
  90°, 15°, 75°, −15° and 105° in radians.
- **TABLESTYLE is declined, not guessed.** Its structure is measured —
  a 52-bit header plus four cell-style blocks of 1738 / 1696 / 1696 /
  1662 bits, each ending in six 168-bit border sub-records — but one
  record per corpus file cannot pin the token sequence inside a block.
  Budgets recorded for a follow-up: R2004 1849, R2010 6836, R2013 6844,
  R2018 5820. The records stay `Unhandled`.
- New `objects::color::ObjectColor` gives the four decoders one `CMC`
  reading that also honours the colour byte's two name-string bits.
- New probe `examples/probe_token_scan.rs` reports the self-identifying
  anchors (full-form `BD`s that decode to short decimals, `CMC`s whose
  true-colour word carries a real method octet) a field list has to
  reach exactly. `examples/probe_field_list.rs` and
  `examples/probe_object_layout.rs` now consume the R2013+ AcDs
  binary-data marker as 16 bits, matching `objects::modern`.

Measured coverage on the local 19-file corpus, before → after, both
runs on a tree that already carries the residual-entity fixes below:

| Version | Decoded | Skipped | Errored | Ratio |
|---|---|---|---|---|
| R2004 (AC1018) | 498 → 510 | 99 → 87 | 0 → 0 | 83.4 % → **85.4 %** |
| R2010 (AC1024) | 519 → 531 | 24 → 12 | 0 → 0 | 95.6 % → **97.8 %** |
| R2013 (AC1027) | 372 → 384 | 24 → 12 | 0 → 0 | 93.9 % → **97.0 %** |
| R2018 (AC1032) | 755 → 762 | 87 → 80 | 0 → 0 | 89.7 % → **90.5 %** |
| **Aggregate** | **2144 → 2187** | **234 → 191** | **0 → 0** | 90.2 % → **92.0 %** |

Every one of the 43 records moved from Unhandled to decoded, and the
corpus stays at zero errors.
### Fixed — the last 39 R2018 errors: MULTILEADER, HATCH and seven more field lists (2026-08-30, closes #57, closes #30, closes #29, refs #31, refs #54)

`sample_AC1032.dwg` now decodes with **zero errors**, and so does every
other file in the 19-file corpus. Every R2007+ decoder involved is
self-validating: its data fields must end exactly on the record's
data-stream boundary, so zero errors means zero known mis-reads.

- **The `strings present` trailer bit is not an entity field.** A record
  with no string stream still writes the one-bit flag that says so, and
  it is the last bit before the handle stream — so such a record's data
  fields end one bit *before* `string_stream::data_section_end`. All 20
  LWPOLYLINEs, the 3DFACE, the four INSERTs and the two SPLINEs of the
  sample carry no strings and every one of them ended exactly one bit
  short of the old boundary. Modelling that as a trailing `B` would have
  put a fabricated field at the end of four different field lists. New
  `string_stream::data_field_end`; `object::data_end_bit` and
  `tables::modern::open_entity` both route through it.
- **Object references consume no data-stream bits.** MULTILEADER read
  three `H` fields inline, HATCH read its per-path boundary handles
  inline, UNDERLAY read its definition handle inline. §20.4.75 is
  explicit for HATCH: the boundary handles are "Common Entity Handle
  Data", after the data stream, so the data stream carries only the
  `BL` count.
- **MULTILEADER embeds a whole `MLeaderAnnotContext` (§20.4.48 +
  §20.4.86).** Leader roots, leader lines, vertices, and the text or
  block content all live inside the MLEADER record. The inherited
  `AcDbAnnotScaleObjectContextData` / `AcDbObjectContextData` prefix is
  **not** written when embedded — the `BL` leader-root count follows the
  `BS 270` version directly — and the `-R2007` arrowhead / block-label
  block is genuinely absent on R2018, because in R2010+ the arrowhead
  moved into the leader line. All 15 records close on their
  string-stream start bit and recover their authored text from it:
  `"MLeader text, hello!"`, `"MULTILEADER TEST"`,
  `"MULTILEADER\PTEST\P123"`. **Not established:** 17 bits between
  `B 293 is annotative` and the R2010+ `BS 271` that §20.4.48 does not
  list — an `MC` (two bytes on all 15 records, holding only `274`, `530`
  and `786`) then a `B` (`true` on all 15). `B` + `RS` and `B` + two
  `RC`s consume the same bits; they are surfaced as
  `MLeader::undocumented_mc` / `undocumented_flag` rather than named.
- **The HATCH gradient block is unconditional and the flag is a `BL`.**
  §20.4.75 does not guard the gradient record behind `Is Gradient Fill`.
  All eight HATCH records carry two strings — `"LINEAR"` then the
  pattern name — even though six have the flag clear; returning early
  consumed one `TV` too few and shifted the entire boundary-path tree.
  Four more corrections from the same section: path points and seed
  points are `2RD`, a `BS pattern type` follows `BS style`, the whole
  pattern-definition block is written only `if (!solidfill)`, and the
  pixel size is a `BD` written only when some path flag has bit 4 set.
  `HatchPath::boundary_handles` becomes `num_boundary_handles`;
  `Hatch::pixel_size` becomes an `f64`; `Hatch::pattern_type` is new.
- **INSERT (§20.4.9):** the `BB` data flags select four different scale
  encodings (`RD` + `DD` forms), the extrusion is a `3BD`, and the `BL`
  owned-object count is written only when `has_attribs` is set. The old
  reading overran three of the four records and mis-scaled the fourth.
- **SPLINE (§20.4.40):** `Degree` is a `BL`, not a `BD`, and on R2013+
  the fit/control branch is **derived**, not read — "the scenario flag
  becomes 1 if the knot parameter is Custom or has no fit data,
  otherwise 2". Both records of the sample store `1`; one is a fit
  spline. `Spline::degree` is now an `i32`.
- **LWPOLYLINE (§20.4.85):** the flag bits are `0x001` normal, `0x002`
  thickness, `0x004` constant width (read *first*, before the counts),
  `0x008` elevation, `0x010` bulges, `0x020` widths, `0x400` vertex ids.
  19 of the sample's 20 records carry flag `0` or `0x200` and survive
  either reading; handle `0x4A6` carries flag `4` and does not. `0x200`
  is not one of the spec's presence bits and is read as the closed flag.
- **3DFACE (§20.4.32):** corners 2-4 are `3DD` triples defaulting to the
  previous corner, not `BD` offsets added to it, and all four corners
  are always present — only the invisible-edge `BS` is conditional.
- **IMAGEDEF (§20.4.81):** `Imgsize` and `Pixelsize` are `2RD`.
- **DIMENSION (angular 2-line, §20.4.27):** the `2RD` 16-point comes
  *first*, before the four `3BD` def points. `DIMENSION`'s split-stream
  decode also moves from `<= string_start` to exact equality.
- **LTYPE (§20.4.58):** the dash record is `BD` length, `BS` shape code,
  two `RD` offsets, `BD` scale, `BD` rotation, `BS` shape flag, and the
  512-byte text area is present only when some `shapeflag & 0x02`. The
  heuristic string-scanning fallback the modern path used is gone; it
  now goes through `tables::modern::open_table_entry` like every other
  table entry.
- **UNDERLAY:** normal, insertion point, rotation, scale — measured, not
  specified (the v5.4 spec has no UNDERLAY section). The old order put
  the scale on `(0, 0, 1)` and the normal on `(1, 1, 1)`. The clip
  vertex encoding is **not established**: the single record carries none.
- **MTEXT's R2018 tail — the 567 bits of #29 (§20.4.46).** A `B` "is
  NOT annotative" and, when set, a version, a default flag, a handle
  slot, a redundant copy of the record's own attachment point, axes,
  insertion point, rectangle and extents, then the column block. They
  repeat the record's own values because the spec says they are a
  redundant copy. MTEXT now asserts exact equality with the string-stream
  start instead of `<=`, and surfaces `is_annotative`, `column_type`,
  `column_width`, `column_gutter` and `column_heights`. The background
  block is also entered on the R2018 text-frame bit `0x10`.
- **ATTRIB / ATTDEF multi-line attributes (§20.4.4, §20.4.5).** An
  R2010+ `RC` version and an R2018+ `RC` attribute type sit between the
  shared TEXT body and the attribute's own fields; a multi-line
  attribute then embeds a whole MTEXT record "starting from the Entmode
  (entity mode)". Handle `0x79D` spends 683 data bits there and carries
  three strings — the empty TEXT value, the MTEXT text, and the tag.
  ATTDEF is ATTRIB plus its own `RC` version and a `TV` prompt; the lone
  unexplained `RC` the old ATTDEF field list carried was that version
  byte, and the two bytes §20.4.4 puts *before* the tag were missing
  entirely, which is why the old list closed on the four single-line
  ATTDEFs and failed on the multi-line one.
- **#54, measured not settled.** Exactly three *entity* records in the
  corpus set the R2013+ `has AcDs binary data` bit — 3DSOLID `0xD65`
  and `0xD6A`, REGION `0xD69` — and all three are ACIS pass-through
  decoders whose field lists this crate does not close, so they cannot
  arbitrate the marker width either. None of the 39 records fixed here
  sets the bit, so the reading did not matter for any of them.
- New probes: `examples/probe_decode_errors.rs` (every erroring record
  with its handle, body-start bit and data boundary) and
  `examples/probe_entity_field_list.rs` (walk a candidate token list
  against one *entity* record — the entity-side twin of
  `probe_field_list`, with `H` and `TV` slots consuming no data bits,
  plus `TEXTFIELDS` / `MTEXTFIELDS` / `ENTMODE` composites and an
  `--at=<bit>` start offset).
- `DecodedEntity::MLeader` is boxed; a MULTILEADER now carries its whole
  context inline and was 1120 bytes.

Measured on the 19-file local corpus, `examples/coverage_report.rs`:

| Corpus slice | Before (`fa7fcc8`) | After |
|---|---|---|
| R2004 (AC1018) × 3 | 498 / 99 / 0 / 83.4 % | 498 / 99 / 0 / 83.4 % |
| R2010 (AC1024) × 3 | 519 / 24 / 0 / 95.6 % | 519 / 24 / 0 / 95.6 % |
| R2013 (AC1027) × 3 | 372 / 24 / 0 / 93.9 % | 372 / 24 / 0 / 93.9 % |
| R2018 (AC1032) × 1 | 716 / 87 / 39 / 85.0 % | **755 / 87 / 0 / 89.7 %** |
| **Aggregate** | **2105 / 234 / 39 / 88.5 %** | **2144 / 234 / 0 / 90.2 %** |

The R2018 error histogram before this change was MULTILEADER 15, HATCH
8, INSERT 4, LTYPE 3, SPLINE 2, and one each of 3DFACE, LWPOLYLINE,
DIMENSION_ANG_2LN, ATTDEF, ATTRIB, IMAGEDEF and PDFUNDERLAY. It is now
empty.


### Fixed — the `AcDb:Handles` delta run: unsigned handle deltas, per-section restart (2026-08-30, closes #43, closes #44, closes #51)

- **The handle delta is an UNSIGNED modular char; only the offset delta
  is signed.** Reading it as a signed MC steals bit `0x40` of the
  terminating byte as a negation flag, so every single-byte delta in
  `0x40..=0x7F` decoded as a small negative number and the multi-byte
  form split its limbs one bit short. On `sample_AC1032.dwg` the first
  divergence is handle-map entry 272, from which point every recovered
  handle ran 108 low: the map claimed `0x438` at offset 148,422 where
  the record at that offset carries `0x4A4`.
- **Both accumulators restart at zero on every handle section.** The
  first `(handle, offset)` pair of a section is absolute. Carrying the
  running totals across the boundary threw the whole of this file's
  second handle section — entries 656..841, 186 of 842 — into a
  fabricated address space: entry 656 was read as handle `0x1343` at
  offset 197,439 when the bytes say handle `0xB23` at offset 99,522,
  and the drift produced the phantom offset 1,289,590 that #44 reported
  as a 96,739-byte shortfall against the section.
- **The section assembly was never wrong.** `AcDb:AcDbObjects` on that
  file is 41 pages of exactly `0x7400` decompressed bytes each
  (1,217,536 total) truncated to the declared 1,192,851; the trailing
  24,685 bytes are the last page's zero padding, which is why its
  compressed payload is only 3,317 bytes. After the fix the map's
  highest offset is 1,192,823 — 28 bytes inside the section — and all
  six previously out-of-range entries land in real records.
- **Verified against the file itself.** Every record repeats its own
  handle, so the object stream is a self-checking oracle for the map.
  Agreement on `sample_AC1032.dwg` goes from 272 / 842 to **842 / 842**,
  walker skips from 97 to 0, and the 842 walked records cover 1,191,935
  of the section's 1,192,851 bytes with 916 unclaimed (longest run: 4
  bytes) — so no unreferenced records are hiding outside the map.
  `ObjectWalkSummary::handle_mismatches` now reports the check on every
  walk, and `DwgFile::all_objects_lossy` exposes it.
- **`BitCursor::read_handle` rejects a counter above 8** (#51) with the
  new `Error::HandleCounterTooLarge`. The nibble can hold 0..=15 but a
  handle value is at most a `u64`, so a wider counter proves the cursor
  is not on a handle field — it previously shifted the excess out and
  produced a plausible handle from garbage. On the pre-fix address
  space this gate alone caught 43 phantom records on
  `sample_AC1032.dwg`. It surfaces as a walker skip, never as a decoder
  error, so coverage ratios stay comparable.
- The three phantom `CUSTOM(727)` records (#44) are gone, along with
  `CUSTOM(517)` at 212,392 and `CUSTOM(564)` at 229,599.
- Class-table census on `sample_AC1032.dwg` (#43), declared
  `num_objects` vs walked, before → after: MATERIAL 3 → 11 of 11,
  MULTILEADER 10 → 15 of 15, ACAD_EVALUATION_GRAPH 1 → 3 of 3,
  TABLECONTENT 1 → 2, TABLEGEOMETRY 1 → 2, and PDFDEFINITION,
  PDFUNDERLAY, SPATIAL_FILTER, ACSH_BOX_CLASS, ACDBASSOCPERSSUBENTMANAGER,
  ACDBPERSSUBENTMANAGER, ACSH_HISTORY_CLASS all 0 → their full declared
  count. 41 of 43 classes now reach their declared instance count;
  TABLECONTENT and TABLEGEOMETRY each declare 5 and reach 2, with no
  unclaimed stream bytes to hold the difference.
- **Coverage** (`examples/coverage_report.rs` over the local 19-file
  corpus): R2018 `sample_AC1032.dwg` 561 / 145 / 39 / 75.3 % →
  **712 / 91 / 39 / 84.6 %**; aggregate 1923 / 319 / 39 / 84.3 % →
  **2074 / 265 / 39 / 87.2 %**. R2004 (81.9 %), R2010 (93.9 %) and
  (Re-measured on `main` after #52 (LAYOUT) with this change merged:
  R2004 498 / 99 / 0 / 83.4 %, R2010 519 / 24 / 0 / 95.6 %, R2013
  372 / 24 / 0 / 93.9 %, R2018 716 / 87 / 39 / 85.0 %, aggregate
  **2105 / 234 / 39 / 88.5 %**.)
  R2013 (91.7 %) are unchanged — those samples carry a single handle
  section, so neither bug could fire.
- Writer side moves with the reader: `write_handle_map` emits unsigned
  handle deltas, restarts its baselines per section, and opens a new
  section when a caller's handles move backward (which an unsigned
  delta cannot encode).
- New probes: `examples/probe_section_pages.rs` (per-page section
  assembly audit — page-map row, data-page header, produced bytes,
  cumulative offsets, handle-map address-space cross-check) and
  `examples/probe_class_census.rs` (declared-vs-walked per class plus
  object-stream byte coverage). `coverage_report` gains a `wskip`
  column and a walker-diagnostic histogram kept separate from the
  dispatch error counts.
### Added — LAYOUT and PLOTSETTINGS decode and dispatch on R2004 and R2010+ (2026-08-30, closes #50, closes #39)

- **The field list is §20.4.84's, read straight.** LAYOUT is the one
  place the spec prescribes the plot-settings block: §20.4.84 opens
  with it — every row glossed `plotsettings …` — and continues into
  LAYOUT's own fields. There is no separate §20.4 PLOTSETTINGS entry,
  and no §19.6 chapter at all, so the "§19.6.12 (L6-12)" and "§19.6.6
  (L6-14)" citations the two modules carried are withdrawn along with
  the field lists that went with them. Both are replaced by one shared
  implementation (`acad_plot_settings::read_fields`), so the embedded
  block and a standalone PLOTSETTINGS record cannot drift apart.
- **Measured against the boundary on all 31 corpus records.** Each
  record's data fields must end exactly on the first bit of its string
  stream (R2010+) or on the `RL` object-data-size (R2004);
  `examples/probe_field_list.rs` prints the delta. The list closes with
  delta 0 on all 9 R2004 records, all 9 R2010, all 9 R2013 and all 4 on
  `sample_AC1032.dwg` — no band declines, and no bits are left over.
- **`viewport_count` is a `BL`, not the `RL` §20.4.84 prints.** On
  `sample_AC1032.dwg` handle 89 the field starts 10 bits before the
  string-stream start bit: a 32-bit `RL` cannot fit, and a `BL` in its
  8-bit form is exactly 10 bits and decodes `2` — the number of
  viewports that layout owns. The 28 records with no viewport spend 2
  bits there, where an `RL` would overrun the boundary by 30.
- **The R2013+ `has_ds_binary_data` flag carries 16 further bits.** The
  spec's common-object-data table gives only the `B`.
  `objects::modern` consumed a single `RC` after it — an unvalidated
  guess no test or corpus record had ever reached. Exactly four
  non-entity records in the corpus set the bit (the four LAYOUTs of
  `sample_AC1032.dwg`, against 327 that clear it), and on all four the
  field list closes only with **16** bits consumed, the first byte
  `0x2C` on every one. The alternative reading — that the marker is
  absent and LAYOUT gained two leading bytes in R2018 — is not ruled
  out by this corpus and is recorded in `src/objects/modern.rs`.
- **Corroborated by the decoded values.** `paper_width` × `paper_height`
  matches each record's own `paper_size` string (`215.9 × 279.4` mm for
  `ANSI_A_(8.50_x_11.00_Inches)` and `Letter_(8.50_x_11.00_Inches)`,
  `210 × 297` for `A4`); the four margins are `6.35` mm (0.25 in) on the
  `none_device` records and `≈4.23` mm on the HP LaserJet one; the
  paper layouts' `limmin`/`limmax` are the sheet less those margins in
  inches (`-0.25, -0.25 … 10.75, 8.25` on Letter; `-0.1667, -0.1667 …
  11.526, 8.101` on A4); every `Model` record carries AutoCAD's default
  `0, 0 … 12, 9`; `ucs_x_axis` is `(1,0,0)` and `ucs_y_axis` `(0,1,0)`
  on all 31; `extent_min`/`extent_max` hold AutoCAD's `±1e20`
  uninitialised sentinel on the empty `Layout1` records; `tab_order` is
  `0` on each `Model` and `1`/`2`/`3` on the paper tabs; and
  `scale_factor` equals `real_world_units / drawing_units` on every
  record (`0.38686… = 1 / 2.58489…` on the `Model` of
  `sample_AC1032.dwg`).
- **Pre-R2004 and R2007 decline rather than guess.** §20.4.84 inserts a
  `T plot_view_name` for R13-R2000 and omits the shade-plot triple and
  `viewport_count`; those files have no walkable object stream in this
  crate (#104), so that branch is undetermined. R2007's field list is
  presumably the R2004 one, but its string stream is not locatable here
  yet. Both return `Error::Unsupported`, which the dispatcher maps to
  `Unhandled`.
- `AcadLayout` now embeds an `AcadPlotSettings` and drops the five
  `Handle` fields it used to carry — §20.4.84's handle references live
  in the object's handle stream, past the data-stream boundary, so they
  were never fields of the data record. `AcadLayout::paper_min_limits`
  / `paper_max_limits` / `min_extents` / `max_extents` are renamed to
  the spec's `limmin` / `limmax` / `extent_min` / `extent_max`, and
  `paper_width()` / `paper_height()` become `limits_width()` /
  `limits_height()` (the paper size proper now lives on the embedded
  plot settings). `objects::acad_layout::decode` and
  `objects::acad_plot_settings::decode` are replaced by
  `decode_object`, which takes the raw payload so it can reach the
  string stream.
- `examples/dump_decoded_entities.rs` prints LAYOUT and PLOTSETTINGS
  values.

Measured on the 19-file local corpus, `examples/coverage_report.rs`:

| Corpus slice | Before (f706937) | After |
|---|---|---|
| R2004 (AC1018) × 3 | 489 / 108 / 0 / 81.9 % | **498 / 99 / 0 / 83.4 %** |
| R2010 (AC1024) × 3 | 510 / 33 / 0 / 93.9 % | **519 / 24 / 0 / 95.6 %** |
| R2013 (AC1027) × 3 | 363 / 33 / 0 / 91.7 % | **372 / 24 / 0 / 93.9 %** |
| R2018 (AC1032) × 1 | 561 / 145 / 39 / 75.3 % | **565 / 141 / 39 / 75.8 %** |
| **Aggregate** | **1923 / 319 / 39 / 84.3 %** | **1954 / 288 / 39 / 85.7 %** |

All 31 LAYOUT records of the corpus now decode; the error count is
unchanged.

### Fixed — the R2010+ entity graphics-preview block is sized by a `BLL` (2026-08-30, closes #42)

- **The common entity preamble's graphics-preview size is an `RL` up to
  R2007 and a `BLL` (§2.4) from R2010 on.** Reading it as an `RL` on
  R2018 produced preview sizes in the millions of bytes for records only
  a few hundred bytes long, and the preamble ran off the end of the
  record: `Bit cursor exhausted: wanted 8 bits, 3 bits remain`, before
  any entity-specific field. That was all 20 errors #41 newly surfaced
  on `sample_AC1032.dwg` — MULTILEADER (10), MESH (3), ACAD_TABLE (2),
  WIPEOUT, IMAGE.
- **Custom classes are a correlation, not a cause.** AutoCAD writes
  proxy graphics for entities whose class it does not implement
  natively, so the `graphics present` flag is set on exactly the
  custom-class records and clear on LINE / TEXT / MTEXT. The preamble
  itself is identical for both; the graphics branch was simply never
  exercised without one. Nothing in the class record — `was_a_proxy`,
  `is_an_entity`, proxy flags — changes the field list.
- **`3B` is three raw bits, not a stop-at-zero prefix code.** The old
  reading could only produce `{0, 2, 6, 7}`, a set with no way to say
  "one byte". The bytes require exactly that: the IMAGE at handle
  `0x662` and the WIPEOUT at `0x44D` both write `001` + `0x8C` = 140
  bytes of preview; MULTILEADER `0x66E` writes `010` + `0x50 0x03` =
  848; MESH `0x343` writes 9512. Every one of those lands the rest of
  the preamble on fields matching the values every plain entity in the
  same file carries (`entmode = 2`, `num_reactors = 0`,
  `no_xdictionary`, colour `0x0100`, linetype scale `1.0`,
  `invisibility = 0`, `lineweight = 0x1D`). `BitWriter::write_bll` now
  emits the exact byte count instead of rounding up to a representable
  prefix length.
- New probe `examples/probe_entity_preamble.rs` traces every entity's
  preamble under both readings and prints the bits left to the record's
  data boundary.

### Fixed — IMAGE / WIPEOUT / MESH bodies on R2018 (2026-08-30)

- **An IMAGE clip boundary of type 1 (rectangle) carries no vertex
  count.** The two corners follow the boundary type directly. The `BL`
  the decoder read there consumed the first 34 bits of the `-0.5`
  corner of `sample_AC1032.dwg`'s IMAGE, silently returning a wrong
  rectangle and overrunning the record. The polygon form (type 2) does
  carry the count — the WIPEOUT on the same file reads `4` there,
  followed by four `RD2` vertices that end one bit before its data
  boundary.
- **WIPEOUT stores a raster-image record.** The two records are
  bit-identical for their first 175 bits, and decoding the WIPEOUT with
  the IMAGE field list closes it exactly on its boundary. The previous
  WIPEOUT field list (`BL clip_state`, `BS` count, `BD2` vertices,
  `B show_clipped`, three `RC`s) matched no part of those bytes — it was
  decoding the graphics-preview payload as geometry.
  `entities::wipeout::Wipeout` is now an alias for
  `entities::image::Image` and `wipeout::decode` takes a `Version`.
- **A MESH's face-list count is a count of `BL` entries, not of
  faces**, and edge endpoints are `BL`, not `BS`. Both real MESH
  records now decode: 62 vertices / 336 face-list entries / 72 faces /
  132 edges and 64 / 320 / 64 / 128, with Euler characteristics 2 and 0
  — the independent check that the lists were read at the right
  offsets. The crease array has its own `BL` count. `decode` now
  rejects any face index or edge endpoint outside the vertex cage.
  Three bits before the data boundary are left unread and documented
  rather than guessed: both records hold `100` there.
- `Mesh::edges` is now `Vec<(u32, u32)>`.

### Changed

- `examples/coverage_report.rs` names the type in its error histogram,
  resolving custom classes through the file's `AcDb:Classes` table the
  way the unhandled histogram already did (closes #16).


### Added — ACAD_VISUALSTYLE decodes and dispatches on R2010+ (2026-08-30, #38)

- **The ODA spec carries no prescription for this object.** §20.4 runs
  from `20.4.1 Common Entity Data` to `20.4.104 XRECORD` and stops;
  VISUALSTYLE appears only in §20.3's list of non-fixed types. The two
  modules that claimed "§19.6.10 (L6-17)" (VISUALSTYLE) and "§19.6.9
  (L6-16)" (MATERIAL) were citing sections that do not exist; those
  citations are withdrawn.
- **The field list was derived from the boundary, not guessed.** Every
  record's data fields must end exactly on the first bit of its string
  stream. Searching for one token sequence over `B / BS / BL / BD / CMC`
  that lands **all 24** VISUALSTYLE records of `arc_2010.dwg` on that
  bit — with every `BD` a plausible double, every `CMC` true-colour word
  a real colour method, and every flag a `BS` in `0..=7` — returns
  exactly one answer: a fixed head (`TV` description, `BS` internal
  style type, `BS` 2, `B` internal-use flag) then 28 `(value, flag)`
  pairs. The same search on `arc_2013.dwg` returns exactly one answer
  too — the same 28 pairs followed by 30 more — and that R2013 sequence
  also closes on all 24 records of `sample_AC1032.dwg`.
- **The pairing explains the corpus's bit budgets.** A flag of `1` costs
  10 bits and a flag of `0` costs 2, so the records of one file differ
  only in multiples of 8: `arc_2010.dwg` measures 574 / 774 / 782 / 790
  / 798 / 806 / 854 / 862 bits, and the three internal styles that carry
  `0` in every flag (`JitterOff`, `OverhangOff`, `EdgeColorOff`) are
  exactly the 574-bit ones.
- **Corroborated by the decoded values.** `is_internal_use_only` — the
  lone `B` of the fixed head — comes out `false` on exactly ten of the
  24 records (`2dWireframe`, `Wireframe`, `Hidden`, `Conceptual`,
  `Realistic`, `Shades of Gray`, `Sketchy`, `X-Ray`, `Shaded with
  edges`, `Shaded`), which is precisely the set AutoCAD's Visual Styles
  Manager lists; `face_opacity` is `0.6` everywhere but `X-Ray`
  (`0.5`); `face_mono_color` is `0xC2FFFFFF` everywhere but
  `ColorChange` (`0xC2808080`); `edge_crease_angle` is `179` on
  `Conceptual` and `40` on `Hidden` / `Sketchy` / `Shades of Gray`;
  `internal_style_type` is a dense `0..=27` in ship order.
- **R2004 / R2007 decline rather than guess.** Those records store the
  same styles without the per-property flags and with a different
  property count, and no sequence over the same token set lands their
  24 records on the `RL` object-data-size boundary. `decode_object`
  returns `Error::Unsupported` there, and the dispatcher now maps an
  `Unsupported` from an object-class decoder to `Unhandled` rather than
  `Error` — "this release is not determined" is not a broken record.
- **`examples/probe_field_list.rs`** measures a candidate field list
  against one real record and prints the delta from the boundary;
  `examples/dump_decoded_entities.rs` prints VISUALSTYLE values.

### Changed — ACAD_MATERIAL withdraws its unverified field list (2026-08-30, #38)

- The previous `BL ambient_color_method, BS ambient_color, BD …` prefix
  was wrong from its first field: on `arc_2004.dwg` handle 17 (the
  `ByLayer` material) it decodes `ambient_color_method = 542113793` and
  `ambient_color = 17728`, then eight consecutive `BD = 1.0`.
- `objects::acad_material` now exposes `read_strings`, which reads the
  name and description from the R2007+ string stream (they were being
  read inline, i.e. from the wrong stream, on every R2007+ file), the
  further string-stream entries an R2018 record carries, and the
  **measured** data-field budget: 1284 bits on R2010 and R2013
  (bit-identical across all three records of each file), 1264 bits after
  the two inline `TV`s on R2004, 516 / 516 / 1028 on R2018.
- What is known about the interior is documented rather than
  implemented: twelve `BD` slots decode to exactly `1/48` at
  data-stream bits 46, 120, 250, 324, 510, 584, 706, 780, 900, 974,
  1096 and 1170 — six pairs, one per texture map, 74 bits apart — and
  the R2018 record's seven string-stream entries corroborate six map
  slots. The per-map block layout is not pinned, so MATERIAL stays
  undispatched and its 30 corpus records stay `Unhandled`.

Measured on the 19-file local corpus, `examples/coverage_report.rs`:

| Corpus slice | Before (0914187) | After |
|---|---|---|
| R2004 (AC1018) × 3 | 489 / 108 / 0 / 81.9 % | 489 / 108 / 0 / 81.9 % |
| R2010 (AC1024) × 3 | 438 / 105 / 0 / 80.7 % | **510 / 33 / 0 / 93.9 %** |
| R2013 (AC1027) × 3 | 291 / 105 / 0 / 73.5 % | **363 / 33 / 0 / 91.7 %** |
| R2018 (AC1032) × 1 | 533 / 166 / 46 / 71.5 % | **557 / 142 / 46 / 74.8 %** |
| **Aggregate** | **1751 / 484 / 46 / 76.8 %** | **1919 / 316 / 46 / 84.1 %** |

Re-measured on `main` after #46 (graphics-preview BLL fix) with this change merged: R2018 561 / 145 / 39 / 75.3 %, aggregate **1923 / 319 / 39 / 84.3 %**.

168 of the corpus's 240 VISUALSTYLE records now decode — the 72 on the
three R2004 files are the remainder.

### Fixed — the R2018 `AcDb:Classes` record layout (2026-08-30, closes #37)

- **`dwg_version` and `maintenance_version` are `BL`, not `BS`.** The
  ODA v5.4.1 spec states the class record twice and the two statements
  disagree: §10.2 (R18+) gives `BS max_class_number` + `RC 0x00` +
  `RC 0x00`, `BS dwg_version`, `BS maintenance_version`; §5.8 (R2007)
  gives `BL` for all three. The bytes side with §5.8. `BS` and `BL`
  are bit-identical on tags `10` (2 bits, zero) and `01` (10 bits, one
  byte) and differ only on tag `00`, where `BS` takes an 18-bit `RS`
  and `BL` a 34-bit `RL` — so the §10.2 reading survives every file
  whose classes keep both version fields under 256.
- **`sample_AC1032.dwg` is the first file that does not.** Four of its
  classes — MLEADERSTYLE (508), ACDBDETAILVIEWSTYLE (516), WIPEOUT
  (520), MULTILEADER (526) — record `dwg_version = 33`,
  `maintenance_version = 329`, making those records 113 bits wide
  where every other record on the file is 57-89. Reading the 329 as a
  `BS` lost 16 bits at record 8; record 9 then hit the reserved `11`
  `BL` tag, the consecutiveness check discarded the whole table, and
  all 194 `Custom(N)` objects on the file fell through to
  `Unhandled`. All 50 classes (500..=549) now decode.
- **Corroborated three ways.** With the corrected list, exactly
  `max_class_number - 500 + 1` records decode with consecutive class
  numbers on `arc_2010` / `arc_2013` / `sample_AC1032`; the `3 × N`
  strings that follow end precisely at the section's §19.4.1
  string-stream trailer (length words 7788 / 7038 / 45804 bits, each
  equal to the measured span); and the `num_objects` field the record
  tail now carries matches the object walk exactly on the high-count
  classes (VISUALSTYLE 240/240, SCALE 186/186, MLEADERSTYLE 11/11,
  TABLESTYLE 10/10). Evidence table in `ARCHITECTURE.md` §7e.
- **`ClassDef` gained `num_objects`, `dwg_version` and
  `maintenance_version`**, and `write_class_map` now round-trips them
  instead of writing zeros — the writer stays the exact inverse of the
  parser, including the oversized-`BL` case, which a new
  BitWriter-built test pins on both layouts.
- **`examples/probe_class_layout.rs`** is the reproducer: per-field bit
  offsets, record widths, resolved class names and the string-stream
  trailer arithmetic for any R2004+ file.
- **`examples/coverage_report.rs`** now names custom classes in its
  unhandled histogram (`VISUALSTYLE (custom class)` rather than
  `CUSTOM(502) (0x01F6)`); a class number is per-file, so the name is
  the only meaningful aggregate key.

Measured on the 19-file local corpus, `examples/coverage_report.rs`:

| Corpus slice | Before | After |
|---|---|---|
| R2004 (AC1018) × 3 | 489 / 108 / 0 / 81.9 % | 489 / 108 / 0 / 81.9 % |
| R2010 (AC1024) × 3 | 438 / 105 / 0 / 80.7 % | 438 / 105 / 0 / 80.7 % |
| R2013 (AC1027) × 3 | 291 / 105 / 0 / 73.5 % | 291 / 105 / 0 / 73.5 % |
| R2018 (AC1032) × 1 | 488 / 231 / 26 / 65.5 % | **533 / 166 / 46 / 71.5 %** |
| **Aggregate** | **1706 / 549 / 26 / 74.8 %** | **1751 / 484 / 46 / 76.8 %** |

65 R2018 records left *skipped*: 45 now decode (its 33 SCALE among
them) and 20 now reach a decoder and fail — MULTILEADER (10), MESH
(3), ACAD_TABLE (2), plus WIPEOUT, IMAGE, IMAGEDEF, ACDBDICTIONARYWDFLT
and DICTIONARYVAR — almost all with "Bit cursor exhausted" inside the
common entity preamble, i.e. the R2007+ preamble gap tracked under
#103. Those 20 were always broken; the class table was hiding them.

### Fixed — non-entity objects were unreachable and read `TV` inline (2026-08-30, #103, closes #33, closes #32)

- **No non-entity object was dispatched at all.** `decode_from_raw`
  short-circuited every object that is neither a drawing entity nor a
  symbol-table entry straight to `Unhandled`, so the 65 DICTIONARY records
  of `sample_AC1032.dwg` — and every XRECORD, `*_CONTROL` and
  ACDB_PLACEHOLDER in the corpus — counted as "skipped" no matter how good
  the decoder behind them was. `entities::dispatch::dispatch_object` now
  routes them, and `decode_from_raw_with_class_map` routes the custom
  classes among them by DXF class name.
- **New `src/objects/modern.rs`** — the object analogue of
  `tables/modern.rs`. One field list per decoder serves both layouts:
  `TV` comes from the object's string stream on R2007+ and inline before
  that, and `ObjectStream::finish` asserts the data fields end *exactly*
  on the record's data-stream boundary (the string-stream start on
  R2010+, the handle-stream start minus the one-bit §19.1 trailer when a
  record holds no strings, the `RL` object-data-size on R2000-R2007). A
  wrong field list is an error, never a plausible-looking struct.
- **The common object data of §19.4.2 keeps its `BL num_reactors` on
  R2007+.** Measured three ways: DICTIONARY's `BL numitems` then equals
  the number of strings its string stream actually holds (23 / 4 / 2 / 0
  across `sample_AC1032.dwg`); ACDB_PLACEHOLDER, whose body is provably
  empty, closes exactly; and the nine one-field `*_CONTROL` records close
  on their `BL numentries`.
- **DICTIONARY's entry names are a block, not name/handle pairs.** The
  value handles are handle references past the end of the data stream, so
  `Dictionary::entries: Vec<DictionaryEntry>` became
  `Dictionary::keys: Vec<String>`, whose index is the index of the
  matching handle. Verified on `arc_2004.dwg` handle 16 (`ByBlock`,
  `ByLayer`, `Global`) and on all 65 R2018 records.
- **ACAD_SCALE read the wrong field list.** The wire is
  `BS version, TV name, BD paper_units, BD drawing_units, B is_unit_scale`,
  not `TV, BD, BD, BS`. `arc_2004.dwg`'s `1:1` record closes exactly on
  its `RL` boundary with the new list; `arc_2013.dwg`'s `1:2` record
  decodes `paper = 1.0, drawing = 2.0` and `is_unit_scale = false`, and
  its `1:1` sibling `true`.
- **ACAD_GROUP read two `B` flags where the wire has two `BS`.** Both
  GROUP records of `sample_AC1032.dwg` leave exactly 30 bits for three
  10-bit fields; the boolean reading ran the trailing `BL` off the end of
  the record, which is why both failed to decode.
- **DIMSTYLE_CONTROL carries one `RC` the other nine controls do not** —
  measured at 8 bits past `num_entries` on `arc_2004.dwg`, `arc_2013.dwg`
  and `sample_AC1032.dwg`. Surfaced verbatim as
  `Control::dimstyle_trailing_rc` rather than named after a guess.
- **New decoders**: `objects::placeholder` (ACDB_PLACEHOLDER) and
  `objects::dictionary_var` (DICTIONARYVAR).
- **`ClassMap::parse` returned an empty table on every real file.** It
  started the bit stream at byte 24 for every release and read
  `max_class_number` as a byte-aligned `RL`. The class list actually
  begins at byte 20 on R2004 and byte 28 on R2010+ (two extra `RL`s), and
  opens with a `BS max_class_number`. R2007+ additionally splits each
  record: all the non-string fields first, then `3 × N` strings as a
  block. With that fixed, `arc_2004.dwg` / `arc_2010.dwg` /
  `arc_2013.dwg` resolve 10 / 10 / 9 classes whose names and object
  counts match their object streams; before, no `Custom(N)` object in any
  file could be dispatched. The writer `write_class_map` was rewritten as
  the true inverse and round-trips in both layouts.
- **A non-entity custom class no longer errors on the entity preamble.**
  A class whose `item_class_id` is `0x1F3` and that has no object decoder
  returns `Unhandled` instead of being fed to `read_common_entity_data`.
- **LIGHT's private `read_tv` ignored its `version` argument** and always
  read 8-bit characters inline (#32). LIGHT exists only from R2007 — the
  release that moved `TV` into the string stream — so it was doubly
  wrong. LIGHT, GEODATA (five `TV`s, including a full WKT/PROJ string)
  and IMAGEDEF now read through the split stream, each self-validating.

Measured coverage on the 19-file `samples/` corpus
(`cargo run --release --example coverage_report -- <corpus>`):

| Version | Before (dec / skip / err / rate) | After (dec / skip / err / rate) |
|---------|----------------------------------|---------------------------------|
| R2004 (AC1018) × 3 | 75 / 522 / 0 / 12.6 % | **489 / 108 / 0 / 81.9 %** |
| R2010 (AC1024) × 3 | 69 / 474 / 0 / 12.7 % | **438 / 105 / 0 / 80.7 %** |
| R2013 (AC1027) × 3 | 69 / 327 / 0 / 17.4 % | **291 / 105 / 0 / 73.5 %** |
| R2018 (AC1032) × 1 | 384 / 337 / 24 / 51.5 % | **488 / 231 / 26 / 65.5 %** |
| **Aggregate** | **597 / 1660 / 24 / 26.2 %** | **1706 / 549 / 26 / 74.8 %** |

The two extra errors on the R2018 sample are the second STYLE_CONTROL and
the second DIMSTYLE_CONTROL record, whose bytes are not a control object
at all; they were previously counted as "skipped" without anything having
tried to read them.

- **`examples/probe_object_layout.rs`** (new) — prints, for every
  non-entity object, the bit budget between the end of its common object
  data and its data-stream boundary under both candidate prefixes, plus
  the raw bits and the strings its string stream holds. This is the
  instrument behind every "measured" claim above.
- **`examples/coverage_report.rs`** now also prints an unhandled
  histogram by object kind, so the next-largest lever is visible from the
  report itself.


### Fixed — MTEXT and friends decoded silently wrong on R2007+ (2026-08-30, #103, closes #25, closes #26)

- **`MTEXT` (0x2C) returned a garbage one-character string for every record.**
  It read its `TV` from the data cursor, which on R2007+ holds the *next data
  field*, so it never errored — the coverage report counted every record as
  decoded. `src/entities/mtext.rs` now has a `decode_modern_split_stream`
  that reads the text from the object's string stream. Verified on
  `sample_AC1032.dwg`: 33 records recover real text
  (`"this is a Mtext\nwith multiple lines in it"`, `"Table sample"`,
  `"4'-0{\\H1x;\\S1#4;}\""`, a 3,436-character paragraph).
- **Self-validating:** MTEXT's only `TV` is its text, so its string stream must
  hold exactly one string and nothing else. Measured across all 22 MTEXT
  records in the main object stream: the stream length equals that string's
  encoded length to the bit (74 of 74, 202 of 202, 282 of 282, 666 of 666,
  54,978 of 54,978). The decoder requires the reader to be exhausted after one
  `read_tv`, and the data fields not to run past the string-stream start bit.
- **`background_scale_factor` is a `BD`, not a `BL`** (§19.4.44). The one
  record in the corpus with the background bit set (handle `0x6D8`) recovers
  `1.25`; reading a `BL` left the following `CMC` on a 16-bit code and then a
  reserved `11` pattern.
- **`TOLERANCE` (0x2E)** ported to the split stream, fully self-validating —
  its `text_string` is the last data field, so the data fields must end
  *exactly* on the string-stream start bit. Measured: all three records span
  exactly 146 bits of body, parsing as three `BD3` triples with nothing left
  over, which also shows the `BS unknown_short` / `BD height` / `BD dimgap`
  fields are absent on R2018. 3 errors → 0, recovering the GD&T frames.
- **`DIMENSION` (0x14-0x1A)** and **`HATCH` (0x4E)** now take their `TV`
  fields (`user_text`, pattern and gradient names) from the string stream, so
  every field after them keeps its alignment. DIMENSION: 10 errors → 2.
- **`StringReader::empty`** covers R2007+ records whose *strings present*
  trailer bit is clear: the `TV` slots exist but hold nothing and still
  consume no data-stream bits, so a decoder must read through an empty string
  stream rather than fall back to the inline layout.
- **`Layer::is_locked` tested the wrong bit (#26).** The DWG `values` word is
  not the DXF group-70 flag word: DXF puts locked at `0x04`, DWG puts *frozen
  in new viewports* there and locked at `0x08`. Every layer frozen in new
  viewports read as locked and every locked layer read as unlocked.
  `is_locked` now tests `0x08` and the new `is_frozen_in_new_viewports` tests
  `0x04`.

### Added

- **`examples/audit_string_streams.rs`** — census of which object types carry
  `TV` fields in their R2007+ string stream, and what those strings are. This
  is the instrument behind the #25 audit table: a type with strings here whose
  decoder reads `TV` inline is silently wrong on that file.
- **`examples/probe_mtext_fields.rs`** — per-field bit walk of an R2007+ MTEXT
  against the string-stream start bit.

Measured coverage on the 19-file `samples/` corpus:

| Version | Before (dec / err / rate) | After (dec / err / rate) |
|---------|---------------------------|--------------------------|
| R2004 (AC1018) × 3 | 75 / 0 / 12.6 % | 75 / 0 / 12.6 % |
| R2010 (AC1024) × 3 | 69 / 0 / 12.7 % | 69 / 0 / 12.7 % |
| R2013 (AC1027) × 3 | 69 / 0 / 17.4 % | 69 / 0 / 17.4 % |
| R2018 (AC1032) × 1 | 374 / 34 / 50.2 % | **384 / 24 / 51.5 %** |
| **Aggregate** | **587 / 34 / 25.7 %** | **597 / 24 / 26.2 %** |


### Fixed — R2004 (AC1018) common object header (2026-08-30, #103, closes #24)

- **`RL` object data size in bits was read for R2000 only.** ODA v5.4.1 §19.1
  puts that field between the object type and the object handle for the whole
  **R2000..R2007** band; R2010+ replaces it with the leading `MC`
  handle-stream size. Reading it for R2000 alone left every AC1018 and AC1021
  record 32 bits out of phase from the handle onwards, which is what produced
  the `Bit cursor exhausted: wanted 8 bits, N bits remain` failures on all
  three R2004 samples. Fixed in `object::ObjectWalker::read_one_at_pos` and
  `entities::dispatch::position_cursor_at_entity_body` behind the new
  `Version::has_object_size_field()` predicate.
- **The value is now surfaced as `RawObject::obj_size_bits`** — the bit at
  which an object's data stream ends and its handle stream begins, i.e. the
  pre-R2010 analogue of the R2007+ string-stream start bit, and therefore a
  decoder's self-check. Measured: the `line_2004.dwg` LINE reports
  `obj_size = 347` and its LINE body ends at bit 347 exactly, recovering the
  authored `(50, 50, 0) -> (100, 100, 0)` geometry.
- **Non-entity objects were missing their common object data (§19.4.2).** The
  pre-R2007 symbol-table decoders started reading at the entry name with the
  cursor still parked on the EED chain. `common_entity::read_common_object_data`
  now consumes `EED + BL num_reactors + B xdic-missing (R2004+) + B
  has-DS-binary-data (R2013+)` first. Measured with
  `examples/probe_r2004_object_prefix.rs`: only that prefix yields readable
  names — `0`, `Standard`, `Annotative`, `ACAD`, `AcadAnnotative`, `ByBlock`,
  `ByLayer`, `Continuous` — where every other candidate produced an empty
  string or a 65-73 character `TV` length.
- **`STYLE` legacy decoder read an `RC` where the field table has two `B`
  bits** (shape file, vertical), overrunning each record by 6 bits.
- **`VPORT` legacy decoder followed an abbreviated field list** (`2 × BD`
  view centre, `BS` view mode, no UCS or grid tail). It now reads the same
  field table as the already-verified R2007+ split-stream decoder minus the
  R2007+ lighting/ambient-colour block and the trailing grid words, which is
  the only list that consumes the `*Active` record's 1127 body bits exactly.
- **Fixture regenerated.** `examples/build_fixtures.rs` now emits the `RL`
  for the R2000..R2007 band (two-pass, since the field is fixed width but its
  value depends on the body), so `tests/fixtures/canonical/synthetic_2004.dwg`
  changed. The other three fixtures are byte-identical and the
  `tests/canonical_corpus.rs` value pins are unchanged.

Measured coverage on the 19-file `samples/` corpus:

| Version | Before (dec / err / rate) | After (dec / err / rate) |
|---------|---------------------------|--------------------------|
| R2004 (AC1018) × 3 | 15 / 60 / 2.5 % | **75 / 0 / 12.6 %** |
| R2010 (AC1024) × 3 | 69 / 0 / 12.7 % | 69 / 0 / 12.7 % |
| R2013 (AC1027) × 3 | 69 / 0 / 17.4 % | 69 / 0 / 17.4 % |
| R2018 (AC1032) × 1 | 374 / 34 / 50.2 % | 374 / 34 / 50.2 % |
| **Aggregate** | **527 / 94 / 23.1 %** | **587 / 34 / 25.7 %** |


### Added — R2007+ object string stream (2026-08-30, #103, closes #14, closes #15)

- **`src/string_stream.rs` (new)** — Locates the per-object string stream that
  R2007 and later use for every `TV` field (ODA v5.4.1 §19.1). The trailer
  rule (strings-present bit, 16-bit size, optional high word) is anchored at
  `payload_bits - handle_stream_bits + mc_field_bits`; that `mc_field_bits`
  correction was **measured** over 59 objects across 11 object types in
  `sample_AC1032.dwg` (R2018), `line_2013.dwg` (R2013) and `arc_2010.dwg`
  (R2010) with zero deviation.
- **`examples/probe_string_stream.rs` (new)** — Reproducible probe that
  brute-forces the trailer and prints the deviation from the predicted end, so
  the rule above is independently checkable from bytes.
- **`src/tables/modern.rs` (new)** — Shared split-stream plumbing for
  symbol-table entries. Its central invariant: a record's data fields must end
  *exactly* on the string-stream start bit, so a mis-read layout errors instead
  of returning plausible-looking garbage.
- **Ported to the split stream:** `STYLE` (0x35), `APPID` (0x43), `UCS`
  (0x3F), `VIEW` (0x3D), `VPORT` (0x41), `LAYER` (0x33), `DIMSTYLE` (0x45),
  plus the `TEXT` (0x01), `ATTRIB` (0x02) and `ATTDEF` (0x03) entities. Each
  has a `BitWriter`-built unit test of the R2007+ layout; the pre-R2007 inline
  decoders are untouched.

### Fixed — R2007+ LAYER decoded silently wrong values (2026-08-30, #103)

- `LAYER` never appeared in the error histogram because it never errored: on
  R2007+ files it read its name from the data stream and produced
  `"\u{252}v\0\0..."` for every layer, with flags, lineweight and colour to
  match. It now reads through the string stream. The result checks out against
  a corpus whose layers are named for their state — `Layer_Freeze` sets
  `values` bit `0x01`, `Layer_Off` `0x02`, `Layer_vp_freeze` `0x04`,
  `Layer_Lock` `0x08`; `Layer_NoPlot` and `Defpoints` clear the plot bit
  `0x10`; `Layer_lw_035` reads lineweight index 9 and `Layer_lw_050` index 11;
  `Layer_color_80` reads ACI 80 while `Layer_true_color` reads none.

### Reverse-engineering findings (2026-08-30)

- **Table-entry field order.** With the name moved to the string stream, the
  three shared fields read `B 64-flag`, `B xdep`, `BS xrefindex+1` — not
  `B, BS, B`. Only that order keeps the `BS` on a 2-bit `10` code across every
  STYLE record in `sample_AC1032.dwg`.
- **Full `CMC` colour form.** VIEW's ambient colour, VPORT's, LAYER's and
  DIMSTYLE's four colours all write `BS` index + `BL` true-colour + `RC` byte
  unconditionally, even when the index carries no flag bits. The `BL` top byte
  selects the interpretation: `0xC3` is an ACI index in the low byte, `0xC2` a
  literal RGB (VIEW's ambient reads `0xC2333333`).
- **The R2013+ "has AcDs binary data" bit is followed by an `RC`.** The four
  DIMSTYLE records in `sample_AC1032.dwg` with the bit clear parse with a
  4-bit object prefix; the two with it set (`ISO-25`, `custom_dim_style`) need
  exactly 8 more bits.
- **TEXT's height is a raw `RD`, not a `BD`.** Every TEXT in the R2018 sample
  carries `DataFlags 0xFF`; the height reads a clean `1.0` only as a 64-bit
  `RD` with no type code in front of it.
- **Blocked — VPORT #697 of `sample_AC1032.dwg`** (handle `0x1588`, 515 bytes)
  reports type code `0x41` but carries no string stream at all. Left erroring
  rather than guessed at.
- **Blocked — multi-line attributes.** R2010 multi-line ATTRIB/ATTDEF embed a
  whole MTEXT record between the TEXT body and the tag (`MULTI_LINE_ATT` in
  the R2018 sample carries three strings where a single-line ATTRIB carries
  two). Not modelled; those two records report their misalignment.
- **Unverified — UCS.** No UCS record exists anywhere in the sample corpus, so
  the ported layout has never been confirmed on real bytes. The string-stream
  invariant makes a wrong layout error rather than invent values.

### Measured coverage effect

`cargo run --release --example coverage_report -- <samples>` on the 19-file
corpus:

| Metric | Before | After |
|---|---|---|
| Aggregate decoded / skipped / errored | 458 / 1660 / 163 | **527 / 1660 / 94** |
| Aggregate ratio | 20.1 % | **23.1 %** |
| `sample_AC1032.dwg` (R2018) | 329 / 337 / 79 — 44.2 % | **374 / 337 / 34 — 50.2 %** |
| R2013 files (each) | 20 / 109 / 3 — 15.2 % | **23 / 109 / 0 — 17.4 %** |
| R2010 files (each) | 18 / 158 / 5 — 9.9 % | **23 / 158 / 0 — 12.7 %** |
| R2004 files (each) | 5 / 174 / 20 — 2.5 % | 5 / 174 / 20 — 2.5 % (unchanged) |

Per-type errors: TEXT 25 → 0, STYLE 21 → 6, DIMSTYLE 20 → 6, VPORT 11 → 4,
ATTDEF 5 → 1, ATTRIB 4 → 1. The R2010 and R2013 sample files now decode with
zero errors; 60 of the 94 remaining errors are the three R2004 files, whose
object-header alignment is a separate bug.

### Security — crossbeam-epoch 0.9.18 to 0.9.20 clears a RUSTSEC advisory (2026-08-30, #12)

The lockfile's transitive `crossbeam-epoch` (reached through `criterion`) moves past the RUSTSEC advisory that made `cargo deny check` fail on every open Dependabot PR. The change is the lockfile alone: no code or API of this crate changes, and `cargo deny check` passes (#12).

### Fixed — R2007+ LTYPE string-stream names (2026-04-29, #103)

- **`src/tables/ltype.rs` / `src/entities/dispatch.rs`** — Added a modern
  `LTYPE` path for R2007+ table records. It reads entry names/descriptions
  from the split string stream, parses the non-string fields in ODA v5.4.1
  §19.5.3 order, and falls back to the legacy inline decoder if the modern
  parse is not plausible.
- **`examples/dump_decoded_entities.rs`** — Print decoded `LTYPE` records so
  recovered linetype names are visible during corpus inspection.
- **`tests/r2013_entity_values.rs`** — Added a real-DWG regression asserting
  that `sample_AC1032.dwg` recovers `ByBlock`, `ByLayer`, and `Continuous`,
  including the `Continuous` description `Solid line`.
- **Measured effect after the BLOCK_HEADER fix:** aggregate coverage improved
  from 437 decoded / 1,660 skipped / 184 errored / 19.2% to 458 decoded /
  1,660 skipped / 163 errored / 20.1%. `sample_AC1032.dwg` improved from
  326 / 337 / 82 / 43.8% to 329 / 337 / 79 / 44.2%, and `LTYPE` errors
  dropped from 30 to 9.

### Fixed — R2007+ BLOCK_HEADER string-stream names (2026-04-29, #103)

- **`src/tables/block_record.rs` / `src/entities/dispatch.rs`** — Added a
  modern `BLOCK_HEADER` path that reads R2007+ table-record names from the
  object's split string stream instead of treating `TV` fields as inline data.
  This follows the ODA v5.4.1 §19.1 rule that Unicode strings in modern objects
  live in the string stream even when the object table lists `TV` fields among
  normal data.
- **`examples/dump_decoded_entities.rs`** — Print `BLOCK_RECORD` names so the
  recovered symbol-table records are visible during corpus inspection.
- **`tests/r2013_entity_values.rs`** — Added a real-DWG regression asserting
  that `sample_AC1032.dwg` recovers core block-record names including
  `*Model_Space`, `*Paper_Space`, `_ArchTick`, `MyBlock`, `my_block`,
  `my_block_v2`, and `my-dynamic-block`.
- **Measured effect after the LWPOLYLINE fix:** aggregate coverage improved
  from 408 decoded / 1,660 skipped / 213 errored / 17.9% to 437 decoded /
  1,660 skipped / 184 errored / 19.2%. `sample_AC1032.dwg` improved from
  312 / 337 / 96 / 41.9% to 326 / 337 / 82 / 43.8%, and `BLOCK_HEADER`
  errors dropped from 39 to 10.

### Fixed — LWPOLYLINE DD vertex decoding (2026-04-29, #103)

- **`src/entities/lwpolyline.rs`** — Decode LWPOLYLINE vertices as
  first point `RD/RD`, then subsequent points as `DD/DD` using the
  previous point as the default. The old decoder read every vertex as
  raw doubles, which produced enormous coordinates and exhausted the
  stream on common AC1032 rectangles.
- **`tests/r2013_entity_values.rs`** — Added a real-DWG regression
  asserting that `sample_AC1032.dwg` decodes at least 10 finite,
  nondegenerate LWPOLYLINE bodies.
- **Measured effect after the common-entity fix:** aggregate coverage
  improved from 400 decoded / 1,660 skipped / 221 errored / 17.5% to
  408 decoded / 1,660 skipped / 213 errored / 17.9%.

### Fixed — R2013/R2018 common-entity/body boundary (2026-04-29, #103)

- **`src/common_entity.rs`** — Corrected the modern common-entity
  preamble layout against traced R2013/R2018 data: removed the stale modern
  `is_on_layer` / `non_fixed_ltype` data-stream reads, limited the
  DS-data bit to R2013+, restored `shadow_flags` to an `RC`, and
  skipped R2004+ CMC color suffixes (`alpha`, `rgb`, color name, and
  book name) without consuming color handles from the data stream.
- **`examples/trace_entity_boundary.rs`** — Added a reusable boundary
  tracer for LINE/CIRCLE/ARC records. It compares the production
  common-entity end against plausible body starts and reports duplicate
  handles / no-candidate records for real DWGs.
- **`examples/trace_common_entity.rs` /
  `examples/test_line_bd_vs_rd.rs`** — Brought the forensic examples
  back in sync with the corrected R2013 preamble so they no longer
  encode the obsolete 68-bit overread.
- **`tests/r2013_entity_values.rs`** — Promoted the real-DWG value
  tests out of `#[ignore]`. `line_2013.dwg` now asserts the authored
  `(50, 50, 0) -> (100, 100, 0)` LINE, the R2013 CIRCLE/ARC samples
  must decode typed geometry, and `sample_AC1032.dwg` must retain at
  least 80 nondegenerate LINE bodies.
- **Measured effect:** `examples/coverage_report.rs ../../samples`
  improved from 169 decoded / 1,655 skipped / 457 errored / 7.4%
  aggregate coverage to 400 decoded / 1,660 skipped / 221 errored /
  17.5%. `sample_AC1032.dwg` improved from 106 decoded / 332 skipped /
  307 errored / 14.2% to 304 decoded / 337 skipped / 104 errored /
  40.8%.

### Fixed — LINE DD coordinate decoding (2026-04-28, #103)

- **`src/bitcursor.rs` / `src/bitwriter.rs`** — Added `DD`
  (bitdouble-with-default) support. The reader handles the spec's
  default, 4-byte patch, 6-byte patch, and full-double forms; the writer
  emits exact-default or full-double forms.
- **`src/entities/line.rs` / `src/element_encoder.rs`** — Decode and encode
  `LINE` end coordinates as `DD` fields using each start coordinate as
  the default, instead of treating them as `BD` deltas.
- **Measured effect before the common-entity boundary fix:** decoded
  count moved from 166 to 169 on the local sample corpus and reduced
  R2018 LINE errors from 83 to 80. The 2026-04-29 common-entity fix
  above is the change that made the real-DWG value tests pass.

### Added (breaking) — `DwgFile::to_bytes`, `dwg-write --dwg`, and a signed modular char that overflows `i64` is an error (2026-04-25, ce4bad0)

- **`DwgFile::to_bytes`** re-assembles a file's readable sections into an R2004-family DWG byte buffer through `file_writer::assemble_dwg_bytes`. This crate's reader round-trips the sections, and no external CAD application is run in CI, so it is experimental. It returns `Error::Unsupported` for R14, R2000 and R2007, and `Error::SectionMap` when the section list came from a fallback or deferred parse.
- **`dwg-write --dwg <path>`** writes that assembled buffer; `--bytes` still writes the concatenated section pages. The module and `--help` text no longer say the output is not a valid DWG.
- **Signed modular chars** in `BitCursor` and in the handle map took `value as i64` and wrapped. A magnitude outside `i64` is now `Error::SignedModularCharOverflow { value, negate }` (`i64::MIN` still decodes). `Error` is not `#[non_exhaustive]`, so the new variant is a breaking change for an exhaustive `match`.
- **`lz77-trace`**: with the feature on, the decoder's trace output still needs `DWG_LZ77_TRACE=1` at run time, so `--all-features` builds stay quiet.

### Added — viewer, conversion and write-path commits that followed the "2026-04-20 late" entry (2026-04-20, direct commits)

**The wasm crate (`wasm/`)** gained its viewer surface. Most of it is state and stable-API stubs that return placeholder values until the parent crate's entity pipeline and trailing-handle decoder close:

- `DwgFile.renderSvg()` skeleton (V-03, a3675b0); a `Viewer` class with pan, zoom around a focal pixel, reset, viewport size, fit to view and drawing/viewport coordinate conversion, state only with no render integration yet (V-05, a565508).
- `layerPanelEntries()`, an empty array until the LAYER walk can run, and `linetypeToDasharray`, which turns a signed-length LTYPE pattern into an SVG `stroke-dasharray` (positive is a dash, negative a gap, zero a dot written as 0.001) (V-06, V-07, 1df6852).
- Export buttons: `exportSvg`, `exportDxf(version)` through `dxf_convert::convert_dwg_to_dxf`, `exportGltf` and `exportPdf`, which aliases `exportSvg`; drag-and-drop accepted MIME types and extensions (V-14, V-18, cfb18d1).
- A client-side-only attestation: `clientSideAttestation()`, and a `wasm.yml` step that fails the build if the compiled module imports fetch, XMLHttpRequest, WebSocket, sendBeacon or EventSource (V-19, 443157e).
- `ViewerState`, which serialises pan, zoom, model or paper space, layout index and a 512-layer visibility bitmap to a versioned (`DWGV1`) URL-safe base64 string (V-24, 2d2ea1c); measurement entry points `measureDistance2D`, `measureDistance3D`, `measurePolygonArea` and `measurePolylineLength` (V-22, c36784f).
- Stubs with a stable shape for ten more viewer tasks, among them hatch fills, text and dimension elements, `expandInserts`, `printPreview`, `openProgressive`, `SectionBox`, `hitTest` and `entityProperties` (V-08 to V-13, V-16, V-17, V-21, V-23, 4452d80); four tiny synthetic R14 sample drawings under `wasm/examples/samples/` (V-20, f5bd374; the loader itself did not land).
- A static-site viewer for GitHub Pages: `docs/viewer/index.html` (drop-zone upload, metadata card, section table) under a content security policy of `default-src 'self'` and `connect-src 'self'`, and `pages.yml`, which builds with `wasm-pack` and deploys (V-15, 1f019ec).

**The parent crate:**

- `gltf::convert_dwg_to_gltf(&DwgFile, format, scene_name)` is the in-memory variant of `convert_file_to_gltf`, which needed a filesystem path and so could not run in the browser; `convert_file_to_gltf` delegates to it, and the wasm `exportGltf` is wired to it (13c786e).
- `dxf_convert::convert_dxf_to_dwg` is an API stub: it returns `Error::Unsupported` for empty input, for input without a `0` / `SECTION` leader, and for DXF that looks valid, with a pointer to the unwritten parser (L12-13, 5d14f12).
- `file_writer::assemble_dwg_bytes` assembles Stages 1 to 5 of the writer (the page map and section info, the CRC splice for page headers and the file-open header, the XOR mask and reserved regions) into one byte buffer. The module says it plainly: this crate's reader round-trips this crate's writer, and acceptance by AutoCAD stays manual (a2ae28e).
- `reed_solomon::{verify_stream, decode_stream, codewords_for_message}` decoded R2004+ payloads as back-to-back 255-byte codewords (L4-57, 0189ce7); the Reed-Solomon entry above removes the two stream functions, a layout §5.13 does not use. The same commit fixed an MSRV regression: `is_multiple_of` needs Rust 1.87 and the crate pins 1.85.
- `benches/write_path.rs`, a Criterion benchmark of `WriterScaffold` and `build_section` on four inputs (`alt_1k`, `uniform_16k`, `mixed_64k`, `mixed_256k`), joined the perf gate's 20 % regression check (a802a33, 3460a74); fifteen new hand-written seeds for the five existing fuzz corpus directories (#132, b90c17a).

### Added — Phase 12 write-path + Phase 13 WASM scaffolding (2026-04-20 late)

**Write path — Stages 1 through foundations of 5:**

- **`src/file_writer.rs`** — `version_magic_bytes(Version)` +
  `build_version_header(Version)` (16-byte $ACADVER leader,
  R2004+ 0x1F marker), `atomic_write(path, bytes)` via temp + rename
  (P0-10), `validate_section_name(&str)` with 16-entry
  `KNOWN_SECTION_NAMES` (P0-11, guards against typo-induced round-trip
  corruption). Existing `WriterScaffold` Stage-1 unchanged.
- **`src/crc.rs`** — `embed_crc8` / `embed_crc32` / `page_checksums`
  writer helpers (L12-02) — zero-fill-and-overwrite pattern matches
  the ODA §2.14 convention for CRC-bearing records.
- **`src/element_encoder.rs`** — `ElementEncoder` trait with
  `Line`/`Circle`/`Arc`/`Point` impls (L12-05).
- **`src/handle_allocator.rs`** — `HandleAllocator` with allocate /
  reserve / collision avoidance (L12-06).
- **`src/classes.rs`** + **`src/handle_map.rs`** — `write_class_map`
  and `write_handle_map` inverse-of-parse emitters (L12-07, L12-08).
- **`src/reed_solomon_encode.rs`** — (255, 239) systematic codeword
  encoder via GF(256) generator-polynomial long division (L12-10).
- **`tests/integration_write_roundtrip.rs`** — 4 tests covering
  multi-section Stage-1 round-trip, empty-section edge, byte-
  deterministic output, and 32-byte page alignment (L12-12 partial).
- **`src/bin/dwg_write.rs`** — 7th CLI binary. Scaffolds named-section
  input via CLI, runs the Stage-1 pipeline, emits a JSON Stage-1
  report + optional Stage-1 concatenated bytes (L12-14). Explicitly
  labeled "NOT A VALID DWG FILE" pending Stages 3-5.

**Entity decoders — MESH / DIMENSION / MLINE / IMAGE / proxy:**

- **`src/entities/mesh.rs`** — subdivision MESH per §19.4.66 (R2010+
  gate, vertex / face / edge count caps derived from `remaining_bits`)
  (L4-34).
- **`src/entities/polyface_mesh.rs`** — legacy 3D mesh header per
  §19.4.29 (L4-35).
- **`src/entities/polygon_mesh.rs`** — M×N indexed mesh header per
  §19.4.30 (L4-36).
- **`src/entities/dimension_linear.rs`**,
  **`dimension_aligned.rs`**, **`dimension_radial.rs`**,
  **`dimension_diameter.rs`**, **`dimension_angular_2l.rs`**,
  **`dimension_angular_3p.rs`**, **`dimension_ordinate.rs`** — 7
  subclass decoders per ODA §§19.4.18..19.4.23 (L4-17..21).
- **`src/entities/mline.rs`** — MLINE (§19.4.71) top-level +
  per-vertex sub-records; nested per-line segment parameters kept
  as `Vec<f64>` (honest-partial decode) (L4-54).
- **`src/entities/imagedef.rs`** — IMAGEDEF (§19.5.26) companion to
  IMAGE (L4-43).
- **`src/entities/proxy_entity_passthrough.rs`** — opaque proxy
  body preserved verbatim (L4-55).
- **`src/entities/lwpolyline.rs`** — count caps now derive from
  `remaining_bits` × 4 bits/point rather than the coarse 1 bit/item
  (L4-12).

**Graph / traversal / rendering:**

- **`src/block_expansion.rs`** (new crate module) — `expand_insert`
  with cycle detection + depth cap (default 16), emits
  `ExpandedEntity { entity, accumulated_transform, depth }`
  composing INSERT instance transforms outer-to-inner (L5-05).
- **`src/graph.rs`** — L6-18 `MODEL_SPACE_BLOCK_NAME` /
  `PAPER_SPACE_BLOCK_PREFIX` / `is_model_space_block_name` /
  `is_paper_space_block_name` / `BlockSpace` / `classify_block_name`.
  L6-19 `filter_by_paper_space_block` / `filter_by_block_space` /
  `membership_for`. L6-20 `ViewportTransform` with
  `model_to_paper` / `paper_bounds` / `contains_paper_point`.
- **`src/objects/acad_layout.rs`** — ACAD_LAYOUT decoder per §19.6.12
  (L6-12); `is_model_space()`, `paper_width/height()`,
  `extents_diagonal()` helpers.

**API hardening:**

- **`src/reader.rs`** — `DwgFile::read_section_with_limit(name,
  max_bytes)` per-call byte cap (SEC-09).
- **`src/python_stubs.rs`** — strict/lossy parity stubs for 10
  JSON-export methods (API-12).

**Fuzzing:**

- **`fuzz/fuzz_targets/rs_fec_decode.rs`** + registered in
  `fuzz/Cargo.toml` (SEC-21).
- **`.github/workflows/fuzz-nightly.yml`** — matrix over all 9
  fuzz targets at 06:00 UTC daily, 5-min duration per target,
  crash + corpus artifacts uploaded (SEC-24).
- **`fuzz/corpus/{rs_fec_decode,header_vars,classmap_parse,handlemap_parse}/`** —
  hand-crafted seeds exercising distinct code paths (SEC-23 seed).
- **`fuzz/fuzz_targets/object_walker.rs`** — uses public
  `collect_all_lossy` API (fixes pre-existing fuzz compile gap).
- **`tests/integration_fuzz_corpus_regression.rs`** — 6 tests replay
  every seed through the matching library entry point and forbid
  panics; this locks the fuzz contract against future regressions
  (L4-61).

**WASM Phase 13 scaffolding:**

- **`wasm/`** (new subcrate) — `dwg-wasm` with wasm-bindgen +
  js-sys + serde-wasm-bindgen. `DwgFile` JS class with
  `open(bytes)` / `versionMagic()` / `versionName()` / `sections()` /
  `sectionMapStatus()` + `crateVersion()` (V-01, V-02).
- **`.github/workflows/wasm.yml`** — matrix build over `--target
  web / bundler / nodejs`, uploads `web` artifact (14-day
  retention), asserts `pkg/dwg_wasm_bg.wasm` + `pkg/dwg_wasm.js`
  present. SHA-pinned actions.

Also: 18+ stale tasks closed as bookkeeping cleanup; Twitter thread
(L-13) refreshed to mention `dwg-to-dxf` / `dwg-to-svg` / `dwg-to-gltf`
which all shipped.

### Added — CI release infrastructure (2026-04-20, Q-06 / Q-07 / Q-09)

- **`.github/workflows/perf.yml`**: criterion-benchmark
  regression gate. Push-to-main saves a named `main` baseline to
  GitHub Actions cache; pull requests run the same bench set and
  diff against that baseline with `critcmp`. >20 % regression on
  any of `lz77`, `section_map`, `object_walk`, `metadata_parse`,
  or `libredwg_compare` fails the job. First-run cache misses are
  a warning, not a failure.
- **`.github/workflows/docs-rs.yml`**: pre-release docs.rs build
  clone. Runs `cargo doc --no-deps --all-features` with
  `RUSTDOCFLAGS='-D warnings'`, asserts >=10 HTML files in
  `target/doc/dwg`, and soft-gates docstring coverage on
  `pub fn` at 80 %.
- **`.github/workflows/release.yml`**: tightened SemVer tag
  regex (`v[0-9]+.[0-9]+.[0-9]+` ± `-prerelease`), added
  `dwg-to-dxf` to the binary matrix (5 binaries × 5 targets),
  added pre-publish dry-run, scaffolded a gated-off
  `publish-pypi` job for eventual PyO3 wheel releases.
- **`README.md`**: added Perf and docs.rs CI status badges.
- **`RELEASE.md`**: concrete release checklist — pre-flight,
  version bump, verification, tag, pipeline monitoring, post-publish.

All third-party actions remain SHA-pinned per the SEC-28 baseline
established at repo public-ification.

### Added — rendering pipeline primitives (2026-04-20)

Decoder-independent building blocks for the SVG / PDF / glTF / DXF
export paths. These ship without waiting on the common-entity
preamble fix (tracked below) so the downstream renderer work can
proceed in parallel.

- **`src/api.rs`**: `ParseMode { Strict, BestEffort }` enum,
  `Decoded<T> { value, diagnostics, complete }` wrapper with
  `complete()` / `partial()` / `map()`, `Warning { code, message,
  bit_offset }`, and a `Diagnostics` accumulator with `warn` /
  `warn_at` / `confidence(total)` / `is_clean`. Lays the API spine
  for the strict/lossy discipline planned across every public
  entry point.
- **`src/geometry.rs`**: `Point2D` / `Point3D` inherent methods
  (`add`, `sub`, `distance`, `lerp`, `new`), `VecOps` trait on
  `Vec3D` (`scale` / `dot` / `cross` / `length` / `normalize`),
  4×4 `Transform3` with `identity` / `translation` / `scale` /
  `rotation_z` / `compose` / `transform_point` / `transform_vector`,
  axis-aligned `BBox3` with empty-sentinel identity under union,
  and an indexed `Mesh` container (shared vertex list + triangle
  indices, `push_triangle` / `push_quad`).
- **`src/curve.rs`**: unified `Curve` enum (`Line` / `Circle` /
  `Arc` / `Ellipse` / `Polyline` / `Spline` / `Helix`) with
  conservative `bounds()` per variant, and `Path { segments,
  closed }` with `from_polyline` helper and union-of-segments
  bounds.
- **`src/color.rs`**: 256-entry ACI palette (`aci_to_rgb(u8)` →
  `(u8, u8, u8)` and `aci_to_hex(u8)` → `#RRGGBB`). Provenance
  noted in module docs.
- **`src/svg.rs`**: string-based SVG 1.1 writer (`SvgDoc::new` /
  `begin_layer` / `end_layer` / `push_curve` / `push_path` /
  `finish`). `Style { stroke, stroke_width, fill, dashes }`. CAD
  Y-up → SVG Y-down flip applied at the root `<g>`.
- **`src/dxf.rs`**: group-code DXF writer (`DxfWriter::new`,
  section balance enforced with `begin_section` / `end_section`,
  typed `write_string` / `write_int` / `write_double` /
  `write_point` / `write_handle` / `write_entity_header` /
  `write_comment`, terminated by `finish`). Panics on misuse
  (nested sections, finish-with-section-open, double-finish).
- **`src/limits.rs`**: new `WalkerLimits` struct for graph
  iteration (`max_handles`, `max_scan_bytes`, `max_block_nesting`)
  with `safe` / `paranoid` / `permissive` profiles mirroring
  `ParseLimits`.
- **`src/handle_map.rs`**: `HandleMap::iter()`, `len()`,
  `is_empty()`, and `IntoIterator for &HandleMap` so callers can
  walk `(handle, offset)` pairs without directly touching the
  `entries` field.

### Added — forensic + external surfaces

- **`examples/trace_common_entity.rs`**: forensic tracer that
  prints every common-entity preamble field's bit position and
  decoded value for the LINE at offset 11884 in `line_2013.dwg`.
  The output is the starting point for the ODA §19.4.1 R2004+
  cross-reference that closes the preamble-misalignment bug.
- **`examples/dump_line_payload.rs`**: bit-walk of the LINE
  payload (MC + object_type + handle) for manual verification
  against the spec.
- **`examples/test_h2_truncate.rs`**: empirical falsification of
  H2 (data-stream boundary bleed) — confirms the preamble field
  order itself is wrong, not a cursor-into-handle-stream bleed.
- **README**: capability matrix ("parsing / metadata / entities /
  geometry / write / IFC-equivalent" × shipped / alpha / pending)
  at the top, rvt-rs sibling cross-link in Related Projects.
- **CONTRIBUTING.md**: entity-decoder coverage is now the #1
  most-wanted contribution.
- **RELEASE.md**: SemVer commitment (with 0.x breakage window),
  cut-a-release runbook, yank / backport / deprecation policies.
- **docs/EXTENDING_DECODERS.md**: worked POINT example (struct,
  decoder fn, dispatcher wiring, tests, defensive caps).
- **cliff.toml**: git-cliff config for automated CHANGELOG
  generation from conventional commits.
- **`.github/ISSUE_TEMPLATE/corpus_submission.yml`**: licensed
  public-corpus submission flow.
- **`.github/ISSUE_TEMPLATE/unsupported_version.yml`**: AC1033+
  version-not-supported intake.
- **`fuzz/fuzz_targets/`**: three new libfuzzer harnesses
  (`classmap_parse`, `handlemap_parse`, `header_vars`) exercising
  all 8 supported versions.
- **GitHub Discussions** enabled on the repo.

### Changed

- `#![deny(unsafe_code)]` → `#![forbid(unsafe_code)]` in
  `src/lib.rs`. The crate ships with zero `unsafe`, so `forbid`
  is satisfiable and makes the invariant a hard compile-time
  error rather than a lint.
- `lz77::decompress` is now documented to clamp its output at
  256 MiB via `DecompressLimits::default()`; new regression test
  pins the contract (`default_limits_cap_output_at_256_mib`) and
  a compression-bomb test proves a 6-byte input claiming 1 TiB
  stays bounded (`small_input_with_huge_expected_size_stays_bounded`).

### Known at 0.1.0-alpha.1 — decoder-correctness regression discovered (task #97)

Task #97 (validate decoders against real R2013 corpus) surfaced a
deeper architectural gap than the dispatcher type-code bugs that
#71-#96 closed:

1. **Handle walk missed modelspace geometry.** The single-entity
   R2013 samples (`line_2013.dwg`, `circle_2013.dwg`, `arc_2013.dwg`)
   each decode 6 objects, all of which are empty `BLOCK`/`ENDBLK`
   shells. The user-drawn LINE/CIRCLE/ARC is stored at a handle
   reachable only through `BLOCK_HEADER → owned entities` — a
   traversal the 0.1.0-alpha.1 reader did not perform.
2. **Bit-cursor offset inside typed payloads was wrong on R2018.**
   `sample_AC1032.dwg` is the one corpus file where typed entity
   decoders fire on real data, and the results are garbage: LINE
   endpoints with `z = 1.2e+225`, POINT positions with
   `x = 4.4e+138`, CIRCLE centers with `z = -3.2e+113`. This
   indicates the cursor is not positioned where the spec says it
   should be after the common-entity preamble — either a bit-count
   error earlier in the pipeline or a missed preamble field in the
   R2018 layout.

Those invariants are now active default tests in the Unreleased
changes above. The historical "honest coverage" numbers below measure
*dispatch success*, not *value correctness*.

## [0.1.0-alpha.1] — 2026-04-19

First public pre-release. **Not production-ready.** See [README](./README.md)
for the full empirical coverage story; the short version is below.

### Scope reality check

- **Entity-decode end-to-end coverage**, measured by
  `examples/coverage_report.rs` against the `nextgis/dwg_samples` +
  `sample_AC1032.dwg` corpus (19 files) after the dimension-subtype
  correction (task #71):
  - R14 / R2000 / R2007 — **not supported** (no handle-map walker for these layouts yet).
  - R2004 — 0 / 21 entities decoded (**0 %**).
  - R2010 — 9 / 21 entities decoded (**43 %**).
  - R2013 — 18 / 21 entities decoded (**86 %**).
  - R2018 (`sample_AC1032.dwg`) — 66 / 306 entities decoded (**22 %**).
  - **Aggregate:** 93 / 369 attempted entities decoded = **25 %**.
- 439 objects in the R2018 sample are legitimate non-entity types
  (dictionaries, controls, symbol-table entries) that the dispatcher
  correctly returns as `Unhandled` — these are not counted as failures.
- Task #71 rewrote the dispatcher's fixed code table to match ODA spec
  §5 Table 4. Pre-fix numbers (27 % aggregate) counted structurally
  wrong dimension decodes as successes; post-fix numbers are the
  honest figure.

The gap between "all 27 entity decoders have passing unit tests" and
"27 % of real entities decode end-to-end" is exactly the
common-entity-preamble + object-stream layout work that 0.1.0 stable
will fix.

### Added

**Container layer (shipping, 193 tests green)**
- `DwgFile::open` / `DwgFile::from_bytes` — top-level reader.
- Version identification for AC1014, AC1015, AC1018, AC1021, AC1024, AC1027, AC1032.
- R13–R15 simple file header + R2004+ XOR-encrypted header + CRC-32 verify.
- LZ77 decompressor (ACadSharp-verified +1 offset dialect).
- Section Page Map + Section Info parser.
- `DwgFile::read_section(name)` for every named section.
- Reed-Solomon(255,239) over GF(256) decoder — Berlekamp-Massey + Chien + Forney.
- Metadata parsers: `SummaryInfo`, `AppInfo` (R18 ANSI + R21+ UTF-16 auto-detect),
  `Preview` (BMP / WMF / PNG code-6), `FileDepList`.
- `HandleMap`, `ClassMap`, `HeaderVars` parsers.
- `ObjectWalker` (R2004+ only) — `all_objects()` returns `Vec<RawObject>` with
  handle-indexed iteration. **Works reliably** on R2018 (745 objects enumerated
  from sample corpus file).

**Entity dispatcher (alpha)**
- 27 per-entity decoders under `src/entities/*.rs` (LINE, POINT, CIRCLE, ARC,
  ELLIPSE, RAY, XLINE, SOLID, 3DFACE, TRACE, SPLINE, TEXT, MTEXT, ATTRIB,
  ATTDEF, INSERT, BLOCK, ENDBLK, VERTEX, POLYLINE, LWPOLYLINE, DIMENSION (7
  subtypes), LEADER, IMAGE, HATCH, MLEADER, VIEWPORT).
- `DecodedEntity` typed enum + `decode_from_raw(raw, version)` dispatcher.
- `DwgFile::decoded_entities()` — end-to-end walk + dispatch + summary.
- `DispatchSummary` — honest bookkeeping (decoded / unhandled / errored).
- **All 27 decoders pass unit tests on synthetic input.** Real-world coverage
  is the 27 % cited above.

**Symbol tables + control objects**
- LAYER, LTYPE, STYLE, VIEW, UCS, VPORT, APPID, DIMSTYLE, BLOCK_RECORD under
  `src/tables/*.rs` — decoder functions exist, not wired into a walker
  dispatcher yet.
- DICTIONARY, XRECORD, `*_CONTROL` under `src/objects/*.rs`.

**Write path (partial)**
- Bit-writer: inverse of every BitCursor primitive, round-trip tested.
- LZ77 literal-only encoder (correctness-first; matcher pass is future work).
- `section_writer::build_section` — per-section framer with Sec_Mask XOR +
  CRC + LZ77. Verified: built sections decompress back to input bit-exactly.
- `file_writer::WriterScaffold` — stage-1 of 5 of a full `DwgFile::to_bytes()`
  pipeline. Stages 2–5 (page map, section info, system pages, file-open
  header) are scaffolded with an explicit roadmap in the module comment.

**R2007 Sec_Mask**
- Layer 1 (byte XOR with per-section LCG seed) — implemented, tested, NOT
  wired into reader yet.
- Layer 2 (7-byte window bit-rotation) — scaffolded, partial implementation.
- R2007 files currently parse header + section list only; section payloads
  return a placeholder error.

**CLI tools**
- `dwg-info`, `dwg-corpus`, `dwg-dump`, `dwg-convert`.
- `examples/coverage_report.rs` — the script that produced the empirical
  numbers above. Run it on your files before relying on decode output.

**Infrastructure**
- CI matrix: Linux / macOS / Windows × (stable, MSRV 1.85) ×
  fmt / clippy / test / doc / deny / msrv.
- `deny.toml` — supply-chain policy: Apache-2 / MIT / BSD / ISC / Zlib /
  Unicode-3.0 / MPL-2.0 / CC0-1.0 allowed; GPL denied; crates.io-only sources.
- Dependabot — weekly cargo + monthly actions.
- Issue + PR templates with clean-room declaration checkbox.
- SECURITY.md with private reporting flow + threat model.
- CITATION.cff for academic citations.
- ARCHITECTURE.md — technical deep-dive.
- Fuzz scaffolding: 5 `cargo-fuzz` targets (lz77_decompress,
  bitcursor_primitives, dwg_file_open, section_map, object_walker) under
  `fuzz/`. Compile-verified; overnight sweep is pre-1.0 work.

### Safety

- `#![deny(unsafe_code)]` on the entire crate.
- 193 tests: 156 unit + 5 corpus + 9 proptest + 22 sample-specific + 1 doctest.
- `cargo clippy --all-targets --all-features -- -D warnings` clean.
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` clean.
- `cargo fmt --all -- --check` clean.
- `cargo publish --dry-run` succeeds — 89 files, 129 KB compressed.

### What's deferred

These block 0.1.0 stable:

1. **Common-entity preamble fixes** to lift R2004 / R2010 / R2018 entity decode
   coverage from 0–22 % to >90 %. This is the highest-impact work item.
2. **R14 / R2000 object-stream walker** — different layout from R2004-family.
3. **R2007 Sec_Mask layer-2 bookkeeping** — spec §5.2.
4. **Table-entry dispatcher** — the equivalent of `DecodedEntity` for
   symbol-table records; today each table-entry decoder is call-it-yourself.
5. **Fuzz session** — first overnight run of the 5 targets under `fuzz/`.
6. **Write path stages 2–5** — `DwgFile::to_bytes()` file-level assembly.

### Legal posture

Clean-room — no Autodesk SDK, no ODA SDK, no LibreDWG (GPL-3) source
consulted. Implemented against the ODA's freely-redistributable *Open Design
Specification for .dwg files* (v5.4.1). Where the spec is ambiguous in one
place (an LZ77 offset-encoding corner), the authors consulted a publicly
documented errata reading via algorithm descriptions only — no implementation
code was reviewed or ported.

### Not yet

- Not published to crates.io.
- No official release tarball.
