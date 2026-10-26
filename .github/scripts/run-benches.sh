#!/usr/bin/env bash
#
# run-benches.sh — run every `[[bench]]` target and save the results
# under one criterion baseline name.
#
#   usage: run-benches.sh <baseline-name>
#
# Kept as a script rather than inline YAML because `.github/workflows/
# perf.yml` invokes it up to three times on one host: on the merge-base
# tree (baseline `main`), on the PR, and again on the PR for the
# confirmation re-run that the noise floor requires (issue #17). A
# target the tree being benched does not declare is skipped, so the
# PR's copy of this script also runs against an older merge-base.

set -euo pipefail

BASELINE_NAME="${1:?usage: run-benches.sh <baseline-name>}"

for bench in lz77 section_map object_walk metadata_parse libredwg_compare write_path; do
  if ! grep -qx "name = \"$bench\"" Cargo.toml; then
    echo "::notice::bench target $bench is not declared in $(pwd); skipped"
    continue
  fi
  cargo bench --bench "$bench" -- --save-baseline "$BASELINE_NAME"
done
