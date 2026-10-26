#!/bin/bash
# Land one PR the merge train has refreshed: tools/maintainer/train/land.sh <pr>
#
# Waits until the PR shows the head refresh.sh pushed, then for its Corpus job on that head, requires
# zero unresolved review threads, and turns squash auto-merge on. If main moves in a path that can
# change the generated files or the PR's ARCHITECTURE.md section suffix (src/ outside
# src/streaming/, tools/corpus/, docs/data/, the spec example fixtures, ARCHITECTURE.md) between the refresh and the merge, it runs `DRY=1 refresh.sh` against the new
# main: when every generated file comes out unchanged the PR keeps its CI results and lands;
# otherwise auto-merge goes off and it exits 8 for a real refresh.
#
# Exit: 0 merged, 1 failing check, 2 unresolved threads (before or after auto-merge), 3 timeout, 4 someone else pushed,
#       5 auto-merge refused, 6 closed, 7 went DIRTY, 8 main moved under it (refresh again).
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
still_valid() {
  [ -d "$OUT" ] || return 1
  DRY=1 bash "$TRAIN_HERE/refresh.sh" "$P" > "$OUT/dry.log" 2>&1 || return 1
  base=$(cat "$OUT/main.sha" 2>/dev/null)
  echo "#$P main moved; a dry refresh on ${base:0:9} leaves every generated file unchanged" >&2
  return 0
}
deadline=$(( $(date +%s) + 5400 ))
while :; do
  [ "$(date +%s)" -gt $deadline ] && { echo "#$P TIMEOUT waiting for Corpus"; exit 3; }
  cur=$(gh pr view "$P" -R "$R" --json headRefOid --jq .headRefOid 2>/dev/null)
  [ -n "$cur" ] && [ "$cur" != "$head" ] && { echo "#$P head moved to ${cur:0:9} (someone pushed); not landing"; exit 4; }
  c=$(gh pr checks "$P" -R "$R" --json name,state --jq '.[]|select(.name=="Corpus (public DWG)")|.state' 2>/dev/null | head -1)
  case $c in
    SUCCESS) break ;;
    FAILURE|CANCELLED|ERROR) echo "#$P Corpus=$c on ${head:0:9}"; exit 1 ;;
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
if moved && ! still_valid; then echo "#$P main moved under it since the refresh"; exit 8; fi
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
    if still_valid; then
      gh pr merge "$P" -R "$R" --squash --auto > /dev/null 2>&1 || { echo "#$P could not re-enable auto-merge"; exit 5; }
      continue
    fi
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
