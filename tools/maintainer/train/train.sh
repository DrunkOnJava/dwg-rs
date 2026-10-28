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
# It waits until the older PR (a lower number) holding commits main does not have lands or closes.
older_open() {
  local head n h c
  git -C "$TRAIN_CLONE" fetch -q origin main 2> /dev/null && git -C "$TRAIN_CLONE" fetch -q origin "pull/$1/head" 2> /dev/null || return
  head=$(git -C "$TRAIN_CLONE" rev-parse FETCH_HEAD)
  gh pr list -R "$TRAIN_REPO" --state open --base main --limit 200 --json number,headRefOid --jq ".[]|select(.number < $1)|\"\(.number) \(.headRefOid)\"" 2> /dev/null |
  while read -r n h; do
    git -C "$TRAIN_CLONE" cat-file -e "$h^{commit}" 2> /dev/null || git -C "$TRAIN_CLONE" fetch -q origin "pull/$n/head" 2> /dev/null || continue
    for c in $(git -C "$TRAIN_CLONE" merge-base --all "$h" "$head" 2> /dev/null); do
      git -C "$TRAIN_CLONE" merge-base --is-ancestor "$c" origin/main || { echo "$n"; exit 0; }
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
    under=$(older_open "$P")
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
