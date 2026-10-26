# Contributing to dwg-rs

Thanks for your interest. This project is small, solo-maintained,
and evolving — contribution guidelines are intentionally light, but
a few practices keep the repo healthy.

## Quickstart: clone to first PR in ten minutes

Everything CI checks runs locally with stock cargo — no corpus, no
credentials, no Windows box.

```sh
git clone git@github.com:DrunkOnJava/dwg-rs.git && cd dwg-rs
cargo build --release
cargo run --release --example basic_open -- path/to/any.dwg   # optional smoke
```

The gate (identical to the required CI checks — run it before every push):

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --release
cargo deny check all          # cargo install cargo-deny
```

Two soft gates you will also hit:

- **Docstrings.** The docs-rs workflow requires at least 80% of `pub fn` in
  `src/` to carry a `///` line directly above them (attributes in between
  are fine). Today the tree is at 100% — keep it there: every new `pub fn`
  gets a one-line doc naming the ODA bit code, flag mask, or spec section
  it implements.
- **Benchmarks.** `cargo bench` results are compared against `main`; a
  ≥ 20% regression on any Criterion bench fails the job. Measure before
  touching `src/bitcursor.rs`, `src/bitwriter.rs`, or `src/lz77*.rs`.

Coverage numbers in `README.md` / `STATUS.md` are *measured*, never
estimated, and come from the `Corpus (public DWG)` CI job. It fetches the
public drawings pinned in `tools/corpus/pins.tsv` (nothing is committed),
runs the real-file tests on them, and fails when any drawing's
measurement differs from `tools/corpus/baseline.tsv`, in either
direction. A change that moves a number updates the baseline in the same
PR. The job's `corpus-measurement` artifact holds the `baseline.tsv` it
measured; or reproduce it locally:

```sh
python3 -I tools/corpus/fetch.py --dest ../dwg-corpus --legacy-samples ../../samples
cargo build --release --example coverage_report \
    --example probe_class_census --example probe_reference_closure
python3 -I tools/corpus/measure.py run --corpus ../dwg-corpus \
    --bin target/release/examples --out corpus-results.json
python3 -I tools/corpus/measure.py check --results corpus-results.json
python3 -I tools/corpus/measure.py baseline --results corpus-results.json   # record a change
```

`--legacy-samples ../../samples` places the 19 drawings that
`tests/samples.rs` and its siblings are written against where they look
for them (they skip silently without it). To compare a branch against
`main` on GitHub's runners: `gh workflow run measure.yml -f ref=<branch> -f base=main`.

The plan is [`ROADMAP.md`](ROADMAP.md) and its milestones; every section of
ODA spec v5.4.1 and its state on `main` is in
[`docs/spec-coverage.md`](docs/spec-coverage.md), and each `missing` or
`partial` row names the issue that tracks it. Cite spec sections by their
v5.4.1 numbers. How issues are labelled and placed is in
[`docs/maintaining.md`](docs/maintaining.md).

