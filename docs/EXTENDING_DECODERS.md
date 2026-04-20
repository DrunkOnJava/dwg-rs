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

Open `src/entities/dispatch.rs` and add your entity to the match
in `dispatch_entity`. The `ObjectType` enum value is in
`src/object_type.rs`; if your entity isn't there yet, add it with
the correct type code from spec §2.12.

```rust
// src/entities/dispatch.rs
match object_type {
    // ... existing arms ...
    ObjectType::Point => {
        let decoded = point::decode(&mut cursor)?;
        DecodedEntity::Point(decoded)
    }
    // ... rest ...
}
```

And add the variant to the `DecodedEntity` enum:

```rust
pub enum DecodedEntity {
    // ...
    Point(Point),
    // ...
}
```

## Step 4 — Write tests

Every new decoder needs at least two tests:

1. **Synthetic round-trip.** Build a valid payload with
   `BitWriter`, decode it, assert the struct matches:

   ```rust
   #[test]
   fn roundtrip_point() {
       let mut w = BitWriter::new();
       w.write_bd(1.0);
       w.write_bd(2.0);
       w.write_bd(3.0);
       w.write_b(true);  // thickness = default
       w.write_b(true);  // extrusion = default
       w.write_bd(0.0);  // x_axis_angle = 0
       let bytes = w.into_bytes();
       let mut c = BitCursor::new(&bytes);
       let p = decode(&mut c).unwrap();
       assert_eq!(p.position, Point3D::new(1.0, 2.0, 3.0));
       assert_eq!(p.thickness, 0.0);
   }
   ```

2. **Truncation test.** Feed a too-short byte slice and assert the
   decoder returns `Err`, not panics:

   ```rust
   #[test]
   fn decode_errors_on_truncated_input() {
       let bytes = [0x00u8; 2]; // way too short
       let mut c = BitCursor::new(&bytes);
       assert!(decode(&mut c).is_err());
   }
   ```

If you have a known real-world payload (e.g., from a public
corpus file), commit it under
`tests/fixtures/raw_entities/` with a `README.md` explaining the
source — then add a test that decodes it and asserts expected
field values.

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

## Step 6 — Run the tests + linter

```bash
cargo test --release --lib entities::<name>::
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
```

Then push your PR. The template at
`.github/PULL_REQUEST_TEMPLATE.md` includes the clean-room
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
