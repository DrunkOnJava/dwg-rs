#!/usr/bin/env python3
"""Keep docs/spec-coverage.md true to docs/data/spec-coverage.tsv (#105).

The TSV has one row per section of the ODA *Open Design Specification for
.dwg files* v5.4.1. This script checks that every section of the spec's
table of contents has a row with a valid state and tracking cell, and
regenerates the summary and the section table of docs/spec-coverage.md
between its `spec-coverage` markers.

Usage:

    python3 tools/ci/spec_coverage.py --check   # CI: exit 1 on any problem
    python3 tools/ci/spec_coverage.py --write   # rewrite the generated part
"""

import argparse
import collections
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TSV = ROOT / "docs" / "data" / "spec-coverage.tsv"
MD = ROOT / "docs" / "spec-coverage.md"
STATES = ("complete", "closes", "decoder", "partial", "missing", "n/a")
BEGIN, END = "<!-- spec-coverage:begin -->", "<!-- spec-coverage:end -->"

# The sections every v5.4.1 reader must account for: chapters 1 to 29
# (chapter 20 through its parts 20.1 to 20.3 and the 104 prescriptions of
# §20.4), read from the specification's table of contents.
REQUIRED = [str(c) for c in range(1, 30) if c != 20] + ["20.1", "20.2", "20.3"] + [f"20.4.{n}" for n in range(1, 105)]


def rows():
    out = []
    for line in TSV.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        cells = line.split("\t")
        if len(cells) != 7:
            raise SystemExit(f"{TSV.name}: expected 7 columns, got {len(cells)}: {line[:80]}")
        out.append(cells)
    return out


def problems(table):
    found = []
    seen = collections.Counter(r[0] for r in table)
    found += [f"§{s} has {n} rows" for s, n in seen.items() if n > 1]
    found += [f"§{s} has no row" for s in REQUIRED if s not in seen]
    for sec, title, page, state, code, tracking, note in table:
        if not re.fullmatch(r"\d+(\.\d+)*", sec):
            found.append(f"{sec!r} is not a section number")
        if state not in STATES:
            found.append(f"§{sec}: state {state!r} is not one of {', '.join(STATES)}")
        if tracking and not re.fullmatch(r"#\d+", tracking):
            found.append(f"§{sec}: tracking {tracking!r} is not an issue reference")
        if state not in ("complete", "n/a") and not tracking:
            found.append(f"§{sec}: state {state} but no tracking issue")
        if page and not page.isdigit():
            found.append(f"§{sec}: page {page!r} is not a number")
    return found


def generated(table):
    counts = collections.Counter(r[3] for r in table if r[3] != "n/a")
    counts_204 = collections.Counter(r[3] for r in table if r[0].startswith("20.4."))
    order = ("complete", "closes", "decoder", "partial", "missing")
    lines = [BEGIN, "",
             "## Summary", "",
             "- All sections: " + ", ".join(f"{counts[k]} `{k}`" for k in order) + ".",
             "- The 104 object prescriptions of §20.4: " + ", ".join(f"{counts_204[k]} `{k}`" for k in order) + ".", "",
             "## Sections", "",
             "| Section | Title | Page | State | Code | Tracking | Note |", "|---|---|---:|---|---|---|---|"]
    lines += [f"| §{s} | {t} | {p} | `{st}` | {c} | {tr} | {n} |" for s, t, p, st, c, tr, n in table]
    return "\n".join(lines + ["", END])


def main():
    ap = argparse.ArgumentParser()
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    args = ap.parse_args()
    table = rows()
    found = problems(table)
    text = MD.read_text()
    if BEGIN not in text or END not in text:
        found.append(f"{MD.name} has no {BEGIN} ... {END} block")
        new = text
    else:
        new = text[: text.index(BEGIN)] + generated(table) + text[text.index(END) + len(END):]
    if args.write:
        MD.write_text(new)
    elif new != text:
        found.append(f"{MD.name} differs from {TSV.name}: run tools/ci/spec_coverage.py --write")
    for p in found:
        print(f"spec-coverage: {p}")
    print(f"spec-coverage: {len(table)} rows, {len(REQUIRED)} required sections, {len(found)} problems")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
