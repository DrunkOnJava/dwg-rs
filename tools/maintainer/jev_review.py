#!/usr/bin/env python3
"""Rank the maintained prose and open issues that merged pull requests may have overtaken.

Two read-only reviews, each a shortlist for a maintainer to read, never an edit:

- `prose`: sentences, list items and table rows of README.md, STATUS.md and ROADMAP.md
  that a pull request merged since the document was last edited may have made untrue
  (housekeeping item 10 in docs/maintaining.md). Text between `<!-- name:begin -->` and
  `<!-- name:end -->` markers is rendered from the baselines and checked by
  readme_status.py, so it is skipped.
- `issues`: open issues that a merged pull request may already have resolved without
  closing them.

Code picks the candidates: status wording, `#N` references and shared rare words pair
each sentence or issue with the few merged pull requests it could be about. TypeSafe's Jev
model (https://docs.typesafe.ai) answers yes/no questions about each pairing with a
probability. For prose, one question asks whether the sentence names a specific part of
the project as missing, pending or limited, and one per pull request asks whether that
pull request delivers it; a pairing scores the product of the two. For an issue, one
question per pull request asks whether it resolves the issue. Pairs at or above `--min`
are printed, highest first: from 0.7 the pairing is likely, below that it is worth a
look. A probability is not evidence: read the pull request before changing a document or
closing an issue. Measured numbers are not asked about, since the model does not check
them; `prose` lists the sentences outside the rendered blocks that state one.

Needs the GitHub CLI (`gh`) and an API key in `TYPESAFE_API_KEY`. Input tokens are billed
(about $0.04 per million) and output tokens are free; every run prints its token count.
Not part of CI or audit.py: a probability is a pointer for review, not a gate.

Usage:

    python3 tools/maintainer/jev_review.py prose [--since YYYY-MM-DD] [--min 0.3] [--json out.json]
    python3 tools/maintainer/jev_review.py issues [--min 0.5] [--json out.json]
"""

import argparse
import json
import math
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REPO = "DrunkOnJava/dwg-rs"
DOCS = ["README.md", "STATUS.md", "ROADMAP.md"]
ENDPOINT = "https://api.typesafe.ai/v1/systemone"
MODEL = "jev-1.14.0"
USD_PER_INPUT_TOKEN = 0.042 / 1_000_000
STATUS = re.compile(
    r"#\d{2,4}\b|\bin review\b|\bnot yet\b|\bno longer\b|\b(?:does|do|is|are|has|have) not\b|\bnot\b"
    r"|\bnone\b|\bnever\b|\bonly\b|\btoday\b|\bpending\b|\bplanned\b|\bmissing\b|\bunscored\b"
    r"|\bskipped\b|\berrored\b|\bstubs?\b|\bdraft\b|\bv\d+\.\d",
    re.I,
)
SECTION_STATUS = re.compile(r"\bnot\b|\bpending\b|\bremaining\b|\bmissing\b|\bplanned\b|\bin review\b|\blimit", re.I)
STOP = set(
    """about after again against also because been before being below between both
    could does doing down during each every from further have having here into itself
    just more most must other over same should some such than that their them then there
    these they this those through under until very were what when where which while with
    would your will only once also each make made makes still now yet the and for are not
    pull request requests merged main dwg-rs drawing drawings file files read reads""".split()
)
WORD = re.compile(r"[A-Za-z][A-Za-z0-9_]{3,}")
REF = re.compile(r"#(\d{2,4})\b")


def gh(*args):
    p = subprocess.run(["gh", *args], capture_output=True, text=True)
    if p.returncode != 0:
        sys.exit(f"jev_review: gh {' '.join(args[:3])} failed: {p.stderr.strip()}")
    return json.loads(p.stdout)


def plain(text):
    """Markdown to the words a reader sees: links keep their text, images, comments and emphasis go."""
    text = re.sub(r"<!--.*?-->", "", text, flags=re.S)
    text = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"\*\*|__", "", text)
    return re.sub(r"\s+", " ", text).strip()


def summary(body, limit=900):
    """A pull request's `## Summary` section, or the start of its body."""
    body = body or ""
    m = re.search(r"^##\s+Summary\s*$(.*?)(?=^##\s|\Z)", body, re.M | re.S)
    text = plain(m.group(1) if m else body)
    return text[:limit] + ("…" if len(text) > limit else "")


ACCEPT = re.compile(r"^#{2,6}\s+(?:Acceptance|Done when|Definition of done)\b.*?(?=^#{1,6}\s|\Z)", re.M | re.S | re.I)


