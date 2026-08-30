#!/usr/bin/env bash
#
# run-benches.sh — run every `[[bench]]` target and save the results
# under one criterion baseline name.
#
#   usage: run-benches.sh <baseline-name>
#
# Kept as a script rather than inline YAML because `.github/workflows/
# perf.yml` invokes it twice: once for the measurement pass, and again
# for the confirmation re-run that the noise floor requires before
# failing the job (issue #17).

set -euo pipefail

BASELINE_NAME="${1:?usage: run-benches.sh <baseline-name>}"

for bench in lz77 section_map object_walk metadata_parse libredwg_compare write_path; do
  cargo bench --bench "$bench" -- --save-baseline "$BASELINE_NAME"
done
