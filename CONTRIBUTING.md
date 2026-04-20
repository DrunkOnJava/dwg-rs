# Contributing to dwg-rs

Thanks for your interest. This project is small, solo-maintained,
and evolving — contribution guidelines are intentionally light, but
a few practices keep the repo healthy.

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
- **Tests.** More coverage is always welcome, especially for
  edge-case file layouts, R2004+ Sec_Mask handling, and R2007's
  compressed-section-info variants.

## What needs discussion first

Open an issue (or a draft PR) before starting work on any of:

- RS-FEC (Reed-Solomon forward error correction) layer — encoder-
  side work is subtle; please align on the plan first.
- R2007 full-decompression path — this is the most complex
  container variant and benefits from design discussion.
- Any modifying-writer path. The project is currently read-only by
  design; write support is a major scope expansion.
- Unsafe code — the library declares `#![deny(unsafe_code)]`. If
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
`readme`, `arch`, `ci`.

## Reverse-engineering findings

When you discover something new about the file format:

1. Write a short probe under `examples/<name>.rs` that reproduces
   the finding from bytes. One self-contained file.
2. Add a dated section to `ARCHITECTURE.md` with an evidence table
   (byte offsets, observed values, variant count across versions).
3. If the finding is a decoding rule, also add a unit test that
   pins the byte pattern.

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
