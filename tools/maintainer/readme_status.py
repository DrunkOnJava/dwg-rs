#!/usr/bin/env python3
"""Render the measured coverage in README.md and STATUS.md from the committed baselines.

The `Corpus (public DWG)` CI job fails unless tools/corpus/baseline.tsv and
tools/oracle/baseline.tsv equal what it measures, so text rendered from them on `main` is
the CI measurement on `main`. The rendered text sits between HTML comment markers:

- `<!-- corpus-table:begin -->` / `<!-- corpus-table:end -->` in README.md: the
  per-release table;
- `<!-- corpus-sources:begin -->` / `<!-- corpus-sources:end -->` in README.md: the
  per-source table. Sources differ in size by orders of magnitude (one R2004 drawing can
  hold more records than forty small ones), so the record-weighted aggregate moves when a
  source is pinned; each source's own row and the median drawing do not;
- `<!-- corpus-summary:begin -->` / `<!-- corpus-summary:end -->` in README.md and
  STATUS.md: the one-line totals;
- `<!-- oracle-summary:begin -->` / `<!-- oracle-summary:end -->` in README.md: the DXF
  value oracle's totals;
- `<!-- corpus-run:begin -->` / `<!-- corpus-run:end -->`: the CI run that measured it.

`--check` compares the measured text only, so a newer run on `main` with the same numbers
does not fail it. tools/maintainer/audit.py runs the same check.

Usage:

    python3 tools/maintainer/readme_status.py --check
    python3 tools/maintainer/readme_status.py --write [--run RUN_ID --date YYYY-MM-DD]

`--write` without `--run` names the newest successful ci.yml run on `main` (needs `gh`).
"""

import argparse
import csv
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BASELINE = ROOT / "tools" / "corpus" / "baseline.tsv"
PINS = ROOT / "tools" / "corpus" / "pins.tsv"
ORACLE = ROOT / "tools" / "oracle" / "baseline.tsv"
DOCS = {
    ROOT / "README.md": ("corpus-run", "corpus-table", "corpus-sources", "corpus-summary", "oracle-summary"),
    ROOT / "STATUS.md": ("corpus-run", "corpus-summary"),
}
REPO = "DrunkOnJava/dwg-rs"
RELEASES = [
    ("AC1012", "R13"),
    ("AC1014", "R14"),
    ("AC1015", "R2000"),
    ("AC1018", "R2004"),
    ("AC1021", "R2007"),
    ("AC1024", "R2010"),
    ("AC1027", "R2013"),
    ("AC1032", "R2018"),
]


def rows():
    with BASELINE.open() as f:
        return list(csv.DictReader((line for line in f if not line.startswith("#")), delimiter="\t"))


def pct(decoded, total):
    if not total:
        return "-"
    share = f"{100 * decoded / total:.1f}"
    return "> 99.9 %" if share == "100.0" and decoded < total else f"{share} %"


def totals(rs):
    d = sum(int(r["decoded"]) for r in rs)
    s = sum(int(r["skipped"]) for r in rs)
    e = sum(int(r["errored"]) for r in rs)
    return len(rs), d, s, e


def table(rs):
    out = [
        "| Version | Files tested | Decoded | Skipped | Errored | Success rate | Median drawing |",
        "|---------|--------------|---------|---------|---------|--------------|----------------|",
    ]
    for code, name in RELEASES:
        mine = [r for r in rs if r["release"] == code]
        n, d, s, e = totals(mine)
        if n:
            out.append(f"| {name} ({code}) | {n} | {d} | {s} | {e} | **{pct(d, d + s + e)}** | {median(mine)} |")
    n, d, s, e = totals(rs)
    out.append(f"| **Aggregate** | **{n}** | **{d}** | **{s}** | **{e}** | **{pct(d, d + s + e)}** | **{median(rs)}** |")
    return "\n".join(out)


def median(rs):
    """The median of each drawing's own decoded share, so every drawing weighs the same."""
    shares = [
        (int(r["decoded"]), t)
        for r in rs
        if (t := int(r["decoded"]) + int(r["skipped"]) + int(r["errored"]))
    ]
    if not shares:
        return "-"
    shares.sort(key=lambda x: x[0] / x[1])
    mid = len(shares) // 2
    if len(shares) % 2:
        return pct(*shares[mid])
    (d1, t1), (d2, t2) = shares[mid - 1], shares[mid]
    return pct(d1 * t2 + d2 * t1, 2 * t1 * t2)


def sources(rs):
    """Each source in rs, in the order tools/corpus/pins.tsv first names it."""
    with PINS.open() as f:
        pinned = [r["source"] for r in csv.DictReader((line for line in f if not line.startswith("#")), delimiter="\t")]
    have = {r["source"] for r in rs}
    return [s for s in dict.fromkeys(pinned) if s in have] + sorted(have - set(pinned))


