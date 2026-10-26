# Maintaining dwg-rs

How the tracker is run, so contributors know what to expect and the
maintainers do the same thing each time. The rules for contributors are in
[`CONTRIBUTING.md`](../CONTRIBUTING.md), the release procedure in
[`RELEASE.md`](../RELEASE.md), and the plan in [`ROADMAP.md`](../ROADMAP.md).

## The plan is the milestones, the epics and the spec

- A **milestone** is a release line (0.2.0, 0.3.0, ... 1.0.0). It is closed
  when its release ships; its open issues move to the next milestone that
  serves them.
- An **epic** (`type:epic`) is a tracking issue in one milestone. Its work is
  its GitHub sub-issues; the epic closes when every sub-issue has closed
  with its evidence.
- A **sub-issue** is one deliverable with an acceptance list. Work on the
  object prescriptions of spec chapter 20 is grouped by family: one
  sub-issue per family, with a checkbox for every section and every release
  band. A section's checkbox becomes its own issue when someone picks it up,
  and that issue quotes the section's field list and names the corpus
  records it is measured on.
- [`docs/spec-coverage.md`](spec-coverage.md) has one row for every section
  of ODA spec v5.4.1 with its state on `main`. A state changes only in the
  pull request that brings its evidence: edit
  `docs/data/spec-coverage.tsv`, then run
  `python3 tools/ci/spec_coverage.py --write`. CI's Spec coverage job fails
  when a section has no row or the page and its data disagree.
- [`docs/release-1.0-criteria.md`](release-1.0-criteria.md) is the 1.0
  release decision. A milestone's completion percentage is not; it counts
  closed issues, not requirements met.

## When a spec section is done

A section of the spec moves through these states in `docs/spec-coverage.md`:

| State | Meaning |
|---|---|
| `missing` | No decoder. |
| `decoder` | Implemented, but no corpus record measures it. |
| `partial` | Known incomplete, or erroring on some corpus record (an issue says which). |
| `closes` | The field list ends on the record's exact data-stream boundary (and the string and handle streams' on R2007+) on every corpus record of the type. |
| `complete` | Closes on every release band, R13 to R2018, and every value equals AutoCAD's own DXF export of the same drawing. |

A boundary that closes proves the field widths add up, not that the values
are right: two fields read in the wrong order can still close. Only the DXF
oracle moves a section to `complete`.

## Triage

Every issue and pull request gets a reply that says what happens next. New
issues from the forms arrive with `needs-triage`.

1. Questions go to [Discussions](https://github.com/DrunkOnJava/dwg-rs/discussions).
   A duplicate is closed with a link to the issue it duplicates.
2. Every open issue carries one `type:*`, at least one `area:*` and one
   `priority:*`; `needs-triage` comes off when they are set. Add a
   `release:*` label for each release band a defect affects.
3. An issue with an acceptance criterion goes into the milestone it serves
   and under the epic it belongs to. Research with no acceptance criterion
   yet stays unscheduled, so it neither blocks a release nor inflates one.
4. An issue is closed when its acceptance criteria are met, with the evidence
   linked (a CI run, a measurement, a commit), or when it is superseded, with
   the superseding issue linked. It is never closed for being old.
5. Work blocked on something outside the code carries `blocked:sample` (a
   corpus file with variation the corpus lacks) or `blocked:autocad` (AutoCAD
   is needed to author or verify).

### Labels

| Label | Meaning |
|---|---|
| `type:bug`, `type:feature`, `type:spec`, `type:research`, `type:docs`, `type:test`, `type:ci`, `type:release`, `type:security`, `type:project-management`, `type:epic` | What kind of work it is. `type:spec` implements or verifies a numbered section of the spec. |
| `area:*` | The part of the format or code: `bitstream`, `container`, `compression`, `integrity`, `header-vars`, `classes`, `handles`, `metadata`, `data-storage`, `objects`, `entities`, `tables`, `string-stream`, `graph`, `geometry`, `writer`, `dxf`, `render`, `viewer`, `python`, `cli`, `corpus`, `oracle`, `fuzz`, `perf`, `ci`, `docs`, `github`, `security`. |
| `release:R13` to `release:R2018` | The release bands an issue affects. |
| `priority:P0` to `priority:P3` | P0 is immediate release or security impact, P1 the current milestone's path, P2 quality and completeness, P3 follow-up. |
| `blocked:sample`, `blocked:autocad` | What the work is waiting for. |
| `needs-triage`, `good first issue`, `help wanted`, `community`, `question`, `duplicate`, `invalid`, `wontfix` | Process. |
| `dependencies`, `rust`, `github-actions` | Dependabot's pull requests. |

## Pull requests

`main` is governed by one ruleset, `main-protection`, kept as code in
`.github/rulesets/`: squash-merge only, linear history, signed commits,
resolved review threads, and up to date with `main` before merging. The
required checks are Rustfmt, Clippy, Test on Linux, macOS and Windows (stable),
Verify MSRV (1.85), Documentation, cargo-deny, the PII guard, Streaming E2E,
Corpus (public DWG) and Spec coverage.
Nobody can bypass it, the maintainer included; an emergency change goes through
a pull request like any other. One issue or finding per pull request, with the
spec section cited and the source-provenance declaration from
[`CLEANROOM.md`](../CLEANROOM.md).

When several decoder PRs are open at once, each merge leaves the others stale
on the generated files: `tools/corpus/baseline.tsv`, `docs/spec-coverage.md`,
the notes in `docs/data/spec-coverage.tsv`, the spec-example fixtures and the
`## 7x.` letters of new ARCHITECTURE.md sections. `tools/maintainer/train/`
lands such a queue one PR at a time:
`tools/maintainer/train/train.sh <pr>...` merges `main` into each branch (no
force push), renumbers its ARCHITECTURE.md section after main's, merges the
coverage table cell by cell, regenerates every generated file from a real run
of the public corpus, refuses any count that is worse than main's baseline,
pushes, and turns squash auto-merge on only once the Corpus job is green and no
review thread is open. A coverage note both sides rewrote keeps the pull
request's text, and main's dropped text is posted on the PR for review; a
conflict in a section's state, or in any file outside that set, stops the train
for that PR. The queue, log and per-PR evidence live in `.git/train/` of the
clone it runs from; `common.sh` lists the environment variables that move them.

## Housekeeping

At every release, and at least monthly:

1. Every open issue has its labels, and where it has acceptance criteria a
   milestone and a parent epic; closed milestones have no open issues.
2. Every open issue under the 1.0.0 milestone serves a requirement in
   `docs/release-1.0-criteria.md`, and every status there links evidence
   that is still current.
3. `docs/spec-coverage.md` agrees with `main`: no section claims more than
   its last measurement.
4. No stale branches on the remote; `delete_branch_on_merge` is on.
5. The Actions cache is under its 10 GB limit.
6. Workflow runs on `main` are green; a red check is a defect to fix.
7. Dependabot pull requests and security alerts are triaged.
8. Discussions and outside pull requests have an answer.
9. The rulesets match their files in `.github/rulesets/`.
