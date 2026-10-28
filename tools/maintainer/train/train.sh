#!/bin/bash
# The merge train: lands queued PRs one at a time.
#
#   tools/maintainer/train/train.sh [pr...]
#
# Appends any PR numbers given to $TRAIN_STATE/queue (one per line), then pops the queue top-down:
# refresh.sh merges main into each PR and pushes, land.sh waits for its GitHub Actions run, commits
# the corpus counts that run measured when they moved, and lands it. Nothing is built or tested on
# this machine. A PR that went DIRTY or found main moved under it is refreshed once more. A PR
# stacked on another PR's branch waits at the end of the queue until GitHub retargets it to main,
# and so does one on main that holds commits of an older open PR, until that PR lands.
# Anything else that stops a PR is logged and the train moves on. Append to the queue file to add
# work while it runs; this script is read once, so restart the train to pick up a change to it.
# Progress: $TRAIN_STATE/train.log.
{
. "$(dirname "$0")/common.sh"
Q=$TRAIN_STATE/queue
L=$TRAIN_STATE/train.log
touch "$Q"
for P in "$@"; do grep -qx "$P" "$Q" || echo "$P" >> "$Q"; done
log() { echo "$(date +%H:%M) $*" >> "$L"; }
waiting=0
wait_at_end() {
  echo "$P" >> "$Q"
  waiting=$((waiting + 1))
  log "TRAIN #$P $1"
  [ "$waiting" -ge "$(grep -c -E '^[0-9]+$' "$Q")" ] && { log "TRAIN every queued PR waits; waiting ${TRAIN_DEFER_SLEEP:-600} s"; sleep "${TRAIN_DEFER_SLEEP:-600}"; waiting=0; }
}
# A PR stacked on an older open PR targets main but holds that PR's commits. Landed first, it would
# carry the older PR's work into main under its own title; refreshed while the older PR is held, it
# merges main over the older PR's commits and can no longer merge against them once that PR lands.
# It waits while an older open PR (a lower number, whatever branch it targets) shares a commit with it
# that is neither on main nor in a PR that has already landed on main by squash (one merged into
# another branch has not): two PRs built on the same landed PR share only that PR's commits and do
# not wait for each other. Every PR head is fetched to refs/train/pr/<n> in the main clone. Prints
# the PR to wait for, or nothing; any failed lookup returns 2, and the caller then defers the PR
# rather than letting it through unchecked.
g() { git -C "$TRAIN_CLONE" "$@"; }
older_open() {
  local head open merged="" n h c pr
  g fetch -q origin main '+refs/pull/*/head:refs/train/pr/*' 2> /dev/null || return 2
  head=$(g rev-parse -q --verify "refs/train/pr/$1^{commit}") || return 2
  open=$(gh api --paginate "repos/$TRAIN_REPO/pulls?state=open&per_page=100" --jq ".[]|select(.number < $1)|.number" 2> /dev/null) || return 2
  for n in $open; do
    h=$(g rev-parse -q --verify "refs/train/pr/$n^{commit}") || return 2
    for c in $(g merge-base --all "$h" "$head"); do
      g merge-base --is-ancestor "$c" origin/main && continue
      if [ -z "$merged" ]; then
        merged=$(gh api --paginate "repos/$TRAIN_REPO/pulls?state=closed&per_page=100" --jq '.[]|select(.merged_at and .base.ref == "main")|.number' 2> /dev/null) || return 2
      fi
      for pr in $(g for-each-ref --contains "$c" --format='%(refname:lstrip=3)' refs/train/pr/); do
        grep -qx "$pr" <<< "$merged" && continue 2
      done
      echo "$n"
      return 0
    done
  done
}
while :; do
  P=$(grep -m1 -E '^[0-9]+$' "$Q")
  [ -z "$P" ] && break
  grep -v -x "$P" "$Q" > "$Q.tmp"; cat "$Q.tmp" > "$Q"; rm -f "$Q.tmp"
  for attempt in 1 2; do
    read -r st base < <(gh pr view "$P" -R "$TRAIN_REPO" --json state,baseRefName --jq '"\(.state) \(.baseRefName)"' 2>/dev/null)
    [ "$st" != OPEN ] && { log "TRAIN #$P skipped: state=$st"; break; }
    [ "$base" != main ] && { wait_at_end "waits: it targets $base, not main; back to the end of the queue"; break; }
    under=$(older_open "$P"); orc=$?
    [ $orc != 0 ] && { wait_at_end "waits: could not check which open PRs it builds on; back to the end of the queue"; break; }
    [ -n "$under" ] && { wait_at_end "waits: it holds commits of open #$under, which lands first; back to the end of the queue"; break; }
    log "TRAIN #$P refresh (attempt $attempt)"
    out=$(bash "$TRAIN_HERE/refresh.sh" "$P" 2>&1); rc=$?
    echo "$out" > "$TRAIN_STATE/refresh-$P.log"
    waiting=0
    [ $rc != 0 ] && { log "TRAIN #$P refresh stopped rc=$rc: $(echo "$out" | grep -E 'REFUSED|FAILED' | head -1)"; break; }
    log "TRAIN #$P $(echo "$out" | grep -E 'pushed|already current' | head -1 | sed 's/^\[refresh #[0-9]*\] //')"
    out=$(bash "$TRAIN_HERE/land.sh" "$P" 2>&1); rc=$?
    log "TRAIN #$P $(echo "$out" | sed -n '$p')"
    [ $rc != 8 ] && [ $rc != 7 ] && break
  done
done
log "TRAIN queue empty"
exit 0
}
