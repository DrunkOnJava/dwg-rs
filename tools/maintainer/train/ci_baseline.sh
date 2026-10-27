#!/bin/bash
# Check a PR's own CI measurement against main and commit the baselines it measured:
#   tools/maintainer/train/ci_baseline.sh <pr> <head>
#
# Called by land.sh once the "Corpus (public DWG)" job finished on <head>, passed or failed. Downloads
# that run's corpus-measurement artifact and:
# - refuses any corpus count worse than main's tools/corpus/baseline.tsv (CI compares only with the
#   PR's own baseline, so a PR committing worse counts would otherwise pass);
# - when the artifact holds the DXF oracle's oracle.tsv, refuses any agreeing count below main's
#   tools/oracle/baseline.tsv less the rows the PR itself retired (oracle_base.py), since a renamed
#   or removed field is not a fall;
# - regenerates tools/corpus/baseline.tsv when the measured counts differ from the committed ones,
#   commits oracle.tsv as tools/oracle/baseline.tsv when it differs (a refresh that took main's TSV
#   on a conflict would otherwise drop the PR's new rows), rewrites the oracle's coverage notes in
#   docs/data/spec-coverage.tsv, regenerates docs/spec-coverage.md, and renders the coverage in
#   README.md and STATUS.md again (readme_status.py, citing this run) when it disagrees;
# - commits and pushes whatever changed. Nothing is built or run locally.
#
# Exit: 0 pushed (new head in $TRAIN_STATE/refresh-<pr>/pushed.sha), 3 checked and nothing to change,
#       1 no measurement or a tool failure, 2 git or gh failure, 6 worse than main.
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
{ [ -z "$run" ] || [ "$run" = null ]; } && { echo "no ci.yml pull_request run for ${head:0:9}"; exit 1; }
gh run download "$run" -R "$TRAIN_REPO" -n corpus-measurement -D "$D" > "$D/download.log" 2>&1 \
  || { echo "CI run $run has no corpus-measurement artifact"; exit 1; }
[ -s "$D/corpus-results.json" ] || { echo "CI run $run measured no drawing"; exit 1; }
B=$(gh pr view "$P" -R "$TRAIN_REPO" --json headRefName --jq .headRefName) || exit 2
cd "$TRAIN_CLONE" && git fetch -q origin main "$B" || exit 2
cd "$TRAIN_WORKTREE" || exit 2
git merge --abort > /dev/null 2>&1
git reset -q --hard
git checkout -q --detach "$head" || exit 2

# Main as the PR was refreshed onto it: if main moved since, land.sh refreshes the PR, and a newer
# main must not turn this older measurement into a regression.
mainref=$(cat "$OUT/main.sha" 2>/dev/null); git cat-file -e "${mainref:-x}^{commit}" 2>/dev/null || mainref=origin/main
git show "$mainref:tools/corpus/baseline.tsv" > "$D/main-baseline.tsv"
python3 -I tools/corpus/measure.py check --results "$D/corpus-results.json" --baseline "$D/main-baseline.tsv" > "$D/vs-main.log" 2>&1
if grep -qE '^- (REGRESSION|MEASUREMENT FAILED)' "$D/vs-main.log"; then
  echo "REFUSED: CI run $run is worse than main's corpus baseline: $(grep -E '^- (REGRESSION|MEASUREMENT FAILED)' "$D/vs-main.log" | head -3 | tr '\n' ' ')"
  exit 6
fi
changed=""
if ! python3 -I tools/corpus/measure.py check --results "$D/corpus-results.json" > "$D/vs-pr.log" 2>&1; then
  python3 -I tools/corpus/measure.py baseline --results "$D/corpus-results.json" > "$D/baseline.log" 2>&1 || { echo "measure.py baseline FAILED"; exit 1; }
  python3 -I tools/corpus/measure.py check --results "$D/corpus-results.json" > "$D/check.log" 2>&1 || { echo "measure.py check still fails after the baseline"; exit 1; }
  changed="corpus baseline ($(grep -c '^- IMPROVED' "$D/vs-main.log") row(s) improved or new vs main)"
