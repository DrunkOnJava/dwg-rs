#!/usr/bin/env python3
"""Audit the GitHub side of the repository against docs/maintaining.md.

Read-only. Prints one line per check, `ok` or `FAIL`, and exits 1 if any check
fails. A failing line is a chore from the housekeeping checklist, with what to
fix. A check whose API call is refused prints `skipped` with the reason (the
rulesets check needs an admin token).

Needs the GitHub CLI (`gh`), logged in with the `repo` scope.

Usage:

    python3 tools/maintainer/audit.py [--repo OWNER/REPO] [--reply-days 3]
"""

import argparse
import fnmatch
import json
import re
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CACHE_LIMIT_BYTES = 10 * 1024**3
# A research or Measure branch with no pull request is kept while its work is
# live; one whose last commit is older than this is stale (docs/maintaining.md).
BRANCH_STALE_DAYS = 14
# Directories with a lockfile that Dependabot deliberately does not update, and why
# (docs/maintaining.md says the same).
# The 1.0 release decision; every open issue under the 1.0.0 milestone serves one of
# its requirements (docs/maintaining.md, housekeeping item 8).
RELEASE_CRITERIA = ROOT / "docs" / "release-1.0-criteria.md"
UNWATCHED = {"/tools/synthetic-dwg": "superseded by examples/build_fixtures.rs; its removal is #20"}
results = []


# The languages .github/workflows/codeql.yml analyses.
CODEQL_LANGUAGES = ("actions", "rust")


def gh(*args):
    p = subprocess.run(["gh", "api", *args], capture_output=True, text=True)
    if p.returncode != 0:
        raise RuntimeError((p.stderr or p.stdout).strip().splitlines()[0][:160] if (p.stderr or p.stdout).strip() else "gh failed")
    return json.loads(p.stdout) if p.stdout.strip() else None


def pages(path):
    """A paginated list endpoint as one list (the caches endpoint wraps its list in an object)."""
    p = subprocess.run(["gh", "api", "--paginate", path], capture_output=True, text=True)
    if p.returncode != 0:
        raise RuntimeError(p.stderr.strip().splitlines()[0][:160] if p.stderr.strip() else "gh failed")
    out, decoder, i, text = [], json.JSONDecoder(), 0, p.stdout
    while i < len(text):
        while i < len(text) and text[i] in " \r\n\t":
            i += 1
        if i >= len(text):
            break
        obj, i = decoder.raw_decode(text, i)
        out.extend(obj if isinstance(obj, list) else obj["actions_caches"] if "actions_caches" in obj else [obj])
    return out


def check(name):
    """Register a check: it returns a list of problems, empty for ok."""

    def wrap(fn):
        def run(*a, **k):
            try:
                problems = fn(*a, **k)
            except RuntimeError as error:
                results.append(("skipped", name, str(error)))
                return
            results.append(("FAIL" if problems else "ok", name, "; ".join(problems[:6]) + (f" (+{len(problems) - 6} more)" if len(problems) > 6 else "")))

        return run

    return wrap


def age_days(stamp):
    return (datetime.now(timezone.utc) - datetime.fromisoformat(stamp.replace("Z", "+00:00"))).days


@check("every open issue has one type, an area and a priority, and no needs-triage")
def issue_labels(repo):
    problems = []
    for issue in pages(f"repos/{repo}/issues?state=open&per_page=100"):
        if "pull_request" in issue:
            continue
        names = [l["name"] for l in issue["labels"]]
        types = [n for n in names if n.startswith("type:")]
        if len(types) != 1 or not any(n.startswith("area:") for n in names) or not any(n.startswith("priority:") for n in names):
            problems.append(f"#{issue['number']} has {names}")
        elif "needs-triage" in names:
            problems.append(f"#{issue['number']} is triaged but still needs-triage")
    return problems


@check("milestones match the releases: closed ones are empty, open ones are not")
def milestones(repo):
    problems = []
    for m in pages(f"repos/{repo}/milestones?state=all&per_page=100"):
        if m["state"] == "closed" and m["open_issues"]:
            problems.append(f"closed milestone {m['title']!r} has {m['open_issues']} open issues")
        if m["state"] == "open" and m["open_issues"] == 0 and m["closed_issues"]:
            problems.append(f"open milestone {m['title']!r} has no open issues: close it if its release shipped")
    return problems


