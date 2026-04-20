# Demo Gallery

> **Status: scaffold.** This gallery activates when the L4 per-entity
> decoders plus the L9 SVG / raster export reach full coverage on the
> measured corpus. Both are tracked in
> [`ROADMAP.md`](../../ROADMAP.md) (milestones `0.2.0` and later). See
> [issue #103](https://github.com/DrunkOnJava/dwg-rs/issues) for the
> per-entity decoder gap that blocks this page.

Real rendered outputs will replace each placeholder below once the
corresponding pipeline passes its ship bar. No renders are published
here yet because shipping synthetic or cherry-picked examples would
misrepresent the current pre-alpha state. Every slot lists what
exactly will be shown, and against which input corpus.

---

## Slot 1 — Floor plan from QCAD examples

**Source corpus.** Files from the
[QCAD examples collection](https://qcad.org/en/download) (GPL-free
sample drawings distributed by the QCAD project for demonstration).
These are small 2D architectural floor plans: walls, doors, windows,
dimensions, room labels.

**What will be rendered.** Full-page SVG via `dwg-rs`'s built-in SVG
back end. Line work, hatches, text, and dimensions — all exported
from the same `DwgFile` instance, with layer colors preserved and
paper-space scale honored. Side-by-side against the same file opened
in a reference CAD viewer to illustrate parity.

**Gate.** Requires R2013 and R2018 `LINE` / `LWPOLYLINE` / `TEXT` /
`HATCH` (no path) / `DIMENSION` decoders at ≥ 90 % on this subset,
plus `svg` module export of at least the six primitive shapes.

---

## Slot 2 — Mechanical drawing from public-domain corpus

**Source corpus.** Public-domain mechanical drawings republished
under a permissive license (candidates include NASA technical
drawings published under `U.S. Gov't — public domain` and
engineering examples from
[`nextgis/dwg_samples`](https://github.com/nextgis/dwg_samples)).

**What will be rendered.** A single-sheet mechanical detail with
orthographic views, leader lines, and a title block. The render will
show `MLEADER`, `MTEXT`, and `INSERT` (block references) working in
combination. A companion JSON dump will list every decoded entity by
handle — this is the "prove the decoder is real, not faked" slot.

**Gate.** `MLEADER` complete decode path (leader-line list + content
MTEXT) plus `INSERT` recursion into `BLOCK_HEADER` owned entities —
currently the biggest structural gap for mechanical drawings.

---

## Slot 3 — Architectural detail

**Source corpus.** A permissively-licensed detail sheet (wall
section, flashing detail, or similar) sourced from open government or
institutional publications. Exact file selected once coverage is
there and a license-clean candidate is picked.

**What will be rendered.** A 1:4-scale detail with heavy use of
`HATCH` fill patterns, leader annotations, and dimensioning.
`HATCH` with boundary paths is the structurally-hardest
2D primitive and is deliberately the last thing in the scaffolded
entity list; this slot exists to demonstrate the full `HATCH`
boundary-path tree, not a simplified version.

**Gate.** `HATCH` boundary-path decoder (ODA spec §26.4) — the
current implementation returns `Error::Unsupported` when
`num_paths > 0`. That guard is intentional and will be lifted only
when the tree walker is correct.

---

## Slot 4 — GIS map export (PNG)

**Source corpus.** A civil-engineering survey map or site-plan
`.dwg` from a state DOT or local-government open-data portal
(license reviewed, source attributed).

**What will be rendered.** Full-page PNG raster at 300 DPI via the
`svg → raster` bridge. Civil drawings use `LWPOLYLINE` heavily for
parcel boundaries and contour lines; this slot validates that
vertex bulge, elevation, and width are decoded accurately. The
render is paired with a coordinate-grid overlay to show that
modelspace-to-paper-space transforms are honored.

**Gate.** `LWPOLYLINE` bulge / elevation / variable-width decode at
100 % on this slot's input file plus the raster export path.

---

## Slot 5 — 3D mesh excerpt (glTF)

**Source corpus.** A simple 3D `.dwg` containing `3DSOLID`,
`REGION`, or `MESH` entities. Candidates are mechanical part files
and small architectural massing models.

**What will be rendered.** A glTF 2.0 file viewable in any standard
glTF viewer, plus a still render for the inline thumbnail. This
slot proves the `3D` track of the project works end-to-end (not just
the `2D` track).

**Gate.** Phase 10 of the roadmap — `3DSOLID` / `MESH` decoders plus
the glTF export module. Both are currently entirely pending.

---

## Why we're publishing the scaffold now

Three reasons. One, to make the scope honest — a prospective user
reading the README gets a clear picture of what will be here once it
works, not a page of "coming soon." Two, to set the shape of the
acceptance bar — the gate for each slot is explicit and testable,
not a judgment call. Three, to give contributors something concrete
to aim at: closing any single slot's gate advances the project
visibly.

This page will be updated as soon as any one slot's gate clears.
Progress is tracked in the `demo-gallery` label on the issue
tracker.