def issue_text(body, limit=1500):
    """An issue's body, whole when it fits; otherwise its acceptance sections first, so the cut never drops them."""
    body = body or ""
    whole = plain(body)
    if len(whole) <= limit:
        return whole
    text = plain("\n".join([m.group(0) for m in ACCEPT.finditer(body)] + [ACCEPT.sub("", body)]))
    return text[:limit] + ("…" if len(text) > limit else "")


def units(path):
    """Each paragraph, list item and table row outside code fences and marker blocks, with its line and section."""
    out, fence, marker, section, header = [], False, None, "", None
    buf, start, kind = [], 0, None

    def flush():
        nonlocal buf, kind
        text = plain(" ".join(buf))
        if text:
            out.append({"line": start, "section": section, "text": text, "table_header": header if kind == "row" else None})
        buf, kind = [], None

    for n, raw in enumerate(path.read_text().splitlines(), 1):
        line = re.sub(r"<!-- ([\w-]+):begin -->.*?<!-- \1:end -->", "", raw)
        if marker:
            if f"<!-- {marker}:end -->" in line:
                marker = None
            continue
        m = re.search(r"<!-- ([\w-]+):begin -->", line)
        if m:
            flush()
            marker = m.group(1)
            continue
        if line.lstrip().startswith(("```", "~~~")):
            flush()
            fence = not fence
            continue
        if fence:
            continue
        s = line.strip()
        if not s:
            flush()
            header = None
            continue
        if s.startswith("#"):
            flush()
            section = s.lstrip("#").strip()
            continue
        if s.startswith("|"):
            flush()
            if re.fullmatch(r"\|[\s:|-]+\|?", s):
                continue
            if header is None:
                header = plain(s)
                continue
            buf, start, kind = [s], n, "row"
            flush()
            continue
        header = None
        if re.match(r"([-*+]|\d+\.)\s", s):
            flush()
            buf, start, kind = [re.sub(r"^([-*+]|\d+\.)\s+", "", s)], n, "item"
            continue
        if not buf:
            start, kind = n, "para"
        buf.append(s)
    flush()
    return out


SENTENCE_END = re.compile(r"(?<=[a-z0-9)`'\"\]])[.!?](?=\s+[A-Z`*(\"])")


def statements(path):
    """Each sentence of a paragraph or list item, with the whole unit as its context; a table row stays whole."""
    for u in units(path):
        if u["table_header"]:
            yield {**u, "context": None}
            continue
        parts = [p.strip() for p in SENTENCE_END.split(u["text"]) if p.strip()]
        for p in parts:
            yield {**u, "text": p if p[-1] in ".!?:;" else p + ".", "context": u["text"] if len(parts) > 1 else None}


def tokens(text):
    words = (t.lower() for t in WORD.findall(text))
    return {w[:-1] if len(w) > 4 and w.endswith("s") and not w.endswith("ss") else w for w in words if w not in STOP}


def shortlist(text, prs, k, idf):
    """The pull requests a statement could be about: any it names by number, then the k sharing the most rare words."""
    refs = {int(r) for r in REF.findall(text)}
    mine = tokens(text)
    named, scored = [], []
    for pr in prs:
        if pr["number"] in refs or refs & pr["refs"]:
            named.append(pr)
            continue
        score = sum(idf[t] for t in mine & pr["tokens"])
        if score > 0:
            scored.append((score, pr))
    scored.sort(key=lambda x: -x[0])
    return named + [pr for _, pr in scored[: max(0, k - len(named))]]


def prepare(prs):
    for pr in prs:
        pr["summary"] = summary(pr.get("body"))
        pr["refs"] = {int(r) for r in REF.findall((pr.get("title") or "") + " " + (pr.get("body") or ""))}
        pr["tokens"] = tokens(pr["title"] + " " + pr["summary"])
    df = {}
    for pr in prs:
        for t in pr["tokens"]:
            df[t] = df.get(t, 0) + 1
    return {t: math.log((len(prs) + 1) / c) for t, c in df.items()}


def ask(key, state, questions, model):
    body = json.dumps({"model": model, "state": state, "questions": questions}).encode()
    for attempt in range(6):
        req = urllib.request.Request(
            ENDPOINT, data=body, headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"}
        )
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code not in (429, 500, 502, 503, 504, 529) or attempt == 5:
                sys.exit(f"jev_review: TypeSafe answered {e.code}: {e.read().decode(errors='replace')[:300]}")
        except (urllib.error.URLError, TimeoutError) as e:
            if attempt == 5:
                sys.exit(f"jev_review: TypeSafe unreachable: {e}")
        time.sleep(min(30, 2**attempt))


def pr_record(pr):
    return {"number": pr["number"], "title": pr["title"], "summary": pr["summary"]}