Pick a starting point from the
[`good first issue`](https://github.com/DrunkOnJava/dwg-rs/labels/good%20first%20issue)
label — each one names the spec section, the file, and the test that will
prove it. Then: branch → change → gate → PR (the template asks for the ODA
spec citation and the source-provenance declaration below). `main` is
squash-merge only, so one PR is one commit; keep them small.

## What's welcome

- **Entity decoder coverage — the #1 most-wanted contribution.** The
  container layer (container, sections, LZ77, RS-FEC, ObjectType,
  HandleMap, ClassMap) is shipped and tested; per-entity field-body
  decoders are alpha and fail on most real-world files (see README
  coverage table). Fixing bit-alignment on the common-entity
  preamble across R2004 / R2010 / R2013 / R2018, and wiring up
  end-to-end decode for LINE / CIRCLE / ARC / LWPOLYLINE / TEXT /
  MTEXT / INSERT / DIMENSION, closes the single biggest gap between
  "decoder functions exist" and "dwg-rs reads real drawings."
  Contributions against the public ODA specification are very
  welcome.
- **Bug reports** with a minimal reproducer (the smallest `.dwg`
  that triggers the issue). Include the DWG version byte (visible
  in the first 6 bytes as `AC10XX`). Security-sensitive reports go
  through [`SECURITY.md`](SECURITY.md), not public issues.
- **Performance regressions** caught by the benchmark harness —
  open an issue with a before/after table.
- **New facts about the file format.** This project documents its
  findings in `ARCHITECTURE.md` with dated evidence. Please mirror
  any new finding there and back it with a reproducible probe under
  `examples/`.
- **Documentation improvements.** The README, ARCHITECTURE, and
  inline doc comments are fair game.
- **Corpus files.** Public DWG files with a licence that lets CI
  fetch them, ideally with AutoCAD's DXF export of the same drawing,
  are as valuable as code (#153). Edge-case file layouts, R2004+
  Sec_Mask variants and R2007 compressed-section-info variants are
  especially wanted.

## What needs discussion first

Open an issue (or a draft PR) before starting work on any of:

- RS-FEC (Reed-Solomon forward error correction) layer — encoder-
  side work is subtle; please align on the plan first.
- R2007 full-decompression path — this is the most complex
  container variant and benefits from design discussion.
- The DWG writer. It is experimental, and making AutoCAD open its
  output is the 0.4.0 milestone (#159); align on the plan first.
- Unsafe code — the library declares `#![forbid(unsafe_code)]`. If
  you genuinely need it (e.g. for a performance-critical SIMD
  decoder), open an issue first.

## Coding conventions

- Rust 2024 edition.
- `cargo fmt` before every commit.
- `cargo test --release` must pass. The CI in `.github/workflows/`
  enforces this.
- **No `unsafe` in the library crate.** See above.
- **No panics in parsing paths.** Malformed input must return an
  `Error`, never `panic!`. Defensive caps on loop iteration and
  allocation are required; see existing decoders for the pattern.
- **No PII in tests.** Use synthetic fixtures — `testuser`,
  `11111111`, generic layer names — not anything drawn from real
  shipped files.
- **Every probe under `examples/`** gets a module-level doc comment
  explaining *what fact it proves* and *how to verify* the result.

## Commit messages

Conventional Commits format:

- `feat(<scope>): ...` for new features
- `fix(<scope>): ...` for bug fixes
- `docs(<scope>): ...` for documentation
- `test(<scope>): ...` for test-only changes
- `refactor(<scope>): ...` for behavior-preserving internal changes
- `perf(<scope>): ...` for performance
- `chore(<scope>): ...` for infra / CI / build

Scopes that appear frequently: `container`, `sections`, `lz77`,
`rs-fec`, `object-types`, `handle-map`, `class-map`, `entities`,
`objects`, `tables`, `streaming`, `writer`, `fixtures`, `readme`,
`arch`, `ci`.

## Reverse-engineering findings

When you discover something new about the file format:

1. Write a short probe under `examples/<name>.rs` that reproduces
   the finding from bytes. One self-contained file.
2. Add a dated section to `ARCHITECTURE.md` with an evidence table
   (byte offsets, observed values, variant count across versions).
3. If the finding is a decoding rule, show it on real files: name
   the corpus records it changes and paste the coverage report from
   before and after into the pull request. Don't add unit tests.

This keeps every claim independently verifiable, which is the
whole point of open reverse-engineering work.

## Legal note for contributors

dwg-rs is Apache-2.0 licensed. By submitting a contribution, you
agree that your work is licensable under Apache-2.0 and that you
have the right to grant that license.

**Please do not submit any code, comments, tests, or documentation
that contains information derived from Autodesk- or Open-Design-
Alliance-proprietary sources** (NDA'd SDKs, ObjectARX internals,
decompiled binaries, leaked internal documents, any part of the ODA
Drawings SDK / Teigha codebase). This project operates strictly
from the public ODA specification document and from on-disk byte
observations of files produced by publicly available CAD software.

Questions: open an issue on GitHub, or file a security-sensitive
report via the advisory flow in [`SECURITY.md`](SECURITY.md).
