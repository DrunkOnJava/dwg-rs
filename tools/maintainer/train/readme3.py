"""Resolve a README.md or STATUS.md merge conflict that lies only in rendered coverage text.

Usage: readme3.py FILE

readme_status.py renders the corpus table, the per-source table, the totals, the oracle summary and
the CI run between `<!-- name:begin -->` and `<!-- name:end -->` markers. Two PRs that each commit
their own measurement conflict there and nowhere else. Each conflict hunk whose lines on both sides
are table rows or carry a marker takes main's side (the merge's "theirs"); ci_baseline.sh renders the
block again from the PR's own CI measurement. Any other hunk is a real conflict: the file is left
untouched and the exit status is 1.
"""
import re
import sys

GENERATED = re.compile(r"^\s*\|.*\|\s*$|<!-- [\w-]+:(?:begin|end) -->")


def main(path):
    text = open(path, encoding="utf-8").read()
    out, i = [], 0
    lines = text.splitlines(keepends=True)
    while i < len(lines):
        if not lines[i].startswith("<<<<<<< "):
            out.append(lines[i])
            i += 1
            continue
        ours, theirs = [], []
        i += 1
        side = ours
        while i < len(lines) and not lines[i].startswith(">>>>>>> "):
            if lines[i].startswith("||||||| "):
                side = []
            elif lines[i].startswith("======="):
                side = theirs
            else:
                side.append(lines[i])
            i += 1
        i += 1
        if not all(GENERATED.search(l) or not l.strip() for l in ours + theirs):
            print(f"{path}: a conflict outside the rendered coverage text")
            return 1
        out.extend(theirs)
    open(path, "w", encoding="utf-8").write("".join(out))
    print(f"{path}: rendered coverage conflicts resolved to main's; ci_baseline.sh renders them again")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
