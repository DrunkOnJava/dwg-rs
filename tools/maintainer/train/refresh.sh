#!/bin/bash
# Bring one PR branch up to main for the merge train, then push it.
#
#   tools/maintainer/train/refresh.sh <pr>          refresh and push
#   DRY=1 tools/maintainer/train/refresh.sh <pr>    exit 0 when merging today's main changes no generated file
#
# Merges origin/main into the PR branch (never a force push) in the train's worktree. Before the
# merge, an ARCHITECTURE.md `## 7x.` section the PR adds whose letter main has since taken is moved
# to the next free letter, with every §7x in lines the PR added (arch.py). Conflicts are resolved
# only in generated or tabular files: tools/corpus/baseline.tsv and docs/spec-coverage.md are
# regenerated; ARCHITECTURE.md and CLEANROOM.md take main's entries then the PR's when both appended at the same
# place; docs/data/spec-coverage.tsv and tests/fixtures/spec-examples/*.tsv merge cell by cell
# (tsv3.py), where a coverage note both sides rewrote keeps the PR's free text and the dropped text
# is posted on the PR. Any other conflict refuses.
#
# Then it builds, runs the pinned public corpus and refuses on any count that is worse than main's
# baseline, regenerates the baseline, runs spec_examples (applying --update unless an example loses
# `closes`), rewrites the DXF oracle's coverage notes, regenerates docs/spec-coverage.md, commits,
# turns the PR's auto-merge off and pushes. Evidence for each step is in $TRAIN_STATE/refresh-<pr>/.
#
# Exit: 0 pushed or already current (DRY: still valid), 1 build or tool failure, 2 git or gh failure,
#       4 conflict outside the generated set, 6 regression against main, 9 DRY: needs a real refresh.
{
set -u
P=$1
. "$(dirname "$0")/common.sh"
OUT=$TRAIN_STATE/refresh-$P; mkdir -p "$OUT"; : > "$OUT/preferred.log"
C=$TRAIN_CORPUS
say() { echo "[refresh #$P] $*"; }

B=$(gh pr view "$P" -R "$TRAIN_REPO" --json headRefName --jq .headRefName) || exit 2
cd "$TRAIN_CLONE" && git fetch -q origin main "$B" || exit 2
git rev-parse origin/main > "$OUT/main.sha"
[ -d "$TRAIN_WORKTREE" ] || git worktree add -q --detach "$TRAIN_WORKTREE" origin/main || exit 2
cd "$TRAIN_WORKTREE" || exit 2
git merge --abort > /dev/null 2>&1
# The worktree is the train's own: a failed or dry refresh can leave regenerated files behind.
git reset -q --hard
git checkout -q --detach "origin/$B" || exit 2
base=$(git rev-parse HEAD)
map=$(python3 -I "$TRAIN_HERE/arch.py" renumber "$(git merge-base origin/main HEAD)" origin/main 2> "$OUT/renumber.log") || { say "REFUSED: section renumber failed: $(cat "$OUT/renumber.log")"; exit 4; }
[ -n "${DRY:-}" ] && [ -n "$map" ] && { say "DRY: section letters collide ($map)"; exit 9; }
if [ -n "$map" ]; then
  git commit -qam "docs(architecture): renumber the new section ($map) after the sections main added" || exit 1
  say "renumbered ARCHITECTURE.md section $map"
fi

conflicted=0
if ! git merge --no-edit -q origin/main > "$OUT/merge.log" 2>&1; then
  [ -n "${DRY:-}" ] && { git merge --abort > /dev/null 2>&1; say "DRY: merge with main conflicts"; exit 9; }
  conflicted=1
  for f in $(git diff --name-only --diff-filter=U); do
    case $f in
      tools/corpus/baseline.tsv|tools/oracle/baseline.tsv|docs/spec-coverage.md) git show "origin/main:$f" > "$f" ;;
      ARCHITECTURE.md|CLEANROOM.md)
        python3 -I "$TRAIN_HERE/arch.py" resolve "$f" > "$OUT/arch.log" 2>&1 || { say "REFUSED: real conflict in $f: $(cat "$OUT/arch.log")"; git merge --abort; exit 4; }
        say "appended $f entries after main's" ;;
      docs/data/spec-coverage.tsv|tests/fixtures/spec-examples/*.tsv)
        prefer=""; [ "$f" = docs/data/spec-coverage.tsv ] && prefer="--prefer-ours 7"
        python3 -I "$TRAIN_HERE/tsv3.py" "$f" $prefer > "$OUT/tsv3.log" 2>&1 || { say "REFUSED: real conflict in $f: $(cat "$OUT/tsv3.log")"; git merge --abort; exit 4; }
        grep '^PREFERRED THE PR' "$OUT/tsv3.log" >> "$OUT/preferred.log"
        say "merged $f cell by cell ($(grep -c '^PREFERRED THE PR' "$OUT/tsv3.log") cell(s) where main's text was replaced by the PR's)" ;;
      *) say "REFUSED: real conflict in $f"; git merge --abort; exit 4 ;;
    esac
    git add "$f"
  done
fi

say "build"
if ! cargo build --release --all-features --examples > "$OUT/build.log" 2>&1; then
  # The machine-wide governor (when present) refuses a build while memory or CPU is under pressure:
  # that is not this PR's failure, so the train retries it later.
  grep -qs "governor: no lease" "$OUT/build.log" && { say "DEFERRED: no build lease while the machine is under pressure"; exit 75; }
  say "build FAILED ($OUT/build.log)"; exit 1
fi
say "corpus run"
python3 -I tools/corpus/fetch.py --dest "$C" > "$OUT/fetch.log" 2>&1 || { say "fetch FAILED"; exit 1; }
python3 -I tools/corpus/measure.py run --corpus "$C" --bin "$CARGO_TARGET_DIR/release/examples" --out "$OUT/r.json" > "$OUT/run.log" 2>&1 || { say "run FAILED"; exit 1; }
git show origin/main:tools/corpus/baseline.tsv > "$OUT/main-baseline.tsv"
python3 -I tools/corpus/measure.py check --results "$OUT/r.json" --baseline "$OUT/main-baseline.tsv" > "$OUT/vs-main.log" 2>&1
if grep -qE '^- (REGRESSION|MEASUREMENT FAILED)' "$OUT/vs-main.log"; then
  say "REFUSED: regression vs main's baseline: $(grep -E '^- (REGRESSION|MEASUREMENT FAILED)' "$OUT/vs-main.log" | head -3 | tr '\n' ' ')"
  git merge --abort > /dev/null 2>&1
  exit 6
fi
say "vs main: $(grep -c '^- IMPROVED' "$OUT/vs-main.log") improved row(s), 0 regressions"
python3 -I tools/corpus/measure.py baseline --results "$OUT/r.json" > "$OUT/baseline.log" 2>&1 || { say "baseline FAILED"; exit 1; }
python3 -I tools/corpus/measure.py check --results "$OUT/r.json" > "$OUT/check.log" 2>&1 || { say "check FAILED after baseline"; exit 1; }
if [ -f examples/spec_examples.rs ]; then
  if ! "$CARGO_TARGET_DIR/release/examples/spec_examples" > "$OUT/spec-examples.log" 2>&1; then
    F=tests/fixtures/spec-examples/r14-objects.tsv
    cp "$F" "$OUT/r14-objects.before.tsv"
    "$CARGO_TARGET_DIR/release/examples/spec_examples" --update > "$OUT/spec-examples-update.log" 2>&1 || { say "spec_examples --update FAILED"; exit 1; }
    worse=$(python3 -I - "$OUT/r14-objects.before.tsv" "$F" <<'EOF'
import csv, sys
def rows(p):
    with open(p, encoding='utf-8') as fh:
        lines = [l for l in fh if not l.startswith('#')]
    r = list(csv.DictReader(lines, delimiter='\t'))
    return {tuple(v for c, v in x.items() if c != 'expect'): x.get('expect', '') for x in r}
a, b = rows(sys.argv[1]), rows(sys.argv[2])
print(' '.join(f'{k}:{a[k]}->{b.get(k)}' for k in a if a[k] == 'closes' and b.get(k) != 'closes'))
EOF
)
    [ -n "$worse" ] && { say "REFUSED: spec example regression: $worse"; git merge --abort > /dev/null 2>&1; exit 6; }
    "$CARGO_TARGET_DIR/release/examples/spec_examples" > "$OUT/spec-examples.log" 2>&1 || { say "spec_examples still failing after --update"; exit 1; }
    say "spec_examples --update applied (no closes row lost)"
    git add tests/fixtures/spec-examples
  fi
fi
if [ -d examples/dxf_oracle ] && [ -f tools/oracle/spec_notes.py ]; then
  "$CARGO_TARGET_DIR/release/examples/dxf_oracle" "$C" > "$OUT/oracle.txt" 2> "$OUT/oracle.err"
  grep -q '^== Summary by spec section' "$OUT/oracle.txt" || { say "dxf_oracle produced no summary ($OUT/oracle.err)"; exit 1; }
  python3 -I tools/oracle/spec_notes.py "$OUT/oracle.txt" docs/data/spec-coverage.tsv > "$OUT/oracle-notes.log" 2>&1 || { say "spec_notes FAILED"; exit 1; }
fi
if [ -f tools/oracle/baseline.tsv ] && grep -q -- '--baseline' examples/dxf_oracle/main.rs 2>/dev/null; then
  git show origin/main:tools/oracle/baseline.tsv > "$OUT/main-oracle.tsv" 2>/dev/null || cp tools/oracle/baseline.tsv "$OUT/main-oracle.tsv"
  if ! "$CARGO_TARGET_DIR/release/examples/dxf_oracle" "$C" --tsv "$OUT/oracle.tsv" --baseline "$OUT/main-oracle.tsv" > "$OUT/oracle-ratchet.log" 2>&1; then
    say "REFUSED: DXF oracle agreement fell against main's: $(grep -E 'agreeing in the baseline|absent from this run' "$OUT/oracle-ratchet.log" | head -3 | tr '\n' ' ')"
    git merge --abort > /dev/null 2>&1
    exit 6
  fi
  cp "$OUT/oracle.tsv" tools/oracle/baseline.tsv
fi
python3 -I tools/ci/spec_coverage.py --write > "$OUT/spec.log" 2>&1 || { say "spec_coverage FAILED"; exit 1; }
python3 -I tools/ci/spec_coverage.py --check >> "$OUT/spec.log" 2>&1 || { say "spec_coverage --check FAILED"; exit 1; }
if [ -n "${DRY:-}" ]; then
  if git diff --quiet HEAD -- tools/corpus/baseline.tsv tools/oracle/baseline.tsv docs/spec-coverage.md docs/data/spec-coverage.tsv tests/fixtures/spec-examples; then
    say "DRY: generated files unchanged on current main"; exit 0
  fi
  say "DRY: generated files differ on current main"; exit 9
fi
git add tools/corpus/baseline.tsv docs/spec-coverage.md docs/data/spec-coverage.tsv; [ -f tools/oracle/baseline.tsv ] && git add tools/oracle/baseline.tsv

if [ $conflicted = 1 ]; then
  git commit -q --no-edit || exit 1
elif ! git diff --cached --quiet; then
  git commit -q -m "chore(corpus): regenerate the baseline on top of main" || exit 1
fi
git rev-parse HEAD > "$OUT/pushed.sha"
[ "$(git rev-parse HEAD)" = "$base" ] && { say "already current, nothing pushed"; exit 0; }
gh pr merge "$P" -R "$TRAIN_REPO" --disable-auto > /dev/null 2>&1
git push -q origin "HEAD:refs/heads/$B" || { say "push FAILED"; exit 2; }
say "pushed $(git rev-parse --short HEAD) to $B (auto-merge off until Corpus is green)"
if [ -s "$OUT/preferred.log" ]; then
  python3 -I - "$OUT/preferred.log" "$(git rev-parse --short HEAD)" > "$OUT/preferred-comment.md" <<'EOF'
import re, sys
log, sha = sys.argv[1], sys.argv[2]
out = [f"Merge-train note for {sha}: main had changed these docs/data/spec-coverage.tsv notes since this PR's base, "
       "and this PR rewrote them too. The PR's free text was kept and main's was dropped. "
       "Tool-written sentences (DXF oracle, spec examples) were kept and regenerated. Check that nothing below still needs saying:", ""]
for line in open(log, encoding='utf-8'):
    m = re.match(r"PREFERRED THE PR: (\S+) column \d+ \(free text\): kept (.*); dropped main's (.*) \(base (.*)\)$", line.strip())
    out.append(f"- §{m.group(1)}: dropped main's text: {m.group(3)}" if m else f"- {line.strip()}")
print('\n'.join(out))
EOF
  gh pr comment "$P" -R "$TRAIN_REPO" --body-file "$OUT/preferred-comment.md" > /dev/null 2>&1 && say "posted the dropped note text to the PR"
fi
exit 0
}
