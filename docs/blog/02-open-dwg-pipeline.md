# From .dwg bytes to SVG, DXF, and (eventually) glTF

*How `dwg-rs` stitches a container reader, a curve/path model, and
per-format writers into one pipeline — and what still has to land
before the output looks like your drawing.*

DWG isn't useful by itself. You open a `.dwg` file to turn it into
*something else* — a PDF, an SVG for a web viewer, a DXF for
downstream tooling, a glTF mesh for a 3D viewer, a JSON dump for a
CI diff. A file-format crate that only parses but doesn't render
leaves every user to reinvent the rendering layer.

`dwg-rs` is deliberately structured so that the parsing side
(container, sections, bit-streams) feeds into a small set of
neutral geometric types, and the rendering side (SVG, DXF, glTF)
consumes those neutral types without knowing anything about DWG's
on-disk layout. That separation is what makes alpha-quality
decoders still useful end-to-end: the container parts that work
(metadata, object enumeration, thumbnail carving) produce real
output today, and when the per-entity decoders land in 0.2.0 the
same renderers will start emitting real geometry without any
pipeline changes.

This post walks that pipeline.

## The shape of the library

The library has three tiers, and each tier only depends on the one
below it:

```
  ┌──────────────────────────────────────────────────┐
  │  Writers:    svg::SvgDoc   dxf::DxfWriter        │
  │              dxf_sections::*   (glTF: pending)   │
  ├──────────────────────────────────────────────────┤
  │  Neutral geometry:   curve::Curve / Path         │
  │                      geometry::BBox3             │
  ├──────────────────────────────────────────────────┤
  │  Parsers:    reader::DwgFile, section_map,       │
  │              lz77, bitcursor, handle_map,        │
  │              entities::*, tables::*, objects::*  │
  └──────────────────────────────────────────────────┘
```

A writer never calls a parser. A parser never calls a writer. Any
round-trip goes through the middle tier.

## The middle tier: `Curve` and `Path`

The central type is `curve::Curve` — one enum variant per
geometric primitive a CAD drawing actually contains:

```rust
pub enum Curve {
    Line   { a: Point3D, b: Point3D },
    Circle { center: Point3D, radius: f64, normal: Vec3D },
    Arc    { center: Point3D, radius: f64, normal: Vec3D,
             start_angle: f64, end_angle: f64 },
    Ellipse { /* major axis + ratio + angles */ },
    Polyline { vertices: Vec<PolylineVertex>, closed: bool },
    Spline(Spline),    // NURBS: control points + knots + weights
    Helix  { /* parametric */ },
}
```

`PolylineVertex` carries a `bulge` factor between this vertex and
the next (`tan(θ/4)` where θ is the included angle of the arc
between them — 0 is straight). A polyline with non-zero bulges is
a series of arcs stitched end-to-end, which is how AutoCAD
represents everything from a rounded rectangle to a property
boundary with curved easements. The writers know about bulge; the
parser's job is just to populate the field.

There is no "tessellated polyline" variant — a NURBS stays as its
control points and knots. Tessellation is the renderer's concern,
at a tolerance the renderer picks. That matters because SVG output
for a spline looks very different from glTF output: SVG has native
cubic Bézier support and probably wants 32 control points;
a glTF mesh wants hundreds of triangles.

`Path` composes `Curve`s — a multi-segment path a DXF LWPOLYLINE
or HATCH boundary would emit. `BBox3` is a 3D axis-aligned box
that every geometry type knows how to contribute to; the SVG
writer uses it to compute `viewBox`.

## Getting from `.dwg` bytes to `Curve`s

The parser layer runs this sequence for you when you call
`DwgFile::open`:

