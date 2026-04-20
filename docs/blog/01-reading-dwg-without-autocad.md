# Reading DWG without AutoCAD: notes from the bit-stream

*A technical walkthrough of the primitives that make DWG hard — and why
a clean-room reader is tractable anyway.*

DWG is a 40-year-old binary file format whose spec was never published
by its vendor. What you can use — legally, for interoperability work —
is the Open Design Alliance's **Open Design Specification for .dwg
files v5.4.1**, a freely-redistributable PDF that is distinct from
the ODA SDK's license. Everything in this post cites that document.
Executable code from the Autodesk SDK, the ODA SDK, and GPL-licensed
DWG readers (LibreDWG) was not imported during `dwg-rs`' build; one
scoped exception — algorithm-description comments in the MIT-licensed
ACadSharp for one LZ77 offset-encoding spec ambiguity — is documented
in [`CLEANROOM.md`](https://github.com/DrunkOnJava/dwg-rs/blob/main/CLEANROOM.md).

This isn't a tour of DWG as a whole. It's a focused walk through the
three layers most likely to bite an engineer writing a reader from
scratch: **the bit-packed primitives**, **the LZ77 dialect**, and
**the split-stream architecture** that changed meaningfully across
R2004, R2007, and R2010+.

## Why bits, not bytes

Start at the primitive layer. Most binary formats think in bytes:
"here's a u32, here's a 16-byte struct." DWG thinks in *bits*, and
its primitives have variable widths depending on a small tag. A
16-bit short can be 2, 10, or 18 bits on disk. A 32-bit long can
be 2, 10, or 34 bits. A double can be 2 bits (yes, two) when the
value is exactly `0.0` or `1.0`.

This is §2 of the spec ("BIT CODES AND DATA DEFINITIONS"). The tag
format is memorable once you've seen it: read a two-bit prefix, then
dispatch on the four possible values to decide how many more bits
follow. §2.2 defines `BS` (bitshort):

```
00  →  read 16 more bits as a little-endian i16
01  →  read 8 more bits as an unsigned byte
10  →  value is literally 0
11  →  value is literally 256
```

The payoff: a header variable like "current layer color index" that's
almost always 0 or 256 costs two bits on disk instead of sixteen.
Across a file's thousands of header fields and tens of thousands of
entity records, that compounds. §2.3 (`BL`), §2.5 (`BD`), and §2.13
(the handle reference format) work the same way. A handful of common
cases are tagged to one or two bits; the full-width case costs a
full native read.

The tradeoff: you cannot byte-align your reader. Consuming one BS
may advance the cursor by 2, 10, or 18 bits, and the next field
picks up mid-byte. A reader has to carry a bit index and track an
in-byte offset.

That's what `BitCursor` in `dwg-rs` does:

```rust
use dwg::bitcursor::BitCursor;

// A three-byte slice containing: 0b00 (dispatch → "0") + 0b10 (dispatch → "256") + ...
let bytes = [0b00_10_0000u8, 0x00, 0x00];
let mut cur = BitCursor::new(&bytes);

let a = cur.read_bs()?;   // returns 0   (two bits consumed)
let b = cur.read_bs()?;   // returns 256 (two more bits consumed)
assert_eq!(cur.position_bits(), 4);
```

Every primitive read is MSB-first within each byte. `align_to_byte()`
is called only when the spec demands a byte boundary — typically
before a CRC-aligned checksum (§2.14).

There's nothing exotic about the code — it's small-integer arithmetic
over a `&[u8]`. What's uncommon is the *discipline*: you cannot
freely `&bytes[offset..]` because the next field might not start at
a byte boundary.

## LZ77, but not quite

Jumping up one layer: §4.7 defines DWG's section compression. It's
LZ77 — literal runs interleaved with back-reference copies — but it
isn't zlib, it isn't DEFLATE, and it isn't one of the usual LZSS
dialects. Three quirks matter.

**Quirk 1: `0x11` terminates.** No external length, no sentinel
checksum, no framing container — the stream ends when the decoder
hits opcode `0x11`. A truncated stream looks indistinguishable from
a complete one until the decoder runs out of input before finding
the terminator. Defensive readers need an explicit "out of input"
error path.

**Quirk 2: literal lengths use a zero-byte run-length extension.**
When a length field reads as `0x00`, the decoder reads another byte
and adds 0xFF to a running total. Each subsequent `0x00` adds
another 0xFF. The first non-zero byte terminates the extension and
contributes its own value plus 3. In practice this is how the
format encodes literal runs longer than 15 bytes without burning a
full-width length field. In adversarial practice it is how you
declare arbitrarily large literal runs with a compact compressed
prefix — a classic decompression-bomb shape that every reader needs
an explicit output cap for.

**Quirk 3: offset encoding depends on the opcode class.** Copy
operations don't use a single "here's the offset" format. Opcodes
`0x10` / `0x12..=0x1F` add `0x4000` to the encoded offset (long
back-references, up to ~80 KB). Opcodes `0x20..=0x3F` add `1`.
Opcodes `0x40..=0xFF` pack two bits of offset into the opcode's low
nibble and encode a 0-to-3 literal count directly in bits 0 and 1 of
the same opcode. Opcodes `0x00..=0x0F` are reserved and signal a
corrupt stream.

There's also a subtle spec/reality gap. The §4.7 prose describes
offsets as "+0x3FFF" (long) and "+0" (short). Real AutoCAD output
shows every class adding one more than the spec suggests — the
offsets are effectively 1-indexed. `dwg-rs` handles this as
`off + 0x4000` / `off + 1`. This matches ACadSharp's comments
(MIT-licensed; we cross-checked the algorithm description, never
the source) and the `nextgis/dwg_samples` corpus. The spec isn't
wrong so much as under-specified on the zero-boundary behavior.

On top of all that, back-references can reach *forward* into the
output buffer as it grows. If the declared `comp_bytes` exceeds the
current distance-to-end, the copy wraps and repeats the tail —
DWG's version of run-length encoding over an LZ77 back-reference.
The output buffer is walked one byte at a time, not with a bulk
`copy_from_slice`, specifically because the source and destination
windows overlap.

## The split-stream architecture

Where things get genuinely different across versions is the
*layout* of an object record inside the decompressed
`AcDb:AcDbObjects` section bytes.

**R2004 (AC1018)** writes each object as a single bit-stream: a
modular-short byte count, then the object's preamble, then the
type-specific payload, then a trailing run of handle references to
owners / reactors / layer / linetype / material. Everything lives
in one forward-flowing stream.

**R2007 (AC1021)** layers a two-stage obfuscation on top of
that. §5 of the spec calls it Sec_Mask: an LCG-driven byte-level
XOR (easy — the LCG is `state = state * 0x343FD + 0x269EC3`, seed
per-section), followed by a bit-level rotation inside 7-byte
windows that tracks a cumulative bit offset through the section.
`dwg-rs` ships the first layer; the second is scaffolded but not
yet wired in, which is why R2007 files currently fail out of the
section-map parser with an explicit "not yet implemented" error
rather than silently returning garbage. The obfuscation adds no
compression — it's purely a layout change. No one outside Autodesk
has ever explained publicly why R2007 got this treatment.

**R2010+ (AC1024, AC1027, AC1032)** introduced the change that
makes per-entity decoding hardest for a new reader: the object's
handle references are **not** at the tail of the data stream. They
live in a *separate* handle-stream at the end of the record, and
the split point is encoded as a modular-char bit count at the start
of the payload. §20.1 calls it the "object handle stream size" but
doesn't draw the picture.

What that means for the decoder: you read the payload size (in
bytes) from the record header, read the handle-stream bit count
from the start of the payload, back-compute the data-stream length
as `payload_bits - handle_stream_bits - sizeof(preamble)`, and then
feed the data stream and handle stream to separate `BitCursor`s.
When a field in the data stream says "owner handle follows," the
owner handle is read from the handle cursor, not the next bits of
the data cursor. When the common-entity preamble says "layer is not
by-layer, read the layer handle," same thing.

R2013 and R2018 keep this split; R2007 doesn't have it (different
reasons for the layout difference — Sec_Mask took R2007's
engineering budget that year, best guess). The
practical consequence is that any reader that works on R2010 by
treating the payload as a single bit-stream will read garbage
handle-references. We know — that's roughly what our current
per-entity decoders do, and it's exactly why the
[README's measured decode rate](../../README.md#pre-alpha-status--read-this-first)
is in the 20–80 % range depending on version: the container layer
is solid, but the entity payload layer still reads the tail of the
payload as if it were one stream.

Closing that gap is the 0.2.0 milestone. Until then, you can still:

- Identify the version (R13 → R2018) against every sample we have,
- Decompress any section,
- Parse metadata (`SummaryInfo`, `AppInfo`, `Preview`, `FileDepList`)
  across ANSI and UTF-16 variants,
- Walk the raw object stream on R2004+ and get 745 objects out of
  the 745-object AC1032 sample with correct handles and type codes.

You just can't yet *decode the fields* of those 745 objects on a
real file. That's an honest pre-alpha disclosure, not a disclaimer
for show.

## Why this is tractable anyway

The layers separate cleanly. `lz77::decompress` is a pure function
over bytes. `BitCursor` is a pure reader over bytes. `SectionMap`,
`HandleMap`, and `Classes` are pure parsers over the output of
`decompress`. The bits that *don't* work today — the per-entity
decoders on real files — do not contaminate the bits that do. A
downstream caller today can get decompressed bytes for any named
section, enumerate every object's handle and type code, and carve
the PNG thumbnail. The harder problem (per-version field layout)
sits cleanly on top, waiting for per-version fixtures that weren't
affordable to build out of synthetic data alone.

That separation is what makes clean-room reverse engineering
tractable. Not "we figured it all out up-front," but: "each layer
we finish is frozen, tested, and doesn't need to be revisited when
we go back to close the next layer."

If you're building CAD tooling in Rust and want an Apache-2.0
foundation that doesn't pull GPL-3 into your dependency graph or
require an ODA SDK membership, [dwg-rs is public on GitHub](https://github.com/DrunkOnJava/dwg-rs).
Contributions to the 0.2.0 milestone (per-entity decoders,
handle-driven walk) are especially welcome.

---

*`dwg-rs` is pre-alpha (0.1.0-alpha.1). Do not benchmark against
the ODA SDK. See the [capability matrix](../../README.md#capability-matrix-at-a-glance)
for shipping vs pending features.*