GAP = {
    "type": "noul",
    "instructions": "Read with the heading in `section`, does `statement` name a specific feature, type, section or tool of this project that is missing, "
    "pending, in review, not done yet, unsupported or limited today?",
    "criteria": {
        "true": {
            "what": "It reports the current state of one named part of the project: something not done yet, a limit, "
            "or an open or in-review item.",
            "examples": [
                "SPLINE fit points are not written yet.",
                "The R2007 writer is in review (#900).",
                "Encrypted sections are refused, not decoded.",
            ],
        },
        "false": {
            "what": "It defines a term, states a rule that applies to any type, describes how something works, or "
            "records a goal, a settled finding or history.",
            "examples": [
                "A decoded record is one whose field list ends on its boundary.",
                "A type needs a scorer before its values count as proven.",
                "Every number on this page comes from a CI run.",
            ],
        },
    },
}
DELIVERS = {
    "true": {
        "what": "The pull request delivers the very item the statement says is missing or pending, or lifts the limit "
        "it names.",
        "examples": [
            "statement 'SPLINE fit points are not written yet.' and pull request 'write SPLINE fit points in the DXF writer'",
        ],
    },
    "false": {
        "what": "The pull request does other work, including other work on the same type, section or topic.",
        "examples": [
            "statement 'SPLINE fit points are not written yet.' and pull request 'score SPLINE in the value oracle'",
            "statement 'SPLINE fit points are not written yet.' and pull request 'measure how fast SPLINE records decode'",
        ],
    },
}
MEASURED = re.compile(r"\b\d[\d,]*\s(?:records?|drawings?|files?|types?|errored|skipped|decoded|sections?)\b|\b\d+(?:\.\d+)?\s?%")
RESOLVES = {
    "true": "The pull request delivers what the issue asks for, so the issue could be closed citing it.",
    "false": "The pull request does something else, only part of what the issue asks for, or only prepares for it.",
}


def run(jobs, key, model, workers):
    """Ask every job's questions; each job is (state, {question id: (pr, question)}, {question id: question})."""

    def one(job):
        state, qs, extra = job
        resp = ask(key, state, {**{qid: q for qid, (_, q) in qs.items()}, **extra}, model)
        return job, resp

    with ThreadPoolExecutor(max_workers=workers) as pool:
        return list(pool.map(one, jobs))


def prose(args, key):
    jobs, meta, numbers = [], [], []
    for doc in DOCS:
        path = ROOT / doc
        since = args.since
        if not since:
            last = subprocess.run(["git", "log", "-1", "--format=%cs", "--", doc], cwd=ROOT, capture_output=True, text=True).stdout.strip()
            since = (datetime.fromisoformat(last) - timedelta(days=2)).date().isoformat()
        prs = gh("pr", "list", "-R", REPO, "--state", "merged", "--search", f"merged:>={since}", "--limit", "300", "--json", "number,title,body,mergedAt")
        idf = prepare(prs)
        cands = [
            u
            for u in statements(path)
            if (STATUS.search(u["text"]) or SECTION_STATUS.search(u["section"])) and not MEASURED.search(u["text"])
        ]
        numbers += [(doc, u) for u in units(path) if MEASURED.search(u["text"])]
        for u in cands:
            short = shortlist(u["text"], prs, args.k, idf)
            if not short:
                continue
            state = {"document": doc, "section": u["section"], "statement": u["text"]}
            if u["table_header"]:
                state["table_columns"] = u["table_header"]
            if u["context"]:
                state["paragraph"] = u["context"]
            qs = {
                f"pr_{pr['number']}": (
                    pr,
                    {
                        "type": "noul",
                        "instructions": {
                            "pull_request": pr_record(pr),
                            "question": "Does `pull_request` deliver something that `statement`, read with the heading in `section`, says is missing, pending, "
                            "in review, not done yet, unsupported or limited?",
                        },
                        "criteria": DELIVERS,
                    },
                )
                for pr in short
            }
            jobs.append((state, qs, {"gap": GAP}))
            meta.append((doc, u))
        print(f"{doc}: {len(cands)} statements with status wording, {len(prs)} pull requests merged since {since}", file=sys.stderr)
    rows, usage = collect(jobs, meta, key, args, lambda doc, u: {"where": f"{doc}:{u['line']}", "text": u["text"]})
    report(rows, usage, args, "statement")
    if numbers:
        print(f"\n{len(numbers)} statements outside the rendered blocks state a measured number; re-measure them, the model does not check numbers:")
        for doc, u in numbers:
            print(f"  {doc}:{u['line']:<5} {u['text'][:120]}")


