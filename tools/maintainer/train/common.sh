# Shared settings for the merge train. Sourced, not run. The train builds and tests nothing
# locally: every build, test and corpus measurement is a GitHub Actions run on the PR.
#
# Every path can be overridden from the environment:
#   TRAIN_REPO       GitHub repository             (default: the `origin` remote's owner/name)
#   TRAIN_STATE      queue, log, per-PR evidence   (default: <git common dir>/train, never committed)
#   TRAIN_WORKTREE   the one worktree the train merges in (default: a sibling of the main clone)
#   TRAIN_DEFER_SLEEP  seconds to wait when every queued PR waits on its base branch (600)
TRAIN_HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TRAIN_GIT_DIR=$(git -C "$TRAIN_HERE" rev-parse --path-format=absolute --git-common-dir)
TRAIN_CLONE=$(dirname "$TRAIN_GIT_DIR")
TRAIN_REPO=${TRAIN_REPO:-$(git -C "$TRAIN_CLONE" remote get-url origin | sed -E 's#^.*github\.com[:/]##; s#\.git$##')}
TRAIN_STATE=${TRAIN_STATE:-$TRAIN_GIT_DIR/train}
TRAIN_WORKTREE=${TRAIN_WORKTREE:-$TRAIN_CLONE-train}
mkdir -p "$TRAIN_STATE"
