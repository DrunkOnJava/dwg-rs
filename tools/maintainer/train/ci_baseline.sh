#!/bin/bash
# Commit the corpus counts a PR's own CI run measured: tools/maintainer/train/ci_baseline.sh <pr> <head>
#
# Called by land.sh when the "Corpus (public DWG)" job failed on <head>. Downloads that run's
# corpus-measurement artifact; when its counts differ from the PR's committed
# tools/corpus/baseline.tsv and none is worse than main's baseline, regenerates the baseline from
# the artifact (measure.py baseline), commits it and pushes. Nothing is built or run locally.
#
# Exit: 0 pushed (the new head is in $TRAIN_STATE/refresh-<pr>/pushed.sha), 1 the job failed for
#       another reason or a tool failed, 2 git or gh failure, 6 a count worse than main's.
{
set -u
P=$1
head=$2
. "$(dirname "$0")/common.sh"
OUT=$TRAIN_STATE/refresh-$P
D=$OUT/ci
rm -rf "${D:?}"; mkdir -p "$D"
run=$(gh run list -R "$TRAIN_REPO" -w ci.yml -c "$head" --json databaseId,event \
  --jq '[.[]|select(.event=="pull_request")][0].databaseId' 2>/dev/null)
[ -z "$run" ] || [ "$run" = null ] && { echo "no ci.yml pull_request run for ${head:0:9}"; exit 1; }
gh run download "$run" -R "$TRAIN_REPO" -n corpus-measurement -D "$D" > "$D/download.log" 2>&1 \
  || { echo "CI run $run has no corpus-measurement artifact"; exit 1; }
[ -s "$D/corpus-results.json" ] || { echo "CI run $run measured no drawing"; exit 1; }
B=$(gh pr view "$P" -R "$TRAIN_REPO" --json headRefName --jq .headRefName) || exit 2
cd "$TRAIN_CLONE" && git fetch -q origin main "$B" || exit 2
cd "$TRAIN_WORKTREE" || exit 2
git merge --abort > /dev/null 2>&1
git reset -q --hard
git checkout -q --detach "$head" || exit 2
if python3 -I tools/corpus/measure.py check --results "$D/corpus-results.json" > "$D/vs-pr.log" 2>&1; then
  echo "CI run $run: the corpus counts match the PR's baseline, so Corpus failed in another step"
  exit 1
fi
git show origin/main:tools/corpus/baseline.tsv > "$D/main-baseline.tsv"
python3 -I tools/corpus/measure.py check --results "$D/corpus-results.json" --baseline "$D/main-baseline.tsv" > "$D/vs-main.log" 2>&1
if grep -qE '^- (REGRESSION|MEASUREMENT FAILED)' "$D/vs-main.log"; then
  echo "REFUSED: CI run $run is worse than main's baseline: $(grep -E '^- (REGRESSION|MEASUREMENT FAILED)' "$D/vs-main.log" | head -3 | tr '\n' ' ')"
  exit 6
fi
python3 -I tools/corpus/measure.py baseline --results "$D/corpus-results.json" > "$D/baseline.log" 2>&1 || { echo "measure.py baseline FAILED"; exit 1; }
python3 -I tools/corpus/measure.py check --results "$D/corpus-results.json" > "$D/check.log" 2>&1 || { echo "measure.py check still fails after the baseline"; exit 1; }
git add tools/corpus/baseline.tsv
git diff --cached --quiet && { echo "CI run $run: the regenerated baseline equals the committed one"; exit 1; }
git commit -q -m "chore(corpus): regenerate the baseline from CI run $run" || exit 1
gh pr merge "$P" -R "$TRAIN_REPO" --disable-auto > /dev/null 2>&1
git push -q origin "HEAD:refs/heads/$B" || { echo "push FAILED"; exit 2; }
git rev-parse HEAD > "$OUT/pushed.sha"
echo "committed the baseline CI run $run measured ($(grep -c '^- IMPROVED' "$D/vs-main.log") row(s) improved or new vs main, 0 worse); pushed $(git rev-parse --short HEAD)"
exit 0
}
