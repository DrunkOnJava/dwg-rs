# Shared settings for the merge train. Sourced, not run.
#
# Every path can be overridden from the environment:
#   TRAIN_REPO       GitHub repository             (default: the `origin` remote's owner/name)
#   TRAIN_STATE      queue, log, per-PR evidence   (default: <git common dir>/train, never committed)
#   TRAIN_WORKTREE   the one worktree the train merges in (default: a sibling of the main clone)
#   TRAIN_CORPUS     fetched public corpus         (default: $XDG_CACHE_HOME/dwg-corpus)
#   TRAIN_TARGET     cargo target dir              (default: $XDG_CACHE_HOME/dwg-rs-train-target)
#   TRAIN_DEFER_SLEEP  seconds to wait when every queued PR waits on its base branch (600)
#   TRAIN_LEASE_WAIT   seconds before retrying a PR whose build got no lease (120)
TRAIN_HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TRAIN_GIT_DIR=$(git -C "$TRAIN_HERE" rev-parse --path-format=absolute --git-common-dir)
TRAIN_CLONE=$(dirname "$TRAIN_GIT_DIR")
TRAIN_REPO=${TRAIN_REPO:-$(git -C "$TRAIN_CLONE" remote get-url origin | sed -E 's#^.*github\.com[:/]##; s#\.git$##')}
TRAIN_STATE=${TRAIN_STATE:-$TRAIN_GIT_DIR/train}
TRAIN_WORKTREE=${TRAIN_WORKTREE:-$TRAIN_CLONE-train}
TRAIN_CORPUS=${TRAIN_CORPUS:-${XDG_CACHE_HOME:-$HOME/.cache}/dwg-corpus}
export CARGO_TARGET_DIR=${TRAIN_TARGET:-${XDG_CACHE_HOME:-$HOME/.cache}/dwg-rs-train-target}
export CARGO_TERM_COLOR=never
mkdir -p "$TRAIN_STATE"
