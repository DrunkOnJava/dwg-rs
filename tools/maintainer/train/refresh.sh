#!/bin/bash
# Bring one PR branch up to main for the merge train, then push it. Nothing is built or run here:
# builds and tests run only on GitHub's runners, and land.sh reads their results.
#
#   tools/maintainer/train/refresh.sh <pr>
#
# Merges origin/main into the PR branch (never a force push) in the train's worktree. A PR stacked
# on one that has since landed merges against the newest commit of that PR it holds rather than the
# usual merge base. Before the merge, an ARCHITECTURE.md `## 7x.` section the PR adds whose letter
# main has since taken is moved to the next free letter, with every §7x in lines the PR added, and
# those lines follow any section main renumbered when the PR below landed (arch.py). Conflicts are resolved
# only in generated or tabular files: tools/corpus/baseline.tsv, tools/oracle/baseline.tsv and
# docs/spec-coverage.md take main's, and so does the coverage readme_status.py renders into
# README.md and STATUS.md when every conflicting line is rendered text (readme3.py); the PR's CI run
# then measures the merge, and land.sh commits the baselines it reports and renders them again; ARCHITECTURE.md and CLEANROOM.md take main's entries then the PR's
# when both appended at the same place; docs/data/spec-coverage.tsv and
# tests/fixtures/spec-examples/*.tsv merge cell by cell (tsv3.py), where a coverage note both sides
# rewrote keeps the PR's free text and the dropped text is posted on the PR. Any other conflict
# refuses. Then it regenerates docs/spec-coverage.md, commits, turns the PR's auto-merge off and
# pushes. Evidence for each step is in $TRAIN_STATE/refresh-<pr>/.
#
# Exit: 0 pushed or already current, 1 tool failure, 2 git or gh failure, 4 conflict outside the
#       generated set.
{
set -u
P=$1
. "$(dirname "$0")/common.sh"
OUT=$TRAIN_STATE/refresh-$P; mkdir -p "$OUT"; : > "$OUT/preferred.log"
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
# A PR stacked on one that has since landed holds that PR's commits but not main's squash of it, and
# the train's own commits on the landed PR (its merges of main, a renumbered section, baselines) then
# conflict with the stacked copies of the same lines. Such a PR merges against the newest commit of
# the landed PR it holds, the base the lanes chose by hand: main's side is then exactly what main
# changed since, the train's commits included, and the PR's side only its own work.
mb=$(git merge-base origin/main HEAD); stack=""; under=""
for n in $(git log --format=%s "$mb..origin/main" | sed -n 's/.*(#\([0-9][0-9]*\))$/\1/p'); do
  h=$(gh pr view "$n" -R "$TRAIN_REPO" --json headRefOid --jq .headRefOid 2> /dev/null) || continue
  git cat-file -e "$h^{commit}" 2> /dev/null || git fetch -q origin "refs/pull/$n/head" 2> /dev/null || continue
  c=$(git merge-base "$h" HEAD 2> /dev/null) || continue
  git merge-base --is-ancestor "$c" origin/main && continue
  if [ -z "$stack" ] || [ "$(git rev-list --count "$c..HEAD")" -lt "$(git rev-list --count "$stack..HEAD")" ]; then stack=$c; under=$n; fi
done
mb=${stack:-$mb}
echo "$base" > "$OUT/pr-head.sha"; echo "$mb" > "$OUT/merge-base.sha"
[ -n "$stack" ] && say "stacked on #$under: merging against ${stack:0:9}, the newest commit of #$under this branch holds"
map=$(python3 -I "$TRAIN_HERE/arch.py" renumber "$mb" origin/main 2> "$OUT/renumber.log") || { say "REFUSED: section renumber failed: $(cat "$OUT/renumber.log")"; exit 4; }
if [ -n "$map" ] && ! git diff --quiet; then
  git commit -qam "docs(architecture): renumber the new section ($map) after the sections main added" || exit 1
  say "renumbered ARCHITECTURE.md section $map"
fi

conflicted=0
merge_main() {
  [ -z "$stack" ] && { git merge --no-edit -q origin/main; return; }
  git merge-base --is-ancestor origin/main HEAD && return 0
  # git merge cannot take a base; merge-recursive can, and leaves the same index and conflict
  # stages, so MERGE_HEAD makes the commit below a merge of origin/main as git merge would.
  git rev-parse origin/main > "$(git rev-parse --git-path MERGE_HEAD)"
  echo "Merge origin/main into $B against ${stack:0:9}, the newest commit of #$under it holds" > "$(git rev-parse --git-path MERGE_MSG)"
  git merge-recursive "$stack" -- HEAD origin/main; rc=$?
  [ $rc = 0 ] && { git commit -q --no-edit; return; }
  [ $rc = 1 ] && return 1
  rm -f "$(git rev-parse --git-path MERGE_HEAD)" "$(git rev-parse --git-path MERGE_MSG)"; git reset -q --hard
  echo "[refresh #$P] merge-recursive against ${stack:0:9} failed (exit $rc); see $OUT/merge.log" >&3
  exit 2
}
exec 3>&1
if ! merge_main > "$OUT/merge.log" 2>&1; then
  conflicted=1
  for f in $(git diff --name-only --diff-filter=U); do
    case $f in
      tools/corpus/baseline.tsv|tools/oracle/baseline.tsv|docs/spec-coverage.md) git show "origin/main:$f" > "$f" ;;
      README.md|STATUS.md)
        python3 -I "$TRAIN_HERE/readme3.py" "$f" > "$OUT/readme3.log" 2>&1 || { say "REFUSED: real conflict in $f: $(cat "$OUT/readme3.log")"; git merge --abort; exit 4; }
        say "took main's rendered coverage in $f" ;;
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

python3 -I tools/ci/spec_coverage.py --write > "$OUT/spec.log" 2>&1 || { say "spec_coverage FAILED"; exit 1; }
python3 -I tools/ci/spec_coverage.py --check >> "$OUT/spec.log" 2>&1 || { say "spec_coverage --check FAILED"; exit 1; }
git add docs/spec-coverage.md docs/data/spec-coverage.tsv

if [ $conflicted = 1 ]; then
  git commit -q --no-edit || exit 1
elif ! git diff --cached --quiet; then
  git commit -q -m "docs(spec-coverage): regenerate the page on top of main" || exit 1
fi
git rev-parse HEAD > "$OUT/pushed.sha"
[ "$(git rev-parse HEAD)" = "$base" ] && { say "already current, nothing pushed"; exit 0; }
gh pr merge "$P" -R "$TRAIN_REPO" --disable-auto > /dev/null 2>&1
git push -q origin "HEAD:refs/heads/$B" || { say "push FAILED"; exit 2; }
say "pushed $(git rev-parse --short HEAD) to $B (auto-merge off until CI is green)"
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
