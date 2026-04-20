# Sibling projects: `rvt-rs` and the case for open CAD interop

*`dwg-rs` has a sibling. Both are clean-room Apache-2.0 Rust
readers for proprietary Autodesk file formats, with identical
legal posture. This post explains why they're designed as a pair.*

[`rvt-rs`](https://github.com/DrunkOnJava/rvt-rs) is a clean-room
Rust reader for Autodesk Revit files — `.rvt`, `.rfa`, `.rte`,
`.rft` — published under Apache-2.0, by the same author as
`dwg-rs`. It opens the OLE/CFB container, decodes Revit's
truncated-gzip streams, extracts metadata and thumbnails, parses
the embedded `Formats/Latest` schema, and enumerates a first-class
inventory of field-type encodings across an 11-release 2016–2026
reference corpus.

Like `dwg-rs`, it is **pre-alpha**. Like `dwg-rs`, the container
layer is the most mature part — schema-directed instance walking
has a verified beachhead on Revit 2024–2026 with 29 per-class
decoders (Wall, Floor, Door, Window, Column, Beam, etc.), but full
geometry extraction is still active research. See
[`rvt-rs/README.md`](https://github.com/DrunkOnJava/rvt-rs) for
the precise "what works today" boundary.

The two projects solve different format problems, but they're
designed as a pair. This is why.

## The Autodesk interop gap

If you're building AEC, BIM, or CAD tooling in an open-source or
permissively-licensed stack, you hit the same wall on both
formats:

- **DWG**: 2D/3D drafting, 40+ years old, used by most of the
  world's construction and engineering shops. Autodesk never
  published a spec. The ODA SDK requires a paid membership. The
  only open implementation (LibreDWG) is GPL-3, which disqualifies
  it from most commercial stacks.
- **RVT/RFA**: Revit's BIM format. The only public spec is the
  OLE/CFB container itself — the *contents* of the streams are
  undocumented. Autodesk's `revit-ifc` exporter runs inside Revit
  and can only emit what the Revit API exposes; real-world IFC
  exports are routinely described in the openBIM community as
  *"very limited"* and *"out of the box, just crap"* (see the
  [OSArch wiki](https://wiki.osarch.org/)).

Both problems close the same door: a permissively-licensed,
vendor-independent reader is the prerequisite to everything
downstream — web viewers, headless converters, CI pipelines that
diff drawings between commits, server-side thumbnail generators,
BIM query engines. Without that foundation, every downstream tool
either pays for a commercial SDK, pulls GPL-3 into its dependency
graph, or runs Revit/AutoCAD headless on a server somewhere.

Neither answer is acceptable for the kind of self-hosted,
composable, permissively-licensed tooling the AEC world has been
slowly building.

## Same legal posture, documented identically

Both repos ship a `CLEANROOM.md` that spells out the allowed and
forbidden sources in the same format:

**Allowed**: ODA's freely-redistributable *Open Design
Specification for .dwg files* PDF (for `dwg-rs`); public Microsoft
OLE/CFB specs + Autodesk's `revit-ifc` project (only its
high-level algorithm descriptions, never its source) for
`rvt-rs`; public academic papers; hex editors and binary diff
tools applied to publicly-released sample files; the Rust
standard library and permissively-licensed crates.

**Forbidden**: Autodesk SDK source (RealDWG, ObjectARX, ObjectDBX,
Revit API C++ source); ODA SDK source (Drawings SDK, Teigha); any
GPL/LGPL/AGPL-licensed implementation of the target format
(LibreDWG for `dwg-rs`, any GPL Revit work for `rvt-rs`);
decompilations of Autodesk binaries; leaked or unofficial
documentation.

Every pull request on either repo carries a contributor
declaration confirming clean-room provenance. If legal review
matters for your downstream adoption, the audit trail is the same
shape in both projects: declaration in every PR body,
cross-referenced by the `CLEANROOM.md` policy, with `NOTICE` files
summarizing the public authority that typically supports
independent file-format reverse engineering for interoperability
(Sega v. Accolade, Sony v. Connectix, EU Software Directive
Art. 6, Australia Copyright Act §47D). None of that is a legal
opinion — just context for reviewers who want to evaluate the
project against their own counsel's criteria.

## Where the two projects connect

They're separate crates today, and will stay separate. DWG and
RVT are different formats, with different container layers,
different compression algorithms (LZ77 vs truncated-gzip),
different bit-packing conventions, and different object models.
Merging them would be architectural malpractice.

But a few things line up:

- **Same author, same MSRV (Rust 1.85 / edition 2024), same
  `#![deny(unsafe_code)]` posture.** If you trust one crate's
  safety claims, the audit surface is similar for the other.
- **Same dependency discipline.** Both crates keep their
  runtime deps minimal (`thiserror`, `bytemuck` on the DWG side;
  `cfb`, `flate2` on the Revit side) and gate any CLI-only
  dependencies behind a feature flag.
- **Complementary output formats.** A pipeline that needs to
  emit IFC4 from a Revit model and DXF from an AutoCAD drawing
  for the same project can use `rvt-rs` and `dwg-rs` side by side
  without pulling in competing licensing regimes.
- **Same pre-alpha honesty.** Both READMEs lead with "what
  doesn't work" before "what does." Neither repo benchmarks
  against the commercial SDK it's reverse-engineering away from.

## If you want to help

The highest-leverage contributions on either side are the same
shape: close the per-entity or per-class decoder gap on real
files, and land the write path.

For `dwg-rs`:

- Per-version entity preamble fixes (R2004 and R2018 are the
  current big gaps; R2013 is ~86 %). See the
  [capability matrix](../../README.md#capability-matrix-at-a-glance)
  and
  [the bit-stream walk-through](./01-reading-dwg-without-autocad.md#the-split-stream-architecture).
- R14/R2000 object-stream walker (different layout from
  R2004-family).
- R2007 Sec_Mask layer-2 bookkeeping (spec §5.2).

For `rvt-rs`:

- Geometry extraction (Phase 5).
- IFC4 `IfcShapeRepresentation` emission (currently the export
  produces a valid spatial tree but geometry-free elements).
- 2016–2023 ADocument entry-point detection (the 2024–2026
  beachhead is solid; earlier releases need their own pattern).

Either repo welcomes contributors who are willing to follow the
clean-room discipline. If you've worked with ODA's or Autodesk's
SDKs commercially or under NDA, both repos' `CONTRIBUTING.md`
files explain how to disclose that so reviewers can flag PRs for
additional review — the discipline doesn't require you to have
never touched the forbidden sources, only to have not consulted
them while writing the contribution in question.

## Where to find each

- **dwg-rs** — [github.com/DrunkOnJava/dwg-rs](https://github.com/DrunkOnJava/dwg-rs)
  — DWG/DXF container reader (AC1014 → AC1032).
- **rvt-rs** — [github.com/DrunkOnJava/rvt-rs](https://github.com/DrunkOnJava/rvt-rs)
  — Revit container reader (`.rvt` / `.rfa` / `.rte` / `.rft`,
  2016–2026).

Both are Apache-2.0. Both are pre-alpha. Both have a
`CLEANROOM.md` and a `NOTICE` and a `THREAT_MODEL.md`. Both are
intended as foundations, not as finished products. If you need
finished, you still pay the SDK toll. If you're willing to help
fill the decoder gaps, both projects give you a place to do that
without GPL-3 contamination and without an NDA'd vendor SDK in
your dependency graph.

Open CAD interop is a long project. These two crates are where it
starts.

---

*`dwg-rs` and `rvt-rs` are both pre-alpha and Apache-2.0 licensed.
No Autodesk SDK, ODA SDK, or GPL-licensed implementation source
was consulted in either.*
