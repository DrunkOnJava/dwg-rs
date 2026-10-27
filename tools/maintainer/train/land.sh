#!/bin/bash
# Land one PR the merge train has refreshed: tools/maintainer/train/land.sh <pr>
#
# Waits until the PR shows the head refresh.sh pushed, then for its Corpus job on that head. Whether
# that job passed or failed, ci_baseline.sh checks its measurement against main (corpus counts and
# DXF oracle agreement), refuses anything worse, and commits the corpus and oracle baselines the job
# measured when they differ from the committed ones; the wait then starts again on the new head, at
# most four times. A failed job whose measurement needs no change failed in another step. Then it requires zero unresolved review threads
# and turns squash auto-merge on. If main moves in a path that can change the measured counts or the
# PR's ARCHITECTURE.md section suffix (src/ outside src/streaming/, tools/corpus/, docs/data/, the
# spec example fixtures, ARCHITECTURE.md) before the merge, auto-merge goes off and it exits 8 for
# a fresh refresh. Nothing is built or run locally.
#
# Exit: 0 merged, 1 failing check, 2 unresolved threads (before or after auto-merge), 3 timeout, 4 someone else pushed,
#       5 auto-merge refused, 6 closed, or a count worse than main's, 7 went DIRTY, 8 main moved under it (refresh again).
{
P=$1
. "$(dirname "$0")/common.sh"
R=$TRAIN_REPO
OUT=$TRAIN_STATE/refresh-$P
head=$(cat "$OUT/pushed.sha" 2>/dev/null)
if [ -n "$head" ]; then
  for _ in $(seq 1 20); do
    [ "$(gh pr view "$P" -R "$R" --json headRefOid --jq .headRefOid 2>/dev/null)" = "$head" ] && break
    sleep 15
  done
else
  head=$(gh pr view "$P" -R "$R" --json headRefOid --jq .headRefOid)
fi
base=$(cat "$OUT/main.sha" 2>/dev/null)
moved() {
  [ -z "$base" ] && return 1
  now=$(gh api "repos/$R/commits/main" --jq .sha 2>/dev/null) || return 1
  [ "$now" = "$base" ] && return 1
  gh api "repos/$R/compare/$base...$now" --jq '.files[].filename' 2>/dev/null \
    | grep -v '^src/streaming/' | grep -qE '^(src/|tools/corpus/|docs/data/|tests/fixtures/spec-examples/|ARCHITECTURE\.md$)'
}
note() { echo "$(date +%H:%M) TRAIN #$P $*" >> "$TRAIN_STATE/train.log"; }
# Runs cancelled to free the runners for PRs ahead in the queue are re-run once the train reaches this PR.
revive() {
  for run in $(gh run list -R "$R" -c "$1" --json databaseId,conclusion --jq '.[]|select(.conclusion=="cancelled")|.databaseId' 2>/dev/null); do
    gh run rerun "$run" -R "$R" --failed > /dev/null 2>&1 && note "re-ran cancelled run $run on ${1:0:9}"
  done
}
revive "$head"
deadline=$(( $(date +%s) + 5400 ))
regen=0
while :; do
  [ "$(date +%s)" -gt $deadline ] && { echo "#$P TIMEOUT waiting for Corpus"; exit 3; }
  cur=$(gh pr view "$P" -R "$R" --json headRefOid --jq .headRefOid 2>/dev/null)
  [ -n "$cur" ] && [ "$cur" != "$head" ] && { echo "#$P head moved to ${cur:0:9} (someone pushed); not landing"; exit 4; }
  c=$(gh pr checks "$P" -R "$R" --json name,state --jq '.[]|select(.name=="Corpus (public DWG)")|.state' 2>/dev/null | head -1)
  case $c in
    CANCELLED) revive "$head" ;;
    SUCCESS|FAILURE|ERROR)
      [ "$regen" -ge 4 ] && { echo "#$P Corpus=$c on ${head:0:9} after $regen regenerated baselines"; exit 1; }
      msg=$(bash "$TRAIN_HERE/ci_baseline.sh" "$P" "$head" 2>&1); rc=$?
      case $rc in
        0) head=$(cat "$OUT/pushed.sha"); regen=$((regen + 1)); note "$msg"; deadline=$(( $(date +%s) + 5400 )) ;;
        3) [ "$c" = SUCCESS ] && break; echo "#$P Corpus=$c on ${head:0:9}: $msg, so it failed in another step"; exit 1 ;;
        6) echo "#$P $msg"; exit 6 ;;
        *) echo "#$P Corpus=$c on ${head:0:9}: $msg"; exit 1 ;;
      esac ;;
  esac
  sleep 60
