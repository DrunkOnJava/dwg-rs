# Adding a new entity decoder

Adding a per-entity decoder is the most-wanted contribution to this
project (see [CONTRIBUTING.md](../CONTRIBUTING.md)). This guide
walks through a worked example — implementing the `POINT` entity —
so you can copy the pattern to any of the 50+ entity types that
need decoders.

## Prerequisites

- Familiarity with bit-level formats. The DWG object stream is
  packed; each field starts immediately after the previous one,
  not byte-aligned.
- A public reference for the field layout. For this project that
  means the **ODA Open Design Specification** v5.4.1. Do NOT
  consult the ODA SDK source or LibreDWG (GPL-3) — see
  [CLEANROOM.md](../CLEANROOM.md).
- A small reproducer file that contains the entity. If you don't
  have one, file an issue under "Corpus submission" — the
  community can often provide a minimal DWG that triggers the
  entity.

## Step 1 — Declare the entity struct

Create `src/entities/<name>.rs`. The struct carries the decoded
field values; it does NOT carry the cursor or decoder state.

```rust
// src/entities/point.rs
use crate::entities::{Point3D, Vec3D};

/// A POINT entity — a marker point per spec §19.4.10.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    /// Position in WCS.
    pub position: Point3D,
    /// Thickness (default 0.0).
    pub thickness: f64,
    /// Extrusion direction (default +Z).
    pub extrusion: Vec3D,
    /// X-axis angle in radians (default 0.0) — used for 2D display
    /// when POINT display mode is > 0.
    pub x_axis_angle: f64,
}
```

## Step 2 — Write the decoder function

The decoder takes a `&mut BitCursor<'_>` positioned at the start of
the entity-specific payload (i.e., AFTER the common entity
preamble) and returns a `Result<YourType>`. Keep the function
small: read fields in spec order, return the struct.

```rust
use crate::bitcursor::BitCursor;
use crate::entities::{read_be, read_bt};
use crate::error::Result;

/// Decode a POINT entity's payload. Cursor is positioned past the
/// common entity preamble.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Point> {
    // §19.4.10: position is a 3×BD (2–66 bits each, depending on
    // the bit-double tag).
    let position = Point3D {
        x: c.read_bd()?,
        y: c.read_bd()?,
        z: c.read_bd()?,
    };

    // Thickness is a BT (BitThickness): 1 bit flag + optional BD.
    let thickness = read_bt(c)?;

    // Extrusion is a BE (BitExtrusion): 1 bit flag + optional 3×BD.
    let extrusion = read_be(c)?;

    // X-axis angle is a BD.
    let x_axis_angle = c.read_bd()?;

    Ok(Point {
        position,
        thickness,
        extrusion,
        x_axis_angle,
    })
}
```

**Rule:** every read uses `?`. No `unwrap`, no `expect`, no
`panic!`. Malformed input must return `Err`, never panic. The fuzz
targets lean on this invariant — a panic breaks the security
promise of the whole crate.

## Step 3 — Register it in the dispatcher

All registration lives in `src/entities/dispatch.rs`. For a
fixed-code entity (type code below 500, from spec §2.12 and the
type table in §5), make four edits:

1. Add the type-code constant next to the others
   (`const OBJECT_TYPE_POINT: u16 = 0x1B;`) and the `ObjectType`
   variant in `src/object_type.rs` if it is missing.
2. Add the variant to `DecodedEntity`, and its arm in
   `DecodedEntity::type_code`.
3. Name it in `fixed_entity_label`. This is the gate
   `dispatch_fixed_code` consults *before* decoding: a code with no
   label comes back `Unhandled` rather than tripping a boundary
   check.
4. Add the arm to `decode_fixed_entity_body`:

```rust
// src/entities/dispatch.rs, fn decode_fixed_entity_body
OBJECT_TYPE_POINT => DecodedEntity::Point(point::decode(c)?),
```

`fixed_entity_label` and `decode_fixed_entity_body` must list the
same codes; the existing `fixed_entity_label_covers_every_body_arm`
check in that file fails when they drift.

You do not call the boundary check yourself. `dispatch_fixed_code`
runs every body through `checked_inline`, which reads the common
entity preamble, calls your decoder, and then requires the cursor to
sit **exactly** on the record's data-stream boundary (the string
stream on R2010+, the `RL` object-data size on R2000-R2007, the size
inside the common entity data on R13/R14). A field list that is wrong
anywhere lands somewhere else and the record reports an error instead
of plausible-looking geometry.

Other kinds of record take other routes in the same file:

