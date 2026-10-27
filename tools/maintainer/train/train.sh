#!/bin/bash
# The merge train: lands queued PRs one at a time.
#
#   tools/maintainer/train/train.sh [pr...]
#
# Appends any PR numbers given to $TRAIN_STATE/queue (one per line), then pops the queue top-down:
# refresh.sh merges main into each PR and pushes, land.sh waits for its GitHub Actions run, commits
# the corpus counts that run measured when they moved, and lands it. Nothing is built or tested on
# this machine. A PR that went DIRTY or found main moved under it is refreshed once more. A PR
# stacked on another PR's branch waits at the end of the queue until GitHub retargets it to main.
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
while :; do
  P=$(grep -m1 -E '^[0-9]+$' "$Q")
  [ -z "$P" ] && break
  grep -v -x "$P" "$Q" > "$Q.tmp"; cat "$Q.tmp" > "$Q"; rm -f "$Q.tmp"
  for attempt in 1 2; do
    read -r st base < <(gh pr view "$P" -R "$TRAIN_REPO" --json state,baseRefName --jq '"\(.state) \(.baseRefName)"' 2>/dev/null)
    [ "$st" != OPEN ] && { log "TRAIN #$P skipped: state=$st"; break; }
    [ "$base" != main ] && { wait_at_end "waits: it targets $base, not main; back to the end of the queue"; break; }
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
