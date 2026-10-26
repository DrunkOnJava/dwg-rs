# 1.0.0 release criteria

Last measured 2026-10-21, on `main` at `b268e884`.

1.0.0 is the first release with a stability commitment
([`RELEASE.md`](../RELEASE.md)). The maintainer set its scope on
2026-10-21: dwg-rs 1.0 has no gaps or limitations. It reads every release
from R13 to R2018 and every section of ODA spec v5.4.1, with values equal to
AutoCAD's own DXF export of the same drawing; it writes DWG that AutoCAD
opens; and its DXF, SVG and glTF exports, its browser viewer and its Python
bindings are complete. This page breaks that into requirements that can be
checked. 1.0.0 is tagged when every requirement has passed; the tracking
epic is #195.

Each requirement has a bar, the value measured now, a status and its
evidence. A status is one of:

- **passed**: measured, and the bar is met;
- **failed**: measured, and the bar is not met;
- **not measured**: no evidence either way.

A closed issue or a milestone's completion percentage is not evidence. A
status changes only in the pull request that links the new evidence.

## Reading

| | Requirement | Bar | Measured now | Status |
|---|---|---|---|---|
| R1 | Measured in public | Every number README.md, STATUS.md and this page publish comes from a CI run on files anyone can fetch, with the run linked | README.md's and STATUS.md's real-file numbers come from the Corpus CI job on 52 public drawings, run linked; STATUS.md's other numbers and this page's do not yet (#94, #112) | **failed** |
| R2 | Every release opens | R13, R14, R2000, R2004, R2007, R2010, R2013 and R2018 files open and walk, on at least two corpus files per release | R14 to R2018 open and walk in CI on at least four drawings per release (eight each for R14 to R2013, four for R2018, all AutoCAD saves with recorded provenance); no R13 file (#119) | **failed** |
| R3 | Every section of the spec is read | Every row of [`spec-coverage.md`](spec-coverage.md) is `complete`: chapters 2 to 29 and all 104 prescriptions of §20.4 | 0 of 138 rows `complete`; 34 `missing`, 33 `partial`, 6 `decoder`, 65 `closes` (CI corpus, #225) (#118, #125, #131, #138) | **failed** |
| R4 | Values equal AutoCAD's | For every section, every value equals AutoCAD's DXF export of the same drawing, matched by handle | Not compared; LINE, CIRCLE and ARC values checked on R2013 against the authored commands only (#99) | **failed** |
| R5 | Classes beyond the spec | Every class the corpus holds that v5.4.1 does not prescribe (VISUALSTYLE, MATERIAL, surfaces and the rest) meets R3 and R4 | VISUALSTYLE and the view styles close; MATERIAL, PROPERTYSET_DATA, MESH and others partial or unmeasured (#152) | **failed** |
| R6 | A corpus that holds everything | Every (section, release) pair has at least one corpus record with a DXF export, from sources with a documented right to fetch them | Two public sources fetched in CI: nextgis (7 simple drawings, R14 to R2013) and ACadSharp (one dense drawing, R14 to R2018, plus three R2018 drawings); AutoCAD's DXF beside every save but R14 and two of the extra R2018 drawings; no R13 (#153) | **failed** |
| R7 | Held-out files | At least two files per release band, used by no development work and measured once before any fix, meet R3 and R4 | None reserved (#158, #196) | **not measured** |
| R8 | Nothing silently wrong | A record that does not decode is reported (diagnostics name its type and why); no decoder returns plausible values for a layout it did not verify | 308 records error honestly on the public corpus (#226, #227, #228, #71); the pre-R2007 symbol tables decode silently wrong (#34); AppInfo is misread (#137) | **failed** |

## Writing

| | Requirement | Bar | Measured now | Status |
|---|---|---|---|---|
| W1 | Round-trips | Every corpus file, every release, written back byte for byte | R2004-family section round-trips; no file-level round-trip (#160) | **failed** |
| W2 | AutoCAD opens it | Written files, round-tripped and authored, open in AutoCAD with no recovery prompt and a clean AUDIT, and in at least two independent readers, on every release, with the evidence recorded | No evidence (#167) | **not measured** |
| W3 | Authoring | A drawing built from nothing through the public API meets W2 | No authoring API (#166) | **failed** |

## Exports, viewer and Python

| | Requirement | Bar | Measured now | Status |
|---|---|---|---|---|
| X1 | DXF | DWG to DXF equals AutoCAD's own DXF of the same drawing, for every target version the source supports | Written for R12 to R2018; checked only syntactically (#169) | **not measured** |
| X2 | SVG and PDF | Rendered geometry equals the decoded geometry for every 2D entity type, within a stated tolerance | Rendered; not measured (#171) | **not measured** |
| X3 | glTF | Every export passes the Khronos validator; solids drawn from their ACIS data | 3DSOLID is a box placeholder (#172, #173) | **failed** |
| X4 | Viewer | No stubs; browser tests open every corpus file; zero network requests after load | Selection, section box, clipping, worker and progressive open are stubs (#174) | **failed** |
| X5 | Python | Wheels on PyPI reach the whole reader; examples run in CI | Type stubs only (#180) | **failed** |

## The stable surface and its hardening

| | Requirement | Bar | Measured now | Status |
|---|---|---|---|---|
| S1 | The stable surface is named | RELEASE.md lists what 1.0 commits to and what it does not | Every module public; nothing named (#192) | **failed** |
| S2 | Breaking changes are caught | cargo-semver-checks and the Python API listing fail CI on an unannounced break | Not checked (#192) | **failed** |
| S3 | Hostile input | One ParseLimits holds every cap; every fuzz target has run an hour with no findings; no panic on 64- or 32-bit targets | Separate limit structs; no recorded fuzz durations; no 32-bit job (#184) | **failed** |
| S4 | Published | crates.io, docs.rs, PyPI and the release binaries checked from what was published | Nothing published (#109, #182) | **failed** |

## Outside evidence

| | Requirement | Bar | Measured now | Status |
|---|---|---|---|---|
| P1 | Practitioners | At least three CAD or BIM practitioners have run the documented workflows on their own drawings and reported; each finding is an issue or a line in the docs | None (#197) | **failed** |

## Limitations

None. A gap at 1.0 is a failed requirement above, not a documented
limitation. Out of scope, and so not counted: releases before R13 (R12 and
earlier DWG are a different format) and the Autodesk and ODA SDKs, which
[`CLEANROOM.md`](../CLEANROOM.md) rules out as sources.