- **Custom classes** (type code 500 and up, such as IMAGE or
  MULTILEADER) resolve through `AcDb:Classes` and dispatch on the DXF
  class name in `decode_from_raw_with_class_map`
  (`dispatch_split_stream_class` for entities, `dispatch_object_class`
  for non-entities).
- **Entities with `TV` fields** read their text from the R2007+ string
  stream through `dispatch_split_stream_entity`; see
  `src/string_stream.rs`.
- **Symbol-table entries** go through `dispatch_table_entry`, and
  **non-entity objects** (DICTIONARY, XRECORD, the controls) through
  `dispatch_object`.

## Step 4 — Prove it on real files

The proof that a decoder is right is the corpus, not a synthetic
payload you built to match your own reading of the spec. Do not add
unit tests (the pull-request template asks for none).

1. Measure before and after with the coverage report on real
   drawings:

   ```bash
   cargo run --release --example coverage_report -- path/to/corpus/
   ```

   Records of your type should move from the unhandled column to
   decoded, with no new errors. A record that errors names the bit
   it stopped at and the bit the boundary sits at; that delta is the
   next thing to explain.
2. Check the values, not just the count:

   ```bash
   cargo run --release --example dump_decoded_entities -- path/to/file.dwg
   ```

   Compare the decoded fields with what the drawing actually holds
   (its DXF export, where the corpus has one).
3. When a field list does not close, `probe_entity_budgets` and
   `probe_decode_errors` show each record's bit budget and the error
   per type.

Paste the before and after coverage tables, the commands, and the
corpus records you measured into the pull request. A new fact about
the format also gets a probe under `examples/` and a dated section in
`ARCHITECTURE.md` (see CONTRIBUTING.md).

## Step 5 — Defensive caps

If your decoder loops over a count read from the stream (e.g.,
vertex count, path count, attribute count), add a defensive cap:

```rust
const MAX_VERTICES: u32 = 100_000;

let count = c.read_bl()?;
if count > MAX_VERTICES {
    return Err(Error::SectionMap(format!(
        "<Entity> vertex count {count} exceeds cap {MAX_VERTICES}"
    )));
}
```

Or, better, derive the cap from the remaining payload bits so
small payloads can't DoS:

```rust
let remaining = c.remaining_bits();
let max_possible = remaining / MIN_BITS_PER_VERTEX;
if count as u64 > max_possible.min(MAX_VERTICES as u64) {
    return Err(Error::SectionMap(...));
}
```

This pattern is documented in
`src/entities/lwpolyline.rs::MAX_VERTICES`; copy it for any
count-driven decoder.

## Step 6 — Run the gate

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --release --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

The real-file integration tests (`tests/samples.rs` and its
neighbours) look for the corpus at `../../samples` relative to the
crate and skip silently when it is absent, so a passing run only
covers real drawings if that directory exists.

Then push your PR. The template at
`.github/PULL_REQUEST_TEMPLATE.md` includes the source-provenance
declaration checkbox and spec-reference prompt — please fill in
both.

## Where to find the field layout

The authoritative reference is **ODA Open Design Specification
for .dwg files** v5.4.1, §19.4 (entity-specific sub-sections).
Each entity has a numbered sub-section (LINE = §19.4.20, CIRCLE =
§19.4.8, …) listing the fields, their bit-types, and version-
specific conditionals.

When the spec is ambiguous (it occasionally is), cross-check
against **publicly documented** references:

- ACadSharp (MIT) — read the field list in code comments, NOT
  the implementation logic.
- Upstream DWG format documentation publicly hosted on
  community sites.
- File observations: open a file in AutoCAD, toggle a property,
  save, hex-diff the result.

Do NOT consult:

- ODA SDK source code (the Teigha / Drawings SDK).
- LibreDWG (GPL-3) source.
- Any decompiled AutoCAD binary.
- Any document behind an NDA or paid membership.

## Beyond this example

The patterns above handle ~80% of DWG entity types. Edge cases
worth knowing:

- **HATCH** has a variable-length boundary-path tree; current code
  returns `Error::Unsupported` for non-empty paths until L4-22
  lands a full implementation.
- **SPLINE** stores knots + control points + weights; the NURBS
  representation is straightforward but the conditional-tags
  pattern is subtle.
- **3DSOLID / REGION / BODY** carry an ACIS SAT blob — don't try
  to decode the SAT; pass it through as `Vec<u8>` for later.
- **MULTILEADER** is large and version-dependent; it's usually the
  last of the essential entities people tackle.

Ask in an issue before starting any of those.
