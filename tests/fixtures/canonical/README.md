# Canonical test corpus (placeholder)

This directory is the planned home for a minimal, Apache-2-licensed,
self-generated corpus of DWG files that CI can run `coverage_report`
against — see task #94.

## Current state

**Empty.** The CI `coverage-smoke` job (`.github/workflows/ci.yml`)
checks for the presence of at least one `.dwg` file here and skips
gracefully with a warning if none exists. No CI failure today — but
also no coverage regression protection.

## Why empty

Generating a DWG file requires the full write pipeline (file_writer
Stages 2–5; see `src/file_writer.rs` module docs). Stage 1 is shipped
but Stages 2–5 are not. Until `DwgFile::to_bytes()` exists, we have
two options:

1. **Vendor sample files** from an external corpus (nextgis/dwg_samples
   is the obvious candidate, but it has no declared license — we
   cannot redistribute its contents).
2. **Generate synthetic fixtures** via the write path. Blocked on
   Stages 2–5.
3. **Hand-craft byte fixtures** by writing out each section manually.
   Extremely tedious; one-per-version would be hundreds of lines of
   careful byte arithmetic.

When the write path matures, this directory should be populated with
synthetic files via a `cargo run --release --example build_fixtures`
step, committed as the canonical CI corpus.

## When this is populated

The CI `coverage-smoke` job will:
- Run `cargo run --release --example coverage_report -- tests/fixtures/canonical`
- Parse the aggregate `ratio%` from the output
- Fail the build if the ratio drops below 20%

A regression that breaks dimension dispatch (task #71), LINE
decoding, or the entity walker will trip that guard.