fi

if [ -s "$D/oracle.tsv" ] && [ -f tools/oracle/baseline.tsv ]; then
  prhead=$(cat "$OUT/pr-head.sha" 2>/dev/null); mb=$(cat "$OUT/merge-base.sha" 2>/dev/null)
  if [ -n "$prhead" ] && [ -n "$mb" ]; then
    python3 -I "$TRAIN_HERE/oracle_base.py" "$mb" "$prhead" "$mainref" > "$D/main-oracle.tsv" || { echo "oracle_base.py FAILED"; exit 1; }
  else
    git show "$mainref:tools/oracle/baseline.tsv" > "$D/main-oracle.tsv"
  fi
  drops=$(python3 -I - "$D/main-oracle.tsv" "$D/oracle.tsv" <<'EOF'
import sys
def table(p):
    out = {}
    for n, line in enumerate(open(p, encoding='utf-8')):
        f = line.rstrip('\n').split('\t')
        if n == 0 or len(f) != 9 or line.startswith('#'):
            continue
        out[(f[0], f[2], f[3], f[4])] = int(f[5]) + int(f[6])
    return out
base, now = table(sys.argv[1]), table(sys.argv[2])
for k, b in base.items():
    got = now.get(k)
    if (got is None and b > 0) or (got is not None and got < b):
        print(f"{' '.join(k)}: {b} agreeing on main, {got if got is not None else 'absent'} now")
EOF
)
  [ -n "$drops" ] && { echo "REFUSED: CI run $run's DXF oracle fell below main's: $(echo "$drops" | head -3 | tr '\n' ' ')"; exit 6; }
  if ! cmp -s "$D/oracle.tsv" tools/oracle/baseline.tsv; then
    cp "$D/oracle.tsv" tools/oracle/baseline.tsv
    changed="${changed:+$changed, }oracle baseline"
  fi
  if [ -s "$D/oracle.log" ] && grep -q '^== Summary by spec section' "$D/oracle.log" && [ -f tools/oracle/spec_notes.py ]; then
    python3 -I tools/oracle/spec_notes.py "$D/oracle.log" docs/data/spec-coverage.tsv > "$D/oracle-notes.log" 2>&1 || { echo "spec_notes.py FAILED"; exit 1; }
    python3 -I tools/ci/spec_coverage.py --write > "$D/spec.log" 2>&1 || { echo "spec_coverage.py FAILED"; exit 1; }
    git diff --quiet -- docs/data/spec-coverage.tsv docs/spec-coverage.md || changed="${changed:+$changed, }oracle coverage notes"
  fi
fi

if [ -f tools/maintainer/readme_status.py ] && ! python3 -I tools/maintainer/readme_status.py --check > "$D/readme-check.log" 2>&1; then
  day=$(gh run view "$run" -R "$TRAIN_REPO" --json createdAt --jq '.createdAt[0:10]' 2>/dev/null)
  python3 -I tools/maintainer/readme_status.py --write --run "$run" --date "$day" > "$D/readme.log" 2>&1 || { echo "readme_status.py --write FAILED: $(cat "$D/readme.log")"; exit 1; }
  changed="${changed:+$changed, }README and STATUS coverage"
fi

[ -z "$changed" ] && { echo "CI run $run: no count worse than main, baselines and README already current"; exit 3; }
git add tools/corpus/baseline.tsv tools/oracle/baseline.tsv docs/data/spec-coverage.tsv docs/spec-coverage.md README.md STATUS.md
git commit -q -m "chore(corpus): commit the baselines CI run $run measured" || exit 1
gh pr merge "$P" -R "$TRAIN_REPO" --disable-auto > /dev/null 2>&1
git push -q origin "HEAD:refs/heads/$B" || { echo "push FAILED"; exit 2; }
git rev-parse HEAD > "$OUT/pushed.sha"
echo "committed the $changed CI run $run measured, nothing worse than main; pushed $(git rev-parse --short HEAD)"
exit 0
}
