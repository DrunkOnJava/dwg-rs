# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

dwg-rs is an Apache-2.0 Rust reader (and early writer) for Autodesk DWG, R13 to R2018 (AC1012 to AC1032), implemented only from the ODA *Open Design Specification for .dwg files* v5.4.1. The crate is `dwg`, edition 2024, MSRV 1.85. The plan to 1.0 is `ROADMAP.md`; the release gate is `docs/release-1.0-criteria.md`; tracker rules are `docs/maintaining.md`.

## Commands

`main` requires twelve checks (the `main-protection` ruleset, kept in `.github/rulesets/`): Rustfmt, Clippy, Test on Linux, macOS and Windows (stable), Verify MSRV (1.85), Documentation, cargo-deny, PII guard, Streaming E2E, Corpus (public DWG) and Spec coverage, so a corpus count change without its regenerated baseline, or a coverage page out of step with its data, blocks the merge. It is strict: a PR must be up to date with `main`, so every merge restarts the full matrix on the PRs behind it. Locally:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
tools/ci/run_test_targets.sh --release --all-features    # every non-unit target of tools/ci/test-targets.txt, then the doc tests
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo deny check advisories bans licenses sources        # CI pins cargo-deny 0.19.4
CARGO_TARGET_DIR=target-msrv cargo +1.85 check --all-features   # MSRV without evicting the stable cache
python3 tools/ci/spec_coverage.py --check                # docs/spec-coverage.md agrees with its data
python3 tools/ci/dispatch_arms.py --check                # fixed_entity_label and decode_fixed_entity_body agree
```

CI also builds with `RUSTFLAGS="-D warnings"`, so a warning anywhere fails the Test job. The Test job runs `tools/ci/run_test_targets.sh`, not a bare `cargo test`: it runs the `real`, `contract` and `hardening` targets of `tools/ci/test-targets.txt` and the documentation examples, never the `unit` ones and never the inline `#[cfg(test)]` suites in `src/`.

Single targets:

```sh
tools/ci/run_test_targets.sh --check-only                                # every tests/*.rs file is classified
cargo test --release --all-features --test canonical_corpus            # one integration file (tests/<stem>.rs)
cargo test --release --all-features --test samples arc_r14_is_ac1014    # one test in it
```

The binaries (`dwg-info`, `dwg-corpus`, `dwg-dump`, `dwg-convert`, `dwg-to-dxf`, `dwg-to-gltf`, `dwg-write`) all have `required-features = ["cli"]`; plain `cargo build` builds none of them:

```sh
cargo run --release --features cli --bin dwg-info -- drawing.dwg
```

Measurement and gate examples (the same commands the `coverage-smoke` CI job runs):

```sh
cargo run --release --example coverage_report -- <dir-of-dwg>      # decoded / skipped / errored per file + TOTAL
cargo run --release --example build_fixtures                        # regenerate tests/fixtures/canonical/*.dwg
cargo run --release --example probe_class_census -- <file> --strict # walk reaches every instance AcDb:Classes declares
cargo run --release --example probe_reference_closure -- <file>     # no gaps between records, no dangling hard handles
cargo run --release --example spec_examples                         # the spec's own worked examples (#107); CI's Test job runs it
```

`tests/fixtures/spec-examples/` quotes the spec PDF verbatim; `python3 tools/spec/extract_examples.py --pdf <spec.pdf> --check` audits it against the PDF. When a decoder change moves a §20.4 example's outcome, the step fails; `cargo run --release --example spec_examples -- --update` rewrites the moved `expect` cells in `r14-objects.tsv` and their notes in `docs/data/spec-coverage.tsv` and `docs/spec-coverage.md`, and lists them. Commit those in the same PR. A record that newly errors is written `error #?` and keeps failing until its tracking issue is filled in.

Fixture generation is byte-deterministic and CI diffs the committed canonical fixtures against `build_fixtures` output, so any change to the writer or the synthetic entities means regenerating and committing the fixtures in the same PR.

Streaming E2E (`.github/workflows/streaming.yml`):

```sh
python3 tools/streaming/check_invariants.py
cargo build --locked --example stream_header --example streaming_schedules
python3 tools/streaming/verify.py target/debug/examples/stream_header target/debug/examples/streaming_schedules
```

Features: `lz77-trace` (plus `DWG_LZ77_TRACE=1` at runtime) traces the LZ77 decoder; `dhat-heap` enables `examples/dhat_profile.rs`. Benches are Criterion (`cargo bench`); the Perf workflow fails a PR only when a bench is at least 20% and at least 5 µs slower than `main` and repeats on a re-run. Measure before touching `bitcursor.rs`, `bitwriter.rs` or `lz77*.rs`.