def source_table(rs):
    out = [
        "| Source | Files tested | Decoded | Skipped | Errored | Success rate | Median drawing |",
        "|--------|--------------|---------|---------|---------|--------------|----------------|",
    ]
    for src in sources(rs):
        mine = [r for r in rs if r["source"] == src]
        n, d, s, e = totals(mine)
        out.append(f"| {src} | {n} | {d} | {s} | {e} | **{pct(d, d + s + e)}** | {median(mine)} |")
    n, d, s, e = totals(rs)
    out.append(f"| **All sources** | **{n}** | **{d}** | **{s}** | **{e}** | **{pct(d, d + s + e)}** | **{median(rs)}** |")
    return "\n".join(out)


def summary(rs):
    n, d, s, e = totals(rs)
    per = []
    for code, name in RELEASES:
        rn, rd, rsk, re_ = totals([r for r in rs if r["release"] == code])
        if rn:
            per.append(f"{name} {pct(rd, rd + rsk + re_)}")
    src = []
    for name in sources(rs):
        sn, sd, ss, se = totals([r for r in rs if r["source"] == name])
        src.append(f"{name} {pct(sd, sd + ss + se)} ({sn})")
    return (
        f"{d:,} decoded / {s:,} skipped / **{e:,} errored** / {pct(d, d + s + e)} of all records on {n} drawings; "
        f"the median drawing decodes {median(rs)}. Per source (drawings): {', '.join(src)}. "
        f"Per version: {', '.join(per)}."
    )


def oracle():
    with ORACLE.open() as f:
        rs = list(csv.DictReader(f, delimiter="\t"))
    t = {k: sum(int(r[k]) for r in rs) for k in ("equal", "default", "different", "dwg_only")}
    types = len({r["type"] for r in rs})
    return (
        f"{sum(t.values()):,} field comparisons on {types} types: {t['equal']:,} equal, "
        f"{t['default']:,} equal to the DXF default, {t['different']:,} different and "
        f"{t['dwg_only']:,} present only in the DWG"
    )


def blocks(rs):
    return {
        "corpus-table": table(rs),
        "corpus-sources": source_table(rs),
        "corpus-summary": summary(rs),
        "oracle-summary": oracle(),
    }


def pattern(name):
    return re.compile(rf"(<!-- {name}:begin -->)(.*?)(<!-- {name}:end -->)", re.S)


def render(name, body):
    return f"\n{body}\n" if name in ("corpus-table", "corpus-sources") else body


def latest_run():
    p = subprocess.run(
        ["gh", "run", "list", "-R", REPO, "-b", "main", "-w", "ci.yml", "-s", "success", "-L", "1", "--json", "databaseId,createdAt"],
        capture_output=True,
        text=True,
    )
    if p.returncode != 0 or not p.stdout.strip():
        sys.exit(f"readme_status: no run given and gh could not list one: {p.stderr.strip()}")
    run = json.loads(p.stdout)[0]
    return str(run["databaseId"]), run["createdAt"][:10]


def missing():
    """Each block a document must carry but does not, as a problem string."""
    return [
        f"{doc.name} has no {name} block"
        for doc, names in DOCS.items()
        for name in names
        if len(pattern(name).findall(doc.read_text())) != 1
    ]


def stale():
    """Each marker block that is missing, doubled, or disagrees with its baseline, as a problem string."""
    want = blocks(rows())
    problems = missing()
    for doc, names in DOCS.items():
        text = doc.read_text()
        for name in names:
            if name in want:
                for _, have, _ in pattern(name).findall(text):
                    if have.strip() != want[name].strip():
                        problems.append(f"{doc.name} {name} disagrees with its baseline")
    return problems


def write(run, date):
    problems = missing()
    if problems:
        sys.exit("readme_status: " + "; ".join(problems))
    want = blocks(rows())
    want["corpus-run"] = f"[run {run}](https://github.com/{REPO}/actions/runs/{run}) on {date}"
    for doc, names in DOCS.items():
        text = doc.read_text()
        for name in names:
            text = pattern(name).sub(lambda m, n=name, b=want[name]: m.group(1) + render(n, b) + m.group(3), text)
        doc.write_text(text)


def main():
    ap = argparse.ArgumentParser()
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    ap.add_argument("--run", help="the CI run id that measured the baseline")
    ap.add_argument("--date", help="that run's date, YYYY-MM-DD")
    args = ap.parse_args()
    if bool(args.run) != bool(args.date):
        ap.error("--run and --date go together")
    if args.check:
        problems = stale()
        for p in problems:
            print(p)
        return 1 if problems else 0
    run, date = (args.run, args.date) if args.run else latest_run()
    write(run, date)
    return 0


if __name__ == "__main__":
    sys.exit(main())
