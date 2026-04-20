# Twitter / X thread draft

**Status:** Draft. 8 tweets, each under 240 characters (leaves room for URL auto-appending and a handle or two).

**Posting notes:** post from the author's own account. No images in the draft; a screenshot of the `coverage_report.rs` output table is a good candidate for tweet 3 or 4 if added. A GIF of `dwg-to-svg` or `dwg-to-gltf` output would pair well with tweet 5.

---

**1/**
Published a pre-alpha today: dwg-rs — an Apache-2.0 Rust reader for AutoCAD DWG files (R13 through AC1032 / 2018+), built from the ODA's published specification.

Narrow niche: LibreDWG (GPL-3) is more complete today; ACadSharp (MIT) covers .NET. This is for Rust stacks that can't take either.

**2/**
Early-preview framing up front: the container layer has landed and carries a multi-hundred test suite. Per-entity decoders are alpha. Real-file decode rates run 22% on R2018 to 86% on R2013. The README publishes the measured per-version table rather than marketing copy.

**3/**
What currently parses: file ID across 8 versions, LZ77 with spec-errata fixes, Section Page Map, Sec_Mask layer-1, CRC-8 + CRC-32, Reed-Solomon (255,239), metadata + SummaryInfo + Preview parsers, handle + class maps, raw object walker.

**4/**
Export pipelines (pre-alpha, partial coverage — see the capability matrix): `dwg-to-dxf` (R12..R2018), `dwg-to-svg` with layers / linetypes / text / MTEXT / hatch / dimension, `dwg-to-gltf` with PBR materials from ACI. Every export is spec-syntactic; real AutoCAD / BricsCAD acceptance is a manual step and is documented.

**5/**
What does NOT work yet: end-to-end entity decode on most real R2004-family files, R14 / R2000 / R2007 walker, DWG writer (scaffolded, not round-trip capable), Python bindings (placeholder — see docs/python.md). Each is a tracking issue on the public roadmap.

**6/**
Source provenance: executable code from Autodesk SDK, ODA Drawings SDK / Teigha, and GPL-licensed readers was not imported. Built from the ODA Open Design Specification v5.4.1 PDF plus public sample files. Full policy in CLEANROOM.md.

**7/**
Why Apache-2? LibreDWG is GPL-3, which downstream Rust stacks with permissive licensing can't absorb. dwg-rs fills that specific gap — a DWG read path permissive downstreams can pull in, for the subset of DWG features already covered.

**8/**
Sibling project, same day: rvt-rs — same source-provenance policy, same author, for Autodesk Revit (.rvt / .rfa 2016-2026). github.com/DrunkOnJava/rvt-rs

**9/**
Repo, roadmap, and the measured decode-rate table: https://github.com/DrunkOnJava/dwg-rs

Feedback welcome via GitHub Discussions. Particularly interested in per-version entity preamble fixes (issue #103) and R14 / R2000 walker contributions.

This is pre-alpha software. See [capability table](https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance) for measured decode rates.