`wasm/` (`wasm-pack build --target web --release`), `fuzz/` (`cargo +nightly fuzz run <target>`, targets in `fuzz/fuzz_targets/`) and `tools/synthetic-dwg/` are separate crates, not workspace members. `python3 tools/maintainer/audit.py` runs the read-only GitHub housekeeping checks from `docs/maintaining.md` (needs `gh`).

## Real-file corpus

Every real drawing CI reads is public, pinned by sha256 in `tools/corpus/pins.tsv`, and fetched at run time; none is committed (nextgis declares no licence, so never vendor its files). The pins cover `nextgis/dwg_samples` at ff05d5a8 (42 drawings from R14 to 2013, each with AutoCAD's DXF export) and ACadSharp's `samples/` at c30a97ce (MIT, including the R2018 files):

```sh
python3 -I tools/corpus/fetch.py --dest ../dwg-corpus --legacy-samples ../../samples   # fetch + verify every pin
cargo build --release --example coverage_report --example probe_class_census --example probe_reference_closure
python3 -I tools/corpus/measure.py run --corpus ../dwg-corpus --bin target/release/examples --out r.json
python3 -I tools/corpus/measure.py check --results r.json        # every drawing against tools/corpus/baseline.tsv
```

The `Corpus (public DWG)` job runs exactly this and fails on any count change in either direction, so a PR that changes a decoded, skipped or errored count must commit the regenerated baseline (`python3 -I tools/corpus/measure.py baseline --results r.json` after the `run` above, or the `corpus-measurement` artifact the job uploads). Streaming E2E reads its own pinned nextgis files (`tools/streaming/*-corpus.json`, cross-checked against `pins.tsv`).

The older real-file integration tests (`tests/samples.rs`, `block_names.rs`, `corpus_roundtrip.rs`, `r2013_entity_values.rs`) read a 19-file subset at `../../samples` relative to the crate root and **skip silently** when it is absent, so a green run of them says nothing about real drawings unless that directory exists. `fetch.py --legacy-samples ../../samples` writes exactly that subset there; from a worktree, `../../samples` resolves against the worktree's own path.

Coverage numbers come from two places, and each says which: the headline tables in `README.md` and `STATUS.md` cite the corpus job's run on `main`, while older sections (STATUS.md's 2026-08-30 baseline, the evidence behind `closes` states in `docs/spec-coverage.md`) are dated local measurements of the 19-file subset. Never estimate or carry a number forward: re-measure, and cite the run or the command and date.

## Architecture

`DwgFile` (`src/reader.rs`) is the entry point. The read path: `version.rs` identifies the AC magic, `header.rs` parses the file header (`cipher.rs` XOR-decrypts the R2004+ one), `section_map.rs` builds the page map and section info, `read_section` un-masks each page (Sec_Mask) and decompresses it (`lz77.rs`), then per-section parsers run: `handle_map.rs` (AcDb:Handles), `classes.rs` (AcDb:Classes), `header_vars.rs`, `metadata.rs`. `object.rs` walks the object stream into `RawObject`s, and `entities/dispatch.rs` (`decode_from_raw`, `decode_from_raw_with_class_map`, `DecodedEntity`, `DispatchSummary`) routes each by `object_type.rs` code to `entities/`, `tables/` or `objects/`. `graph.rs` resolves owner, reactor, layer and style references. The exporters (`dxf*.rs`, `svg.rs`, `gltf.rs`) and the writer (`bitwriter.rs`, `lz77_encode.rs`, `section_writer.rs`, `file_writer.rs`, `DwgFile::to_bytes`) sit on top. `lib.rs` and `Cargo.toml` are the authoritative module and binary lists; ARCHITECTURE.md §2 maps them.

Three containers share no mechanism:

- **R13 to R15**: the file is the object stream, addressed by the flat §3.2.6 locator list; no section map, no compression.
- **R2004 family** (AC1018, AC1024, AC1027, AC1032): encrypted header, page map, Sec_Mask, §4.7 LZ77.
- **R2007** (AC1021): `r2007.rs` and `r21_lz.rs`, its own Reed-Solomon de-interleave, page and section maps and §5.10 LZ. Encrypted R2007 sections are refused, not decoded.