@check("every open issue under the 1.0.0 milestone serves a requirement of docs/release-1.0-criteria.md")
def release_criteria(repo):
    cited = {int(n) for n in re.findall(r"(?:#|/issues/)(\d+)", RELEASE_CRITERIA.read_text())}
    problems = []
    for m in pages(f"repos/{repo}/milestones?state=open&per_page=100"):
        if not m["title"].startswith("1.0.0"):
            continue
        for issue in pages(f"repos/{repo}/issues?milestone={m['number']}&state=open&per_page=100"):
            if "pull_request" in issue:
                problems.append(f"pull request #{issue['number']} is in {m['title']!r}: milestones hold issues")
            elif issue["number"] not in cited:
                problems.append(f"#{issue['number']} is in {m['title']!r}, but no requirement cites it")
    return problems


@check(f"no remote branch but main, open pull requests' heads and branches active in the last {BRANCH_STALE_DAYS} days")
def branches(repo):
    open_heads = {p["head"]["ref"] for p in pages(f"repos/{repo}/pulls?state=open&per_page=100")}
    problems = []
    for b in pages(f"repos/{repo}/branches?per_page=100"):
        if b["name"] == "main" or b["name"] in open_heads:
            continue
        commit = gh(f"repos/{repo}/commits/{b['commit']['sha']}")
        stamp = commit["commit"]["committer"]["date"]
        when = datetime.fromisoformat(stamp.replace("Z", "+00:00"))
        if datetime.now(timezone.utc) - when > timedelta(days=BRANCH_STALE_DAYS):
            problems.append(f"{b['name']} ({age_days(stamp)} days old)")
    return problems


@check("the Actions cache is under 90% of its 10 GB limit")
def caches(repo):
    # The usage endpoint lags: it still said 9.1 GB minutes after 4.9 GB had been
    # deleted (2026-09-30), so add up the list itself.
    used = sum(c["size_in_bytes"] for c in pages(f"repos/{repo}/actions/caches?per_page=100"))
    return [f"{used / 1024**3:.1f} GB used; run tools/maintainer/clean_caches.py (--delete to act)"] if used > 0.9 * CACHE_LIMIT_BYTES else []


MAIN_SUITES = """
query($o:String!,$n:String!){
  repository(owner:$o,name:$n){
    ref(qualifiedName:"refs/heads/main"){
      target{
        ... on Commit{
          history(first:100){
            nodes{
              checkSuites(first:50){
                nodes{ conclusion status workflowRun{ event createdAt url workflow{ name } } }
              }
            }
          }
        }
      }
    }
  }
}"""


@check("the latest push or scheduled run of every workflow on main is not a failure")
def main_green(repo):
    # The REST runs list is sometimes served from a stale index, for filtered
    # queries of any shape: on 2026-09-30 one URL returned three different
    # snapshots minutes apart, and later both of a per-workflow pair at once, each
    # time reporting a Fuzz failure from 09-06 that a dozen green nights had
    # superseded. The check suites of main's last 100 commits, through GraphQL,
    # are read from the primary store. A scheduled run is attached to the commit
    # that was main's head when it ran, so a workflow whose last run is older
    # than 100 commits is not judged. Only push and scheduled runs say anything
    # about main (not a manual Measure experiment, CodeQL or Dependabot), and a
    # cancelled or skipped run says nothing either way.
    owner, name = repo.split("/")
    data = gh("graphql", "-f", f"query={MAIN_SUITES}", "-f", f"o={owner}", "-f", f"n={name}")
    if data.get("errors"):
        raise RuntimeError(data["errors"][0]["message"][:160])
    latest = {}
    for commit in data["data"]["repository"]["ref"]["target"]["history"]["nodes"]:
        for suite in commit["checkSuites"]["nodes"]:
            run = suite.get("workflowRun")
            if not run or run["event"] not in ("push", "schedule") or suite["status"] != "COMPLETED":
                continue
            if suite["conclusion"] not in ("SUCCESS", "FAILURE", "TIMED_OUT", "STARTUP_FAILURE"):
                continue
            workflow = run["workflow"]["name"]
            if workflow not in latest or run["createdAt"] > latest[workflow][0]:
                latest[workflow] = (run["createdAt"], suite["conclusion"], run["url"])
    return [f"{workflow}: {conclusion.lower()} ({url})" for workflow, (_, conclusion, url) in sorted(latest.items()) if conclusion != "SUCCESS"]


@check("no open Dependabot, code scanning or secret scanning alerts")
def alerts(repo):
    problems = []
    for kind, path in (("Dependabot", "dependabot"), ("code scanning", "code-scanning"), ("secret scanning", "secret-scanning")):
        try:
            n = len(gh(f"repos/{repo}/{path}/alerts?state=open&per_page=100"))
        except RuntimeError as error:
            if "no analysis" in str(error):
                continue
            raise
        if n:
            problems.append(f"{n} open {kind} alerts")
    return problems


