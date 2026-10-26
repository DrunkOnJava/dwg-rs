#!/bin/bash
# The merge train: lands queued PRs one at a time.
#
#   tools/maintainer/train/train.sh [pr...]
#
# Appends any PR numbers given to $TRAIN_STATE/queue (one per line), then pops the queue top-down:
# refresh.sh brings each PR up to main and pushes, land.sh lands it. A PR that went DIRTY or found
# main moved under it is refreshed once more; anything else that stops a PR is logged and the train
# moves on. Append to the queue file to add work while it runs. Progress: $TRAIN_STATE/train.log.
{
. "$(dirname "$0")/common.sh"
Q=$TRAIN_STATE/queue
L=$TRAIN_STATE/train.log
touch "$Q"
for P in "$@"; do grep -qx "$P" "$Q" || echo "$P" >> "$Q"; done
log() { echo "$(date +%H:%M) $*" >> "$L"; }
while :; do
  P=$(grep -m1 -E '^[0-9]+$' "$Q")
  [ -z "$P" ] && break
  grep -v -x "$P" "$Q" > "$Q.tmp"; cat "$Q.tmp" > "$Q"; rm -f "$Q.tmp"
  for attempt in 1 2; do
    st=$(gh pr view "$P" -R "$TRAIN_REPO" --json state --jq .state 2>/dev/null)
    [ "$st" != OPEN ] && { log "TRAIN #$P skipped: state=$st"; break; }
    log "TRAIN #$P refresh (attempt $attempt)"
    out=$(bash "$TRAIN_HERE/refresh.sh" "$P" 2>&1); rc=$?
    echo "$out" > "$TRAIN_STATE/refresh-$P.log"
    [ $rc != 0 ] && { log "TRAIN #$P refresh stopped rc=$rc: $(echo "$out" | grep -E 'REFUSED|FAILED' | head -1)"; break; }
    log "TRAIN #$P $(echo "$out" | grep -E 'vs main:' | head -1 | sed 's/^\[refresh #[0-9]*\] //'); $(echo "$out" | grep -E 'pushed|already current' | head -1)"
    out=$(bash "$TRAIN_HERE/land.sh" "$P" 2>&1); rc=$?
    log "TRAIN #$P $(echo "$out" | sed -n '$p')"
    [ $rc != 8 ] && [ $rc != 7 ] && break
  done
done
log "TRAIN queue empty"
exit 0
}