def issues(args, key):
    if args.number:
        wanted = [int(n) for arg in args.number for n in arg.split(",")]
        open_issues = [gh("issue", "view", str(n), "-R", REPO, "--json", "number,title,body,createdAt") for n in wanted]
    else:
        open_issues = gh("issue", "list", "-R", REPO, "--state", "open", "--limit", "500", "--json", "number,title,body,createdAt")
    prs = gh("pr", "list", "-R", REPO, "--state", "merged", "--limit", "500", "--json", "number,title,body,mergedAt")
    idf = prepare(prs)
    jobs, meta = [], []
    for issue in open_issues:
        body = issue_text(issue.get("body"))
        text = issue["title"] + " " + body
        later = [pr for pr in prs if pr["mergedAt"] >= issue["createdAt"]]
        refs = {issue["number"]}
        named = [pr for pr in later if refs & pr["refs"]]
        rest = shortlist(text, [pr for pr in later if pr not in named], max(0, args.k - len(named)), idf)
        short = named + rest
        if not short:
            continue
        state = {"issue": {"number": issue["number"], "title": issue["title"], "body": body}}
        qs = {
            f"pr_{pr['number']}": (
                pr,
                {
                    "type": "noul",
                    "instructions": {"pull_request": pr_record(pr), "question": "Does the merged `pull_request` resolve `issue`?"},
                    "criteria": RESOLVES,
                },
            )
            for pr in short
        }
        jobs.append((state, qs, {}))
        meta.append(("issue", issue))
    print(f"{len(open_issues)} open issues, {len(prs)} merged pull requests, {len(jobs)} issues with candidates", file=sys.stderr)
    rows, usage = collect(jobs, meta, key, args, lambda _, i: {"where": f"#{i['number']}", "text": i["title"]})
    report(rows, usage, args, "issue")


def collect(jobs, meta, key, args, describe):
    questions = sum(len(qs) + len(extra) for _, qs, extra in jobs)
    if questions > args.max_questions:
        sys.exit(f"jev_review: {questions} questions over {len(jobs)} requests is above --max-questions {args.max_questions}")
    rows, usage, model = [], {"requests": len(jobs), "questions": questions, "input_tokens": 0}, None
    for (job, resp), (doc, item) in zip(run(jobs, key, args.model, args.workers), meta):
        _, qs, _ = job
        model = resp["model"]
        usage["input_tokens"] += resp["usage"]["input_tokens"]
        gap = resp["answers"]["gap"]["noul"] if "gap" in resp["answers"] else 1.0
        for qid, (pr, _) in qs.items():
            p = resp["answers"][qid]["noul"]
            rows.append({**describe(doc, item), "pr": pr["number"], "pr_title": pr["title"], "gap": gap, "pair": p, "noul": gap * p})
    usage["model"] = model
    usage["usd"] = round(usage["input_tokens"] * USD_PER_INPUT_TOKEN, 4)
    rows.sort(key=lambda r: -r["noul"])
    return rows, usage


def report(rows, usage, args, noun):
    shown = [r for r in rows if r["noul"] >= args.min]
    for r in shown:
        flag = "likely" if r["noul"] >= 0.7 else "look"
        text = r["text"] if len(r["text"]) <= 160 else r["text"][:157] + "..."
        parts = f" (gap {r['gap']:.2f} x pr {r['pair']:.2f})" if r["gap"] < 1.0 else ""
        print(f"{r['noul']:.2f} {flag:<6} {r['where']:<16} #{r['pr']:<4} {r['pr_title'][:70]}{parts}\n       {text}")
    print(
        f"{len(shown)} of {len(rows)} {noun}-pull request pairs at or above {args.min}; {usage['requests']} requests, "
        f"{usage['input_tokens']:,} input tokens (${usage['usd']}) on {usage['model']}"
    )
    if args.json:
        Path(args.json).write_text(json.dumps({"usage": usage, "pairs": rows}, indent=1))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name, k, floor in (("prose", 6, 0.3), ("issues", 6, 0.5)):
        p = sub.add_parser(name)
        p.add_argument("--min", type=float, default=floor, help="print pairs at or above this probability")
        p.add_argument("--k", type=int, default=k, help="candidate pull requests per statement or issue")
        p.add_argument("--json", help="also write every pair, with usage, to this file")
        p.add_argument("--model", default=MODEL)
        p.add_argument("--workers", type=int, default=8)
        p.add_argument("--max-questions", type=int, default=3000)
        if name == "prose":
            p.add_argument("--since", help="pull requests merged on or after this date (default: two days before each document's last commit)")
        else:
            p.add_argument("--number", action="append", help="review these issues, in any state, instead of every open one (N[,N...])")
    args = ap.parse_args()
    key = os.environ.get("TYPESAFE_API_KEY")
    if not key:
        sys.exit("jev_review: set TYPESAFE_API_KEY (https://console.typesafe.ai)")
    {"prose": prose, "issues": issues}[args.cmd](args, key)


if __name__ == "__main__":
    main()
