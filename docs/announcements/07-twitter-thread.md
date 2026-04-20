# Twitter / X thread draft

**Status:** Draft. 7 tweets, each under 240 characters (leaves room for URL auto-appending and a handle or two).

**Posting notes:** post from the author's own account. No images in the draft; a screenshot of the `coverage_report.rs` output table is a good candidate for tweet 3 or 4 if added.

---

**1/**
Shipping a pre-alpha today: dwg-rs — a clean-room, Apache-2.0 Rust reader for AutoCAD DWG files (R13 through AC1032).

For ~28 years DWG has been a moat. This is a small, honest pickaxe.

**2/**
Audit-honest framing up front: the container layer ships and is covered by 193 tests. Per-entity decoders are alpha. Real-file decode rates run 22% on R2018 to 86% on R2013. The README publishes the table, not marketing copy.

**3/**
What works: file ID across 8 versions, LZ77 with spec-errata fixes, Section Page Map, Sec_Mask layer-1, CRC-8 + CRC-32, Reed-Solomon (255,239), metadata parsers, handle + class maps, raw object walker.

**4/**
What does NOT work yet: end-to-end entity decode on most real R2004-family files, R14 / R2000 / R2007 walking, DWG writer (stage 1 of 5), SVG / PDF / glTF export, Python bindings. Each is a tracking issue on the roadmap.

**5/**
Clean-room posture: no Autodesk SDK, no ODA Drawings SDK / Teigha source, no LibreDWG (GPL-3) source consulted. Only the ODA Open Design Specification v5.4.1 PDF plus public sample files. Policy lives in CLEANROOM.md.

**6/**
Why Apache-2? LibreDWG is GPL-3, which disqualifies it from most commercial Rust stacks. dwg-rs fills that specific gap — a DWG read path you can pull into a permissive codebase without changing your license posture.

**7/**
Sibling project: rvt-rs — same clean-room posture, same author, for Autodesk Revit (.rvt / .rfa). First open-source tool to enumerate the Formats/Latest class schema inventory. github.com/DrunkOnJava/rvt-rs

**8/**
Repo, roadmap, and the honest decode-rate table: https://github.com/DrunkOnJava/dwg-rs

Feedback welcome via GitHub Discussions. Particularly interested in per-version entity preamble fixes (issue #103) and R14 / R2000 walker contributions.