```rust
use dwg::DwgFile;

let file = DwgFile::open("drawing.dwg")?;
println!("version: {}", file.version());
println!("sections: {}", file.sections().len());

// Any named section's decompressed bytes — fully reliable.
if let Some(Ok(bytes)) = file.read_section("AcDb:Preview") {
    std::fs::write("thumbnail.bmp", &bytes)?;
}

// Structured metadata — works on every corpus file tested.
if let Some(Ok(summary)) = file.summary_info() {
    println!("title:  {}", summary.title);
    println!("author: {}", summary.author);
}

// Raw object walk — R2004+ ships; R14/R2000/R2007 pending.
if let Some(Ok(objects)) = file.all_objects() {
    println!("{} objects", objects.len());
    for obj in &objects {
        println!("  handle 0x{:X}: {:?}", obj.handle.value, obj.kind);
    }
}

// End-to-end typed entity decode — alpha quality.
if let Some(Ok((entities, summary))) = file.decoded_entities() {
    println!("{:.1}% decoded", summary.decoded_ratio() * 100.0);
}
```

That last call is where the 0.1.0-alpha.1 limits bite. The
container gives you every object with its correct handle, type
code, and raw payload bytes — but the *typed* decoders underneath
`decoded_entities()` currently succeed on 20–80 % of entities
depending on version (see the
[capability matrix](../../README.md#capability-matrix-at-a-glance)
for the measured numbers). On R2013 it's ~86 %. On R2018 it's
~22 %. On R2004 it's 0 %. The gap is the per-version handle/data
stream split covered in the
[previous post](./01-reading-dwg-without-autocad.md#the-split-stream-architecture),
not a missing renderer.

## From `RawObject` to `Curve`: the bridge module

The newest piece in the stack is `src/entity_geometry.rs` — the
thin glue layer that turns decoded entity structs into `Curve`s:

```
LineEntity      → Curve::Line
CircleEntity    → Curve::Circle
ArcEntity       → Curve::Arc
EllipseEntity   → Curve::Ellipse
LwPolylineEntity→ Curve::Polyline (with bulges)
SplineEntity    → Curve::Spline
PointEntity     → (no direct Curve — rendered as a 3D dot at origin)
```

Each conversion is obvious once both sides exist; the glue is
where unit conversion, `major_axis` direction extraction for
ellipses, and `closed` flag resolution on polylines all live.
Entity types that don't fit the curve model (TEXT, MTEXT, INSERT,
HATCH) don't show up here — they have their own rendering paths
that bypass `Curve` entirely.

Today this module is a thin translation layer because the set of
decoded entities is small. As the 0.2.0 per-entity decoders land,
every new entity type that represents geometry will route through
this same file.

## The writers

### SVG

`svg::SvgDoc` is deliberately string-based — no external SVG
crate, no DOM. Elements are appended as you go, layer groups are
`<g>` nested by name, and `SvgDoc::finish()` produces a complete
document as a `String`. CAD Y-up coordinates are preserved
verbatim in the output; the root `<svg>` element applies a
`transform="scale(1,-1) translate(0,-H)"` to flip for screen
display. Downstream renderers can override via a custom viewBox.

```rust
use dwg::svg::{SvgDoc, Style};
use dwg::curve::Curve;
use dwg::entities::Point3D;

let mut doc = SvgDoc::new(800.0, 600.0);
doc.begin_layer("walls");
let wall = Style {
    stroke: "#000000".into(),
    stroke_width: 2.0,
    fill: None,
    dashes: None,
};
doc.push_curve(
    &Curve::Line {
        a: Point3D::new(0.0, 0.0, 0.0),
        b: Point3D::new(100.0, 100.0, 0.0),
    },
    &wall,
    None,  // optional element id
);
doc.end_layer();
let svg_text = doc.finish();
```

The shipping example, `examples/dwg_to_svg.rs`, is an honest
snapshot of what works today. It reads a `.dwg`, prints the
version and section count, and for each enumerated object emits a
small 3-pixel dot at a deterministic position based on the
object's handle. The output is a visible "fingerprint" of the
file — 745 dots for the AC1032 sample, all drawn in the same
viewBox as the dashed frame — proving the pipeline reaches every
object. When the per-entity decoders fix the handle stream, the
same example starts emitting real lines, arcs, and polylines
without any source changes to the example itself.

### DXF

`dxf::DxfWriter` handles the line-based group-code format. Each
record is a group code (integer 0–1071) on one line, followed by
its value on the next — strictly line-oriented, strictly paired.
Sections begin with `0 SECTION / 2 <name>` and end with
`0 ENDSEC`; the document ends with `0 EOF`.

The core writer tracks section state:

```rust
use dwg::dxf::DxfWriter;
let mut w = DxfWriter::new();
w.begin_section("HEADER");
w.write_string(9, "$ACADVER");
w.write_string(1, "AC1032");
w.end_section();
w.finish();
let dxf = w.take_output();
```

The section emitters in `dxf_sections.rs` sit on top:
`write_header_section`, `write_tables_section`,
`write_blocks_section`, `write_entities_section`,
`write_objects_section`. Each takes a neutral data struct — a
slice of `HeaderEntry`, a slice of `LayerTableEntry`, a slice of
`Curve` — and composes a full section. The writer is decoupled
from the reader: you can hand-assemble DXF from your own model,
synthesize it for tests, or pipe it from a decoded `.dwg`, and
the emitter doesn't know which.

That split is deliberate. A common complaint about CAD file
libraries is that the output writer is tangled with a specific
model class, so substituting your own geometry engine means either
faking the model class or rewriting the writer. `dwg-rs` writers
only touch `Curve` / `Path` / `BBox3` / neutral table-entry
structs.

### glTF (pending)

The glTF 3D export is Phase 10 of the roadmap. It will consume
the same `Curve` types plus `Mesh` data extracted from 3D
entities (3DFACE, POLYFACE_MESH, BODY, REGION, ACIS). Today only
the Curve side of that type model exists; the Mesh side is
scaffolded in `entity_geometry.rs` with `TODO` markers and will
land when the 3D-entity decoders do.

## Where the pipeline is tight

A few things are worth calling out:

**No parse-time renderer coupling.** The parser never knows
whether its output will become SVG, DXF, glTF, or JSON. That
means a single file can feed multiple outputs in one pass without
reparsing, and it means adding a new output format is purely
additive — a new module under the `Writers` tier, no edits
anywhere in the parser tier.

**No I/O in the middle tier.** `Curve`, `Path`, and `BBox3` are
pure data types. They don't read files and they don't write
files; they compose into trees that readers populate and writers
consume. This makes them trivial to unit-test and trivial to
fuzz.

**Memory model is straightforward.** Decompressed section bytes
are owned `Vec<u8>`s. Every `BitCursor` borrows from one of those
vecs. The parser produces owned `Curve`/`Path` values; the writer
borrows them. No lifetime gymnastics leak into user code.

**Limits are explicit.** Every cap (LZ77 output size, handle-map
entry count, spline control-point count, XRECORD size) lives in
the `limits` module with a documented default and a permissive
override. The defaults reject obviously-malformed files; the
permissive profile lets you process unusual inputs intentionally.

## What's shipping vs pending

Against the README capability matrix:

- **Shipping**: DwgFile open, version identification, LZ77
  decompress, section-map parsing, metadata parsers, handle-map
  parsing, raw object enumeration on R2004+, the `Curve`/`Path`
  types, the SVG writer, the DXF writer + section emitters,
  `examples/dwg_to_svg.rs`.
- **Alpha**: per-entity decoders (20–80 % real-file decode rate
  depending on version; see [README's measured numbers](../../README.md#pre-alpha-status--read-this-first)).
- **Pending**: R14/R2000/R2007 object walk, entity graph (owners,
  reactors, blocks), full symbol-table content decoders, glTF
  export, DWG writer stages 2–5.

The pipeline structure is in place. Filling in the decoder gaps
is additive from here — no architectural reshuffling required.

If your use case is "open a DWG, extract metadata, dump decoded
bytes, carve the thumbnail," `dwg-rs` is usable today. If it's
"render the actual drawing," you want to watch the 0.2.0
milestone.

---

*`dwg-rs` is pre-alpha (0.1.0-alpha.1) and Apache-2.0 licensed. No
executable code from the Autodesk SDK, ODA SDK, or GPL-licensed DWG
readers was imported; see [`CLEANROOM.md`](https://github.com/DrunkOnJava/dwg-rs/blob/main/CLEANROOM.md)
for the full source-provenance policy.
[Source on GitHub](https://github.com/DrunkOnJava/dwg-rs).*
