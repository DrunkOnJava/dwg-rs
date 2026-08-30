#!/usr/bin/env bash
#
# criterion-gate.sh — compare two criterion baselines and decide whether
# any benchmark regressed enough to fail CI.
#
#   usage: criterion-gate.sh <base-baseline> <candidate-baseline> [tripped-file]
#
# Reads criterion's own `estimates.json` (nanoseconds) rather than
# scraping `critcmp`'s formatted text, so the parse cannot drift with a
# column-layout change and the absolute delta is available exactly.
#
# A benchmark "trips" only when BOTH hold:
#
#   ratio = cand_mean / base_mean  >=  $RATIO_THRESHOLD   (default 1.20)
#   delta = cand_mean - base_mean  >=  $ABS_FLOOR_NS      (default 5000)
#
# The absolute floor exists because a ratio alone cannot tell a real
# regression from GitHub-runner noise on a microsecond-scale bench —
# see the failure-rule comment at the top of `.github/workflows/perf.yml`
# and issue #17.
#
# The table is printed on every invocation, regardless of the verdict.
#
# Exit codes:
#   0 — every comparable benchmark is within the thresholds
#   1 — at least one benchmark tripped (names written to [tripped-file])
#   2 — nothing comparable found (cold cache / missing baseline)
#   3 — a required tool is missing

set -uo pipefail

BASE_NAME="${1:-}"
CAND_NAME="${2:-}"
TRIPPED_FILE="${3:-}"

if [ -z "$BASE_NAME" ] || [ -z "$CAND_NAME" ]; then
  echo "usage: criterion-gate.sh <base-baseline> <candidate-baseline> [tripped-file]" >&2
  exit 3
fi

CRITERION_DIR="${CRITERION_DIR:-target/criterion}"
RATIO_THRESHOLD="${RATIO_THRESHOLD:-1.20}"
ABS_FLOOR_NS="${ABS_FLOOR_NS:-5000}"

if ! command -v jq >/dev/null 2>&1; then
  echo "::error::jq is required by criterion-gate.sh but was not found" >&2
  exit 3
fi

if [ -n "$TRIPPED_FILE" ]; then
  : > "$TRIPPED_FILE"
fi

compared=0
tripped=0

printf '%-44s %13s %13s %7s %12s  %s\n' \
  'benchmark' "$BASE_NAME(ns)" "$CAND_NAME(ns)" 'ratio' 'delta(ns)' 'verdict'
printf '%s\n' '--------------------------------------------------------------------------------------------------------'

while IFS= read -r cand_est; do
  cand_dir="$(dirname "$cand_est")"
  bench_dir="$(dirname "$cand_dir")"
  base_est="$bench_dir/$BASE_NAME/estimates.json"
  [ -f "$base_est" ] || continue

  name="$(jq -r '.full_id // empty' "$cand_dir/benchmark.json" 2>/dev/null)"
  if [ -z "$name" ]; then
    name="${bench_dir#"$CRITERION_DIR"/}"
  fi

  base_ns="$(jq -r '.mean.point_estimate' "$base_est" 2>/dev/null)"
  cand_ns="$(jq -r '.mean.point_estimate' "$cand_est" 2>/dev/null)"
  case "$base_ns" in ''|null) continue ;; esac
  case "$cand_ns" in ''|null) continue ;; esac

  read -r ratio delta trip over_ratio <<EOF
$(awk -v b="$base_ns" -v c="$cand_ns" -v rt="$RATIO_THRESHOLD" -v af="$ABS_FLOOR_NS" 'BEGIN {
    ratio = (b + 0 > 0) ? (c + 0) / (b + 0) : 0;
    delta = (c + 0) - (b + 0);
    over  = (ratio >= rt + 0) ? 1 : 0;
    trip  = (over && delta >= af + 0) ? 1 : 0;
    printf "%.3f %.0f %d %d", ratio, delta, trip, over;
  }')
EOF

  compared=$((compared + 1))
  if [ "$trip" = "1" ]; then
    verdict='REGRESSION'
    tripped=$((tripped + 1))
    if [ -n "$TRIPPED_FILE" ]; then
      printf '%s\n' "$name" >> "$TRIPPED_FILE"
    fi
  elif [ "$over_ratio" = "1" ]; then
    verdict="under ${ABS_FLOOR_NS}ns noise floor"
  else
    verdict='ok'
  fi

  printf '%-44s %13.0f %13.0f %7s %12s  %s\n' \
    "$name" "$base_ns" "$cand_ns" "$ratio" "$delta" "$verdict"
done < <(find "$CRITERION_DIR" -type f -path "*/$CAND_NAME/estimates.json" 2>/dev/null | sort)

echo
if [ "$compared" -eq 0 ]; then
  echo "criterion-gate: no benchmark had both a '$BASE_NAME' and a '$CAND_NAME' baseline"
  exit 2
fi

echo "criterion-gate: compared $compared benchmark(s); \
ratio threshold ${RATIO_THRESHOLD}x, absolute floor ${ABS_FLOOR_NS}ns"
if [ "$tripped" -gt 0 ]; then
  echo "criterion-gate: $tripped benchmark(s) tripped both thresholds"
  exit 1
fi
echo "criterion-gate: all benchmarks within thresholds"
exit 0