def contains(want, have):
    """`have` holds everything `want` says; the API may add defaults, and lists are compared without order."""
    if isinstance(want, dict):
        return isinstance(have, dict) and all(k in have and contains(v, have[k]) for k, v in want.items())
    if isinstance(want, list):
        return isinstance(have, list) and len(want) == len(have) and all(any(contains(w, h) for h in have) for w in want)
    return want == have


@check("the live rulesets say what .github/rulesets/*.json say")
def rulesets(repo):
    live = {r["name"]: gh(f"repos/{repo}/rulesets/{r['id']}") for r in gh(f"repos/{repo}/rulesets")}
    problems = []
    for path in sorted((ROOT / ".github" / "rulesets").glob("*.json")):
        want = json.loads(path.read_text())
        have = live.get(want["name"])
        if have is None:
            problems.append(f"{want['name']} is not on the repository")
            continue
        for key in ("target", "enforcement", "conditions", "bypass_actors"):
            if not contains(want[key], have[key]):
                problems.append(f"{want['name']}: {key} differs from {path.name}")
        want_rules = {r["type"]: r.get("parameters") or {} for r in want["rules"]}
        have_rules = {r["type"]: r.get("parameters") or {} for r in have["rules"]}
        if set(want_rules) != set(have_rules):
            problems.append(f"{want['name']}: rules {sorted(have_rules)} on the repository, {sorted(want_rules)} in {path.name}")
        else:
            problems += [f"{want['name']}: the {t} rule differs from {path.name}" for t in want_rules if not contains(want_rules[t], have_rules[t])]
    for name in sorted(set(live) - {json.loads(p.read_text())["name"] for p in (ROOT / ".github" / "rulesets").glob("*.json")}):
        problems.append(f"{name} is on the repository but has no file")
    return problems


@check("repository settings: squash only, branches deleted on merge, SHA pins, private reporting, CodeQL, read-only token")
def settings(repo):
    r = gh(f"repos/{repo}")
    problems = []
    if not r["allow_squash_merge"] or r["allow_merge_commit"] or r["allow_rebase_merge"]:
        problems.append("merge methods are not squash only")
    if not r["delete_branch_on_merge"]:
        problems.append("delete_branch_on_merge is off")
    if not gh(f"repos/{repo}/actions/permissions").get("sha_pinning_required"):
        problems.append("actions are not required to be SHA pinned")
    if gh(f"repos/{repo}/actions/permissions/workflow")["default_workflow_permissions"] != "read":
        problems.append("the default workflow token is not read-only")
    if not gh(f"repos/{repo}/private-vulnerability-reporting").get("enabled"):
        problems.append("private vulnerability reporting is off")
    # CodeQL runs from .github/workflows/codeql.yml (C3), the only way to scan
    # Rust; the default setup must stay off, since GitHub refuses an advanced
    # setup's results while it is on.
    if gh(f"repos/{repo}/code-scanning/default-setup").get("state") == "configured":
        problems.append("CodeQL default setup is on, so codeql.yml's results are refused")
    analysed = {a.get("category") for a in gh(f"repos/{repo}/code-scanning/analyses?ref=refs/heads/main&tool_name=CodeQL&per_page=100")}
    missing = sorted(f"/language:{lang}" for lang in CODEQL_LANGUAGES if f"/language:{lang}" not in analysed)
    if missing:
        problems.append(f"main has no CodeQL analysis for {', '.join(missing)}")
    return problems


@check("every label the issue forms and Dependabot apply exists")
def label_references(repo):
    have = {l["name"] for l in pages(f"repos/{repo}/labels?per_page=100")}
    wanted = {}
    for path in list((ROOT / ".github" / "ISSUE_TEMPLATE").glob("*.yml")) + [ROOT / ".github" / "dependabot.yml"]:
        text = path.read_text()
        for m in re.finditer(r"^labels:\s*\[(.*?)\]", text, re.M):
            for label in (x.strip().strip("\"'") for x in m.group(1).split(",") if x.strip()):
                wanted.setdefault(label, path.name)
        for block in re.finditer(r"^\s+labels:\n((?:\s+- .+\n)+)", text, re.M):
            for label in (x.strip()[2:].strip() for x in block.group(1).splitlines()):
                wanted.setdefault(label, path.name)
    return [f"{label} is applied by {source} but does not exist" for label, source in sorted(wanted.items()) if label not in have]