done
unresolved() {
  gh api graphql -F owner="${R%/*}" -F name="${R#*/}" -F n="$P" \
    -f query='query($owner:String!,$name:String!,$n:Int!){repository(owner:$owner,name:$name){pullRequest(number:$n){reviewThreads(first:100){nodes{isResolved}}}}}' \
    --jq '[.data.repository.pullRequest.reviewThreads.nodes[]|select(.isResolved==false)]|length' 2>/dev/null
}
u=$(unresolved)
[ "$u" != 0 ] && { echo "#$P Corpus green but $u unresolved review thread(s)"; exit 2; }
if moved; then echo "#$P main moved under it since the refresh"; exit 8; fi
gh pr merge "$P" -R "$R" --squash --auto > /dev/null 2>&1 || { echo "#$P could not enable auto-merge"; exit 5; }
echo "#$P Corpus green, 0 threads, auto-merge on"
while :; do
  [ "$(date +%s)" -gt $((deadline + 3600)) ] && { gh pr merge "$P" -R "$R" --disable-auto > /dev/null 2>&1; echo "#$P TIMEOUT waiting for merge; auto-merge off"; exit 3; }
  s=$(gh pr view "$P" -R "$R" --json state,mergeStateStatus --jq '"\(.state) \(.mergeStateStatus)"' 2>/dev/null)
  case $s in
    MERGED*) echo "#$P MERGED as $(gh pr view "$P" -R "$R" --json mergeCommit --jq '.mergeCommit.oid[0:9]')"; exit 0 ;;
    CLOSED*) echo "#$P CLOSED without merge"; exit 6 ;;
    *DIRTY) gh pr merge "$P" -R "$R" --disable-auto > /dev/null 2>&1; echo "#$P went DIRTY before merging"; exit 7 ;;
    # With strict up-to-date on, a branch behind main never merges; only a real refresh pushes main in.
    *BEHIND) gh pr merge "$P" -R "$R" --disable-auto > /dev/null 2>&1; echo "#$P is behind main; auto-merge off"; exit 8 ;;
  esac
  if moved; then
    [ "$(gh pr view "$P" -R "$R" --json state --jq .state 2>/dev/null)" = MERGED ] && continue
    gh pr merge "$P" -R "$R" --disable-auto > /dev/null 2>&1
    echo "#$P main moved under it while waiting; auto-merge off"; exit 8
  fi
  f=$(gh pr checks "$P" -R "$R" --json name,state --jq '[.[]|select(.state=="FAILURE")|.name]|join(", ")' 2>/dev/null)
  [ -n "$f" ] && { gh pr merge "$P" -R "$R" --disable-auto > /dev/null 2>&1; echo "#$P failing: $f; auto-merge off"; exit 1; }
  # A reviewer can open a thread after auto-merge went on; the ruleset then holds the PR until
  # the deadline, so hand it back instead of stalling the train.
  case $(unresolved) in
    ''|*[!0-9]*|0) ;;
    *) gh pr merge "$P" -R "$R" --disable-auto > /dev/null 2>&1
       echo "#$P review thread opened after auto-merge went on; auto-merge off"; exit 2 ;;
  esac
  sleep 60
done
}