**Every decoder is self-validating.** A field list must end exactly on the record's data-stream boundary, or it returns an error instead of plausible-looking geometry. The boundary source differs by band: on R2010+ the string-stream trailer (or the bit before the `strings present` flag when there are no strings); on R2000 and R2004 the `RL` object-data size in the object prologue; on R2007 the same `RL` read through the §19.1 trailer; on R13 and R14 a size inside the common entity data. "Errored" counts are therefore trustworthy and name an unfinished field list; "skipped" (no decoder) is the real gap. A boundary that closes proves the widths add up, not the values: only the DXF oracle (#99) proves values.

**R2007+ split streams** (`string_stream.rs`): `TV` text lives in a separate string stream and `H` references in a handle stream, so a modern decoder reads text from the string cursor and treats a handle slot as consuming no data-stream bits. Reading `TV` from the data cursor produces garbage UTF-16 or a cursor overrun.

**`src/streaming/` is a sealed `no_std` grammar.** `tools/streaming/check_invariants.py` rejects `std::`, `alloc::`, `Vec`, `String`, `Box`, `Rc`, `Arc`, `DwgFile`, `unsafe`, `extern crate` and `include!` anywhere in it, comments included, and CI compiles the same files through `tools/streaming/no_std.rs` with no allocator, natively and for `wasm32-unknown-unknown`. Parser state and the checkpoint holder have compile-time size bounds. Never import legacy reader types there. Pausable delivery goes through `byte_source.rs`; the contract is in `docs/streaming/README.md`.

Invariants across the crate: `#![forbid(unsafe_code)]`; malformed input returns `Error`, never panics; claimed counts and allocations are capped (`limits.rs`, `ParseLimits`, `WalkerLimits`).

## Rules for changes

- **Clean-room** (`CLEANROOM.md`): never consult or import LibreDWG, the ODA/Teigha SDK, Autodesk SDKs or any GPL DWG source. Algorithm-description comments in MIT, Apache or BSD projects may resolve a spec ambiguity only with a dated disclosure added to `CLEANROOM.md`. Every PR body carries the source-provenance declaration from the PR template.
- **Cite the spec by v5.4.1 number.** Objects are §20.4.x; older code still says §19.4.x (#106). `docs/spec-coverage.md` gives every section a state (`missing`, `decoder`, `partial`, `closes`, `complete`) and is generated: edit `docs/data/spec-coverage.tsv`, then run `python3 tools/ci/spec_coverage.py --write`. CI's Spec coverage job fails when a section has no row or the page and its data disagree. A state changes only in the PR that brings its evidence.
- **Unit tests are banned** (the maintainer's standing directive, 2026-09-23), and evidence is real-file. Do not add one, and do not add a regression test for a fix: a unit-suite pass count is never evidence. The PR template asks for the corpus records a change was measured on and the reproducing command. `tools/ci/test-targets.txt` classes every `tests/*.rs` file as `real` (reads real drawings or the committed canonical fixtures), `cli`, `contract` (checks a published claim), `hardening` (replays inputs that crashed a parser) or `unit` (synthetic-only or property tests); the Test job runs all but `unit`, and `tools/ci/run_test_targets.sh` fails on a file the manifest does not class, so a new target needs a line there and a reason. The `unit` files stay as unrun source, and the inline `#[cfg(test)]` suites in `src/` are deleted area by area (git history is the archive): never add one, and never let a decoder PR re-grow one. A new format finding gets a probe under `examples/` (module doc saying what it proves and how to verify it) and a dated section in `ARCHITECTURE.md` with its evidence table.
- **New decoders** follow `docs/EXTENDING_DECODERS.md`. For a fixed-code entity: a type-code constant, a `DecodedEntity` variant, a `fixed_entity_label` entry and a `decode_fixed_entity_body` arm in `entities/dispatch.rs`. The label list and the body arms must match (`python3 tools/ci/dispatch_arms.py --check` reads both from the source, in CI's Spec coverage job), and `checked_inline` applies the boundary check for you. Custom classes dispatch on their DXF class name instead. `templates/entity-decoder` is a cargo-generate skeleton.
- Every `pub fn` in `src/` gets a `///` line naming its bit code, flag mask or spec section; the docs-rs workflow fails below 80% and the tree is at 100%.
- No absolute home paths (`/Users/<name>/`, `/home/<name>/`, `C:\Users\<name>\`) anywhere outside `.github/workflows/`: the PII guard fails on them. No PII in fixtures; use synthetic values.
- Conventional Commits with the scopes listed in CONTRIBUTING.md. `main` is squash-merge only, so one PR is one commit and one finding.