def dependabot_entries():
    """(ecosystem, directories) of each update in .github/dependabot.yml."""
    entries, current, in_list = [], None, False
    for line in (ROOT / ".github" / "dependabot.yml").read_text().splitlines():
        m = re.match(r"\s*- package-ecosystem:\s*(\S+)", line)
        if m:
            current, in_list = (m.group(1), []), False
            entries.append(current)
        elif current is None or not line.strip() or line.lstrip().startswith("#"):
            continue
        elif re.match(r"\s+directory:\s*\S+", line):
            current[1].append(line.split(":", 1)[1].strip().strip("\"'"))
            in_list = False
        elif re.match(r"\s+directories:\s*$", line):
            in_list = True
        elif in_list and re.match(r"\s+- \S+\s*$", line):
            current[1].append(line.strip()[2:].strip().strip("\"'"))
        else:
            in_list = False
    return entries


@check("Dependabot watches every lockfile, Dockerfile and composite action, or the audit says why not")
def dependabot_coverage(repo):
    tree = gh(f"repos/{repo}/git/trees/HEAD?recursive=1")
    if tree.get("truncated"):
        raise RuntimeError("the repository tree is too large to list in one call")
    need = {}
    for item in tree["tree"]:
        path = item["path"]
        name = path.rsplit("/", 1)[-1]
        folder = "/" + path.rsplit("/", 1)[0] if "/" in path else "/"
        if name == "Cargo.lock":
            need[("cargo", folder)] = path
        elif name in ("package-lock.json", "pnpm-lock.yaml", "yarn.lock") and "node_modules" not in path:
            need[("npm", folder)] = path
        elif name == "Dockerfile" or name.startswith("Dockerfile."):
            need[("docker", folder)] = path
        elif re.fullmatch(r"\.github/actions/[^/]+/action\.ya?ml", path):
            need[("github-actions", folder)] = path
    watched = dependabot_entries()
    return [
        f"{path} ({ecosystem}) is not in .github/dependabot.yml"
        for (ecosystem, folder), path in sorted(need.items())
        if folder not in UNWATCHED and not any(e == ecosystem and any(fnmatch.fnmatchcase(folder, p) for p in dirs) for e, dirs in watched)
    ]


@check("open pull requests from others have a maintainer's reply within the window")
def pull_replies(repo, days):
    owner = repo.split("/")[0]
    problems = []
    for pr in pages(f"repos/{repo}/pulls?state=open&per_page=100"):
        author = pr["user"]["login"]
        if author == owner or author.endswith("[bot]"):
            continue
        comments = pages(f"repos/{repo}/issues/{pr['number']}/comments?per_page=100")
        last = comments[-1] if comments else None
        if last is None or last["user"]["login"] != owner:
            since = last["created_at"] if last else pr["created_at"]
            if age_days(since) > days:
                problems.append(f"#{pr['number']} by {author} waited {age_days(since)} days")
    return problems


@check("discussions have a reply within the window")
def discussion_replies(repo, days):
    owner, name = repo.split("/")
    query = 'query($o:String!,$n:String!){repository(owner:$o,name:$n){discussions(first:50,orderBy:{field:UPDATED_AT,direction:DESC}){nodes{number title createdAt closed author{login} comments{totalCount}}}}}'
    data = gh("graphql", "-f", f"query={query}", "-f", f"o={owner}", "-f", f"n={name}")
    problems = []
    for d in data["data"]["repository"]["discussions"]["nodes"]:
        if d["closed"] or (d["author"] or {}).get("login") == owner:
            continue
        if d["comments"]["totalCount"] == 0 and age_days(d["createdAt"]) > days:
            problems.append(f"#{d['number']} {d['title']!r} has no reply after {age_days(d['createdAt'])} days")
    return problems


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default="DrunkOnJava/dwg-rs")
    ap.add_argument("--reply-days", type=int, default=3, help="days a pull request or discussion may wait for a reply")
    args = ap.parse_args()
    issue_labels(args.repo)
    milestones(args.repo)
    release_criteria(args.repo)
    branches(args.repo)
    caches(args.repo)
    main_green(args.repo)
    alerts(args.repo)
    rulesets(args.repo)
    settings(args.repo)
    label_references(args.repo)
    dependabot_coverage(args.repo)
    pull_replies(args.repo, args.reply_days)
    discussion_replies(args.repo, args.reply_days)
    failed = 0
    for status, name, detail in results:
        failed += status == "FAIL"
        print(f"{status:<8}{name}" + (f"\n        {detail}" if detail else ""))
    print(f"\n{len(results) - failed} of {len(results)} checks ok" + (f", {failed} failed" if failed else ""))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
