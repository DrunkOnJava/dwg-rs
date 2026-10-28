#!/usr/bin/env bash
# Run the real, cli, contract and hardening targets of tools/ci/test-targets.txt,
# then the documentation examples.
#
# Usage: tools/ci/run_test_targets.sh [--check-only | cargo test options...]
#   e.g. tools/ci/run_test_targets.sh --release --all-features
#   --check-only checks the manifest and runs nothing.
#
# The toolchain is whatever cargo resolves: set RUSTUP_TOOLCHAIN to pick one.
#
# Fails when a tests/*.rs target is missing from the manifest, listed twice or
# classed with an unknown word, or when the manifest names a file that is not in
# tests/. Unit targets are never run, and neither are the inline #[cfg(test)]
# suites in src/: only the selected targets are built, so a unit target cannot
# slip into a run by accident.
#
# Targets without a skip= list share one cargo invocation (`--no-fail-fast`, so
# every failure of the run is shown); each target with a skip= list gets its
# own, because libtest's --skip applies to every binary of an invocation. Every
# invocation runs even when an earlier one failed, and the exit status is 1 if
# any did. Cargo refuses --doc together with --test, so the doc tests run last
# on their own.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
manifest="$root/tools/ci/test-targets.txt"

listed=$(awk '!/^#/ && NF >= 2 {print $2}' "$manifest" | sort)
present=$(cd "$root/tests" && ls ./*.rs | sed 's|^\./||; s|\.rs$||' | sort)
dupes=$(printf '%s\n' "$listed" | uniq -d)
missing=$(comm -13 <(printf '%s\n' "$listed") <(printf '%s\n' "$present"))
stale=$(comm -23 <(printf '%s\n' "$listed") <(printf '%s\n' "$present"))
unknown=$(awk '!/^#/ && NF >= 2 && $1 !~ /^(real|cli|contract|hardening|unit)$/ {print $1 " " $2}' "$manifest")
if [ -n "$dupes$missing$stale$unknown" ]; then
  [ -n "$missing" ] && echo "not classified in tools/ci/test-targets.txt: $missing" >&2
  [ -n "$stale" ] && echo "listed but not in tests/: $stale" >&2
  [ -n "$dupes" ] && echo "listed twice: $dupes" >&2
  [ -n "$unknown" ] && echo "unknown class: $unknown" >&2
  exit 1
fi
if [ "${1:-}" = "--check-only" ]; then
  echo "every tests/*.rs target is classified"
  exit 0
fi

plain=()
skipped=()
while read -r class target rest; do
  case "$class" in
    real | cli | contract | hardening) ;;
    *) continue ;;
  esac
  if [[ "${rest:-}" == skip=* ]]; then
    skipped+=("$target ${rest#skip=}")
  else
    plain+=("$target")
  fi
done < <(grep -v '^#' "$manifest")

status=0

if [ ${#plain[@]} -gt 0 ]; then
  selected=()
  for target in "${plain[@]}"; do selected+=(--test "$target"); done
  echo "== ${#plain[@]} targets: ${plain[*]}"
  cargo test "$@" --no-fail-fast "${selected[@]}" || status=1
fi

for entry in ${skipped[@]+"${skipped[@]}"}; do
  target="${entry%% *}"
  IFS=',' read -r -a names <<< "${entry#* }"
  filters=()
  for name in "${names[@]}"; do filters+=(--skip "$name"); done
  echo "== $target, without ${#names[@]} synthetic-fixture tests"
  cargo test "$@" --no-fail-fast --test "$target" -- "${filters[@]}" || status=1
done

echo "== documentation examples"
cargo test "$@" --doc || status=1

if [ "$status" -ne 0 ]; then
  echo "::error::a test target or the documentation examples failed; every failure is in the log above"
fi
exit "$status"
