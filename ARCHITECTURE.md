# Architecture

This document is the canonical technical reference for how `dwg-rs`
is organized and why. Read this before making non-trivial changes.

All byte offsets, field names, and section numbers cite the
**Open Design Specification for .dwg files v5.4.1** — freely
redistributable from the Open Design Alliance, *not* part of ODA's
SDK license. This crate was implemented exclusively against that
document; no ODA SDK source, Autodesk SDK source, or LibreDWG
(GPL-3) source was consulted.

## 1. The DWG format in one page

A DWG file is a layered container. From the outside in:

```
┌─────────────────────────────────────────────────────────────┐
│ Bytes 0x00..0x80 — FILE OPEN HEADER (plaintext)             │
│   6-byte version magic ("AC1032"), padding, image pointer,  │
│   CRC-32 of the header block.                               │
├─────────────────────────────────────────────────────────────┤
│ Bytes 0x80..0xEC — 108-byte ENCRYPTED HEADER                │
│   XOR with the 108-byte "magic sequence" (spec §4.1) —      │
│   a deterministic rand() output with seed=1.                │
│   Decrypted content: pointers to Page Map, Section Info,    │
│   Section Map, and global file parameters.                  │
├─────────────────────────────────────────────────────────────┤
│ Rest of file — a stream of 32-byte-aligned PAGES            │
│   Each page has a 32-byte header XOR-masked with            │
│   Sec_Mask (§4.6): page_type, section_number, compressed    │
│   size, uncompressed size, offset, checksum.                │
│   Page payload: LZ77-compressed bit-stream (§4.7) —         │
│   decompressing yields the "section" bytes.                 │
└─────────────────────────────────────────────────────────────┘
```

Sections are identified by *name* (`AcDb:Header`, `AcDb:Classes`,
`AcDb:Handles`, `AcDb:AcDbObjects`, `AcDb:SummaryInfo`, etc.), not
by page. A single section can span multiple pages; the Section Info
table maps each section name to the list of pages holding its data.

Inside the decompressed section bytes, the DWG format uses a
**bit-packed stream** (spec §2) where primitive types have variable
widths — a 16-bit short might take 2 bits, 10 bits, 18 bits, or
a special sentinel. This bit-level encoding is what `BitCursor`
(read path) and `BitWriter` (write path) handle.

## 2. Module organization

```
src/
├── lib.rs              — public API surface + module list
│
├── bitcursor.rs        — Read bit-packed primitives (B/BB/3B/BS/BL/BLL/BD/MC/MS/H/RC/RS/RL/RD/TV)
├── bitwriter.rs        — Inverse of bitcursor, round-trip tested
├── cipher.rs           — 108-byte magic sequence + Sec_Mask XOR
├── crc.rs              — CRC-8 (16-bit output) + CRC-32 (IEEE)
├── reed_solomon.rs     — §5.13's two R2007 codes: check and correct, plain or interleaved
├── reed_solomon_encode.rs — §5.13 encoder; reproduces AutoCAD's parity bytes
│
├── lz77.rs             — Decompressor (the main read-path hot loop)
├── lz77_encode.rs      — Encoder (literal-only, correctness-first)
│
├── header.rs           — 0x80-byte file header + R2004+ encrypted header
├── section.rs          — Section name enum + kind classification
├── section_map.rs      — R2004-family Page Map + Section Info parser
├── section_writer.rs   — Page emitter (inverse of section_map)
├── r2007.rs            — R2007 container: RS de-interleave, page + section maps (§5.1-§5.4)
├── r21_lz.rs           — R2007's own LZ variant (§5.10)
│
├── handle_map.rs       — AcDb:Handles (object-stream index)
├── classes.rs          — AcDb:Classes (custom-type dispatch table)
├── header_vars.rs      — AcDb:Header variable table (raw bit-stream)
├── metadata.rs         — SummaryInfo + AppInfo + Preview + FileDepList
│
├── object.rs           — Object-stream walker + RawObject type
├── object_type.rs      — Built-in type codes + Custom(N) fallback
├── common_entity.rs    — Shared common entity data preamble for every entity
├── string_stream.rs    — R2007+ split string and handle streams (§19.1)
│
├── entities/           — Per-entity decoders + dispatch.rs (the boundary-checked dispatcher)
├── tables/             — Symbol-table entry decoders
├── objects/            — Non-entity objects: controls, DICTIONARY, XRECORD, styles, LAYOUT, ...
│
├── reader.rs           — DwgFile — the primary public entry point
├── api.rs / limits.rs  — Strict vs best-effort parsing; ParseLimits + WalkerLimits caps
├── graph.rs            — Owner / reactor / layer / style resolution
├── block_expansion.rs  — INSERT → block contents
│
├── streaming/          — no_std byte-demand header grammar + checkpoints (docs/streaming/)
├── byte_source.rs      — Pausable byte delivery for the streaming grammar
│
├── geometry.rs / curve.rs / entity_geometry.rs — Rendering pipeline types
├── dxf.rs / dxf_sections.rs / dxf_convert.rs   — DXF writer
├── svg.rs / gltf.rs    — SVG and glTF exporters
│
├── file_writer.rs      — DWG writer: section assembly into a byte buffer
├── element_encoder.rs / handle_allocator.rs — Entity encoders + handle allocation for the writer
├── python_stubs.rs     — Placeholder for the Python bindings
│
├── error.rs            — Error enum (thiserror)
├── version.rs          — Version enum + AC-magic mapping
│
└── bin/                — CLI tools behind the `cli` feature (the [[bin]] list in Cargo.toml)
```

`src/lib.rs` and the `[[bin]]` entries in `Cargo.toml` are the
authoritative module and binary lists.

## 3. The read pipeline

`DwgFile::open(path)` runs this sequence:

```
 Disk bytes
     │
     ▼
 [Phase A] Identify version    header.rs  ──> Version enum
     │
     ▼
 [Phase A] Parse header block  header.rs  ──> CRC-verified
     │
     ▼
 [Phase A] XOR-decrypt 108b    cipher.rs  ──> plaintext pointers
     │
     ▼
 [Phase B] Locate Page Map +   section_map.rs
             Section Info
     │
     ▼
 [Phase B] Build (name → page) map, kept in DwgFile
     │
     ▼
  user calls .read_section("AcDb:Handles")
     │
     ▼
 [Phase C] For each page of that section:
             • un-mask 32-byte page header (cipher.rs)
             • LZ77 decompress (lz77.rs)
             • concatenate into one byte buffer
     │
     ▼
 [Phase D] Parse that buffer per-section:
             • handle_map::parse
             • classes::parse
             • metadata::SummaryInfo::parse
             • object::ObjectWalker::new
             • ...
     │
     ▼
 [Phase E-F] Per-object / per-entity / per-table decoders consume
             bytes via BitCursor, producing typed Rust structs.
```

Each phase can be exercised independently — `lz77::decompress(stream)`
is a pure function; so is `handle_map::parse(bytes)`.

## 4. Bit-packed primitives

Every DWG primitive is read via one of ~14 methods on `BitCursor`.
The encoding is MSB-first within each byte; reading happens at
bit granularity. Key methods:

| Method          | Stream shape                                     | Range                    |
|-----------------|--------------------------------------------------|--------------------------|
| `read_b`        | 1 bit                                            | `bool`                   |
| `read_bb`       | 2 bits                                           | `0..=3`                  |
| `read_3b`       | 3 raw bits (the BLL byte count)                  | `0..=7`                  |
| `read_bs`       | 2-bit tag + {16, 8, 0, 0} bit payload            | `i16`                    |
| `read_bl`       | 2-bit tag + {32, 8, 0, reserved} bit payload     | `i32`                    |
| `read_bd`       | 2-bit tag + {64, 0, 0, reserved} bit payload     | `f64`                    |
| `read_bll`      | 3-bit length + that many LE bytes                | `u64`                    |
| `read_rc/rs/rl/rd` | Byte-aligned raw 8/16/32/64 bit values        | native                   |
| `read_mc`       | Byte stream, bit 7 = continuation                | `i64` (signed)           |
| `read_ms`       | Two-byte modular stream                          | `u64` (unsigned)         |
| `read_handle`   | 4-bit code + 4-bit counter + payload             | `Handle { code, value }` |

The `BitWriter` methods mirror each `read_*` method exactly. Property
tests in `tests/proptest_roundtrip.rs` lock the invariant that
every primitive round-trips bit-exactly.

## 5. LZ77 decompression (spec §4.7)

DWG uses a spec-specific LZ77 dialect with five opcode classes:

| Opcode range   | Form              | What it encodes                             |
|----------------|-------------------|---------------------------------------------|
| `0x01..=0x0F`  | literal-length    | Copy `byte + 3` literals from input         |
| `0x00`         | extended-literal  | Running total: 0x0F + 0xFF per extra 0x00   |
| `0x10`         | long back-ref     | Offset += 0x4000                            |
| `0x12..=0x1F`  | short class       | compBytes = (opcode & 0x0F), offset in op2  |
| `0x20`         | mid back-ref      | compBytes follows as extended count         |
| `0x21..=0x3F`  | two-byte offset   | low 6 bits = offset high bits               |
| `0x40..=0xFF`  | compact           | compBytes + 2-bit offset high + litCount    |
| `0x11`         | terminator        | End of stream                               |

**Spec errata the reader accounts for:** the raw spec's offset
encoding has a pervasive off-by-one error in the `0x10`, `0x12-0x1F`,
and `0x40-0xFF` classes — the decoded offset needs `+1` to match
real files. Cross-verified against ACadSharp (MIT); `lz77.rs` has
inline references to the exact opcode-class fixes.

## 6. R2004+ Sec_Mask

Starting with R2004 (`AC1018`), Autodesk obfuscated the 32-byte
page headers. Each 4-byte word of the raw header is XOR'd with a
mask derived from the page's file offset:

```
Sec_Mask(offset) = 0x4164536B XOR offset
```

XOR is its own inverse — the same operation encrypts and decrypts.
The reader un-masks headers; the writer re-masks them. The 0x80-byte
"encrypted header" earlier in the file uses a different XOR scheme
(the 108-byte magic sequence in `cipher.rs`).

R2007 (`AC1021`) layered a *second* Sec_Mask on top of section
payloads — a bit-level rotation of 7-byte windows combined with
another byte XOR. `r2007.rs` scaffolds the first layer; the second
layer's full bookkeeping is a pending follow-on. Every other R2004
family version (R2010 / R2013 / R2018) uses the simpler one-layer
Sec_Mask and works today.

## 7. Object stream navigation

The `AcDb:AcDbObjects` section holds one variable-length record per
drawing object. The authoritative enumeration comes from
`AcDb:Handles`, which is a compact index of `(handle, byte_offset)`
pairs:

```
 AcDb:Handles section bytes:
   [BE u16 size][pairs ...][CRC]  [BE u16 size][pairs ...][CRC]  [0x0000]
   └──── handle section 1 ─────┘  └──── handle section 2 ─────┘  terminator
        pair = (unsigned MC handle delta, signed MC offset delta)
```

`HandleMap::parse` walks this index, applies the deltas to recover
absolute handles and offsets, and returns a sorted list. The walker
then seeks to each offset, reads the object's size + type code + body.

Two encoding rules govern the pair run, both established by the
`sample_AC1032.dwg` audit in #43/#44:

- The **handle delta is an unsigned modular char**; only the **offset
  delta is signed**. Handles grow monotonically inside a handle
  section, so the encoder spends all seven low bits of the terminating
  byte on magnitude; byte offsets genuinely move backward when deleted
  objects' space is reused, so those keep the 0x40 negation bit.
- **Both accumulators restart at zero on every handle section.** The
  first pair of each section is absolute, not relative to the previous
  section's last entry.

The object stream checks the map against itself: every record repeats
its own handle, so "does the record at the offset this map produced
carry the handle this map produced?" is a free oracle.
`ObjectWalkSummary::handle_mismatches` reports each disagreement, and
`examples/probe_class_census.rs` cross-checks the walk against the
`num_objects` instance counts in `AcDb:Classes`.

For R2010+, a 2-bit "object type tag" preceeds the 16-bit type code
to compress common type numbers into 1 byte — see `object_type.rs`
`ObjectType::read` for the dispatch.

### A record's `MS` excludes its own `MC` (finding, 2026-08-30, #75 / #77)

A record is framed `MS field | MC field | MS bytes of payload | 2-byte
CRC`, where the `MC` — the handle-stream size in bits, R2010+ only —
is **not counted by the `MS`**. The stream is its own evidence: laying
every walked record's `MS`-sized span end to end leaves a run of bytes
between each pair of records, and every one of those runs equals the
width of the *preceding* record's `MC` field.

| Reading | Agreements |
|---|---|
| run = preceding record's `MC` width | **841 of 841** |
| run = following record's `MC` width | 734 of 841 |

Measured on `sample_AC1032.dwg` (R2018), with 100 % agreement on
`arc_2010.dwg`, `line_2010.dwg` and `line_2013.dwg` as well. Only 8 of
those 916 bytes are zero, so they are content, not alignment padding;
772 one-byte plus 70 two-byte `MC` fields plus the 4-byte `0x0dca`
section prologue account for all 916 exactly.

Consequences, all of them visible in `cargo run --release --example
probe_class_census`:

- `RawObject::raw` spans `mc_width + size_bytes`, so the slice reaches
  the last bytes of the record's handle stream. `size_bytes` still
  reports the `MS` verbatim, so `raw.len() != size_bytes` on R2010+.
- **Records are sequential after all.** Every R2004+ corpus file tiles
  its object stream with zero bytes between records; the only unclaimed
  bytes are the 4-byte prologue. Pre-R2010 records carry no `MC` and
  always tiled.
- The sequential (handle-map-less) walk no longer drifts by one `MC`
  width per record on R2010+.
- `string_stream::data_section_end` drops its `+ mc_field_bits`
  correction, which existed only to compensate for the short slice.

### Is a declared-but-unwalked object missing, or absent? (finding, 2026-08-30, #76)

`AcDb:Classes` carries a `num_objects` instance count per custom class
(DXF group 91), which makes it a cheap oracle for walker completeness —
but only if it is a live census. `examples/probe_reference_closure`
decides the question from the bytes with three measurements that never
consult the class table:

1. **Stream tiling** — bytes between two walked records are room an
   unreferenced record could occupy. Zero on every R2004+ corpus file.
2. **Reference closure** — decode every record's handle stream and
   resolve each reference against `AcDb:Handles`. Every *hard*
   reference (§2.13 codes 3 and 5) in the corpus resolves. The only
   unanswered references anywhere are six code-4 **soft** pointers, the
   class the spec permits to dangle: BLOCK_HEADER `0xA0B` of
   `sample_AC1032.dwg` names ten owned entities and six of them
   (`0xD17`, `0xD18`, `0xD40`, `0xD41`, `0xD95`, `0xD96`) have no
   record, in a drawing that leaves 2,841 of the 3,683 handle values in
   `0x1..=0xE63` unused.
3. **Owner census** — a dictionary key is the only way a
   dictionary-owned object is reachable, so the key count bounds the
   population.

Applied to the DICTIONARYVAR shortfall, measurement 3 is decisive:

| File | `AcDbVariableDictionary` keys | DICTIONARYVAR walked | declared |
|---|---|---|---|
| `*_2004.dwg` | 10 | 10 | 16 |
| `*_2007.dwg` | 6 | 6 | 11 |
| `*_2010.dwg` | 5 | 5 | 10 |
| `*_2013.dwg` | 5 | 5 | 5 |
| `sample_AC1032.dwg` | 11 | 11 | 11 |

Keys and walked records agree on every file, including the two whose
class table also agrees. CELLSTYLEMAP says the same thing more sharply:
`arc_2010.dwg` declares one, contains none, and carries no
`ACAD_ROUNDTRIP_2008_TABLESTYLE_CELLSTYLEMAP` dictionary key for one to
hang from — a live census cannot declare an instance with no owner
slot.

The class-table parse is not in doubt: on `arc_2004.dwg` the ten class
records consume 5161 of the declared 5168 bits (7 bits of byte
padding), the class numbers run 500..509 with no gap, and eight of the
ten declared counts match the walk exactly. **So `num_objects` is not a
live census on every release**, and a shortfall on it is evidence to be
explained, not automatically a walker bug.

## 7a. R2007+ split streams (finding, 2026-08-30)

From R2007 (`AC1021`) an object's body is three streams, not one:

```
 +--------------+----------------+----------------+
 | data stream  | string stream  | handle stream  |
 +--------------+----------------+----------------+
                ^                ^
                |                `- trailer ends here
                `- data fields end exactly here
```

Every `TV` (variable text) field a record's field table lists inline is
actually stored in the string stream, in field-table order. A decoder
that reads `TV` from the data cursor gets random bits — the observable
symptoms are `table entry name is not valid UTF-16`, `Bit cursor
exhausted`, or a reserved `11` type code.

### Locating the string stream

The trailer at the end of the data area is, reading backwards from its
end: one *strings-present* bit, a 16-bit size in bits, and — when bit
15 of that size is set — a second 16-bit word supplying the high bits.
`src/string_stream.rs` implements this.

The trailer ends where the handle stream begins:

```
trailer_end = payload_bits - handle_stream_bits
```

| Evidence | Value |
|---|---|
| Objects checked | 59 |
| Object types spanned | 11 (LAYER, LTYPE, STYLE, VIEW, VPORT, APPID, DIMSTYLE, BLOCK_HEADER, TEXT, ATTRIB, ATTDEF) |
| Files | `sample_AC1032.dwg` (R2018), `line_2013.dwg` (R2013), `arc_2010.dwg` (R2010) |
| Deviation from the rule | 0 in all 59 |

Reproduce with `cargo run --release --example probe_string_stream --
<file.dwg> <type-code>`; the probe prints `delta_vs_predicted` per
object.

Until dwg-rs#77 this rule carried a `+ mc_field_bits` correction on the
right-hand side — the width of the record's leading `MC`
handle-stream-size field. Two readings fitted the evidence: either the
record's `MS` byte count excludes that `MC`, or the recorded
handle-stream size counts it. The object stream itself decided the
question (see *A record's `MS` excludes its own `MC`* below), so the
correction moved to where it belongs — the walker's record slice — and
`payload_bits` now spans the whole record. The bit offset is unchanged.

### The self-validating decode

Because the data stream ends exactly where the string stream begins,
every split-stream decoder can check itself: `tables::modern::SplitStream::finish`
rejects the record unless the data cursor lands on the string-stream
start bit. That converts a mis-read field layout from *silently wrong
values* into an error naming the bit offset and the deviation — which
is how the R2007+ VIEW ambient-colour form, the VPORT flag block and
TEXT's `RD` height were each found. It also produced one false finding,
a DIMSTYLE "`AcDs`-binary-data byte", which §7x shows was the table
prefix's `BL num_reactors` read two bits late: a boundary check proves a
field list's width, not which field holds which bits.

### Other findings that fell out of the same work

| Finding | Evidence |
|---|---|
| ~~Shared table-entry fields read `B 64-flag`, `B xdep`, `BS xrefindex+1` — not `B, BS, B`~~ Superseded by §7x: the prefix holds a `BL num_reactors` this reading skipped, and after it two bits remain, read as `B 64-flag, B Xdep` | Only that order keeps the `BS` on a 2-bit `10` code across every STYLE record in `sample_AC1032.dwg`; the alternative yields xref index 83 for `Standard`. The `10` was the 64-flag and Xdep bits |
| VIEW / VPORT / LAYER / DIMSTYLE colours use a full `CMC`: `BS` index + `BL` true colour + `RC` byte, unconditionally | VIEW `view_custom` ambient reads index 0 with no flags yet is followed by `BL = 0xC2333333`; LAYER `0` reads `0xC3000007` |
| `BL` top byte selects the colour form: `0xC3` = ACI index in the low byte, `0xC2` = literal RGB | `Layer_color_80` reads ACI 80; `Layer_true_color` reads `0xC2…` |
| ~~The R2013+ "has AcDs binary data" bit is followed by an `RC`~~ Superseded by §7x: no bits follow the flag; the `RC` was the reactor count | The two DIMSTYLE records in `sample_AC1032.dwg` "with the bit set" need exactly 8 more prefix bits than the four with it clear. They are the two with reactors (8 and 2), whose `BL` is the code `01` and a byte |
| TEXT `height` is a raw `RD`, not a `BD` | With `DataFlags 0xFF`, object #236's remaining 66 bits are `BE` + `BT` + a 64-bit `1.0` at bit 220 |
| LAYER `values` bits: `0x01` frozen, `0x02` off, `0x04` frozen-in-new, `0x08` locked, `0x10` plot, bits 5-9 lineweight index | `sample_AC1032.dwg` names its layers after their state and every one matches |

## 7b. The R2000-R2007 object prologue (finding, 2026-08-30)

Per §19.1 an object record's prologue is version-dependent:

```
 MS   size in bytes                       (all versions; stripped by the walker)
 MC   handle-stream size in bits          R2010+ only
 OT   object type                         all versions
 RL   object DATA size in bits            R2000-R2007 only   <-- "obj_size"
 H    object handle                       all versions
```

The `RL` spans the whole R2000..R2007 band; R2010+ dropped it in
favour of the leading `MC`. The reader originally read it for R2000
only, which left every AC1018 record 32 bits out of phase from the
handle onwards and produced ~20 `Bit cursor exhausted` errors per
R2004 sample.

`obj_size` is the bit at which the object's **data** stream ends and
its handle stream begins — the pre-R2010 analogue of the R2007+
string-stream start bit, and therefore the invariant a decoder can
validate itself against. It is surfaced as `RawObject::obj_size_bits`.

Evidence (`examples/dump_line_payload.rs` on `line_2004.dwg`):

| Reading | handle | preamble | LINE body | end vs `obj_size` |
|---------|--------|----------|-----------|-------------------|
| without `RL` | `0x18395D087600000` (nonsense) | "succeeds" at bit 276 | cursor exhausted | — |
| with `RL` = 347 | `0x83` | bit 84 | `(50,50,0) -> (100,100,0)` | bit 347, delta **0** |

Non-entity objects (symbol-table entries, dictionaries) then carry the
§19.4.2 common object data rather than the §19.4.1 entity preamble:
the EED chain, a `BL` reactor count, the R2004+ xdictionary-missing
flag and the R2013+ DS-binary-data flag. Reading it is what turns
`line_2004.dwg`'s table entries from empty/garbage names into `0`,
`Standard`, `ACAD`, `ByLayer`, `Continuous`, `*Model_Space` — see
`examples/probe_r2004_object_prefix.rs`, which prints every candidate
prefix side by side.

## 7c. Which objects put `TV` in the string stream (audit, 2026-08-30)

`examples/audit_string_streams.rs` walks every object in a file,
locates its string stream and prints the strings it holds. A type
whose records carry strings there but whose decoder reads `TV` from
the data cursor is *silently wrong* — it returns whatever bits sit at
that position and never errors.

On `sample_AC1032.dwg` (R2018), by object type:

| Type | Records | With stream | Strings | Sample |
|---|---|---|---|---|
| TEXT 0x01 | 25 | 25 | 25 | `Hello this is a single line text` |
| ATTRIB 0x02 | 4 | 4 | 9 | `17`, `ATTINFO` |
| ATTDEF 0x03 | 5 | 5 | 16 | `ATTINFO`, `Enter number:` |
| BLOCK 0x04 | 22 | 22 | 22 | `*Model_Space`, `_ArchTick` |
| DIMENSION 0x14-0x1A | 11 | 10 | 11 | (all empty in this file) |
| MTEXT 0x2C | 34 | 33 | 33 | `this is a Mtext\nwith multiple lines in it` |
| TOLERANCE 0x2E | 3 | 3 | 3 | `{\Fgdt;j}%%v{\Fgdt;n}%%v…` |
| HATCH 0x4E | 5 | 4 | 8 | `ANSI31`, `LINEAR` |
| DICTIONARY 0x2A | 65 | 52 | 174 | `ACAD_COLOR` (now decoded — see §7d) |
| LAYOUT 0x52 | 4 | 4 | 20 | `ANSI_A_(8.50_x_11.00_Inches)` |
| MLEADER (class) | 10 | 10 | 10 | `MLeader text, hello!` |
| IMAGEDEF (class) | 1 | 1 | 1 | `.\image.JPG` |
| INSERT 0x07, ENDBLK 0x05, SEQEND 0x06, LINE, CIRCLE, ARC, POINT, LWPOLYLINE, XRECORD 0x4F | — | **0** | 0 | no `TV` fields at all |

Two structural facts fall out:

- **A record with no string stream still has its `TV` slots**, and they
  still consume no data-stream bits. Falling back to an inline read
  there shifts every following field, so `StringReader::empty` exists
  to give those records a reader that yields `""`.
- **BLOCK is inline-correct by accident.** Its name is its only field,
  so the data cursor after the common entity preamble already sits on
  the string-stream start bit and the inline `TV` read lands on the
  string stream. It is not a counter-example to the split layout.

## 7d. The non-entity object path (2026-08-30)

A non-entity object — DICTIONARY, XRECORD, the ten `*_CONTROL` table
owners, ACDB_PLACEHOLDER, ACAD_GROUP, and the custom classes SCALE,
DICTIONARYVAR, IMAGEDEF — is *not* an entity and does not carry the
common entity preamble. Its record is

```text
object header | common object data (§19.4.2) | type-specific fields
```

and `src/objects/modern.rs` is the shared plumbing for it, mirroring
`src/tables/modern.rs` for symbol-table entries. `ObjectStream::read_tv`
takes a `TV` from the object's string stream on R2007+ and inline before
that, so each decoder writes its field list exactly once.

**The common object data keeps its `BL num_reactors` on R2007+.**

```text
EED chain
BL   num_reactors
B    no_xdictionary_handle   -- R2004+
B    has_ds_binary_data      -- R2013+ (no further bits, §7h)
```

Measured three independent ways: DICTIONARY's `BL numitems` decodes to
the number of strings its string stream actually holds only with the
`BL` consumed; ACDB_PLACEHOLDER, whose body is provably empty, closes
exactly; and the nine one-field `*_CONTROL` records close on their
`BL numentries`. `tables/modern.rs` used to skip this `BL` and
compensate with a different flag order; both readings summed to the
same total for an APPID record, so both satisfied the boundary check.
§7x settles it against AutoCAD's DXF: the symbol-table prefix holds the
`BL` too.

**Every object decoder is self-validating.** `ObjectStream::finish`
requires the data fields to end exactly on the record's data-stream
boundary:

| Release band | Boundary |
|---|---|
| R2010+, record with strings | first bit of the string stream |
| R2010+, record without strings | handle-stream start minus the one `B` "strings present" trailer bit |
| R2000-R2007 | the `RL` object-data-size-in-bits from the object prologue |
| R13/R14 | unknown — no check |

A decoder that lands anywhere else returns `DecodedEntity::Error`, so a
wrong field list can never inflate the coverage ratio. That is also why
MATERIAL and PROPERTYSET_DATA — which read only what their bytes prove
and stop — are deliberately **not** dispatched, along with TABLESTYLE,
whose block structure is measured (§7d.4) but whose token sequence a
single record per file cannot pin.

### 7d.1 Deriving a field list the spec does not carry — VISUALSTYLE

The ODA v5.4.1 object-prescription chapter §20.4 stops at XRECORD; it
carries no prescription for VISUALSTYLE or MATERIAL, the two largest
undispatched classes on the corpus. The boundary above is what makes a
field list for them *checkable* anyway, and it turns out to be strong
enough to make one **derivable**: search for a token sequence over
`B / BS / BL / BD / CMC` that lands **every** VISUALSTYLE record of a
file exactly on its own boundary, requiring every `BD` to decode to a
plausible double, every `CMC` true-colour word to carry a real colour
method octet (`0xC0`…`0xC8`), and — once the pattern was visible — every
other slot to be a `BS` flag in `0..=7`.

On `arc_2010.dwg`'s 24 records that search returns exactly **one**
answer: a fixed head (`TV` description, `BS` internal style type,
`BS` 2, `B` internal-use flag) followed by 28 `(value, flag)` pairs. On
`arc_2013.dwg` it returns exactly one answer too — the same 28 pairs
plus 30 more — and that R2013 sequence closes on all 24 records of
`sample_AC1032.dwg` as well. The `(value, flag)` pairing also explains
the corpus's bit budgets: a flag of `1` costs 10 bits and a flag of `0`
costs 2, which is why the records of one file differ only in multiples
of 8, and why the three internal styles that carry `0` in every flag
are exactly the shortest records in the file.

Independent corroboration comes from the decoded values: the lone `B`
of the fixed head is `false` on exactly the ten styles AutoCAD's Visual
Styles Manager lists and `true` on the fourteen it hides; `face_opacity`
is `0.6` everywhere except `X-Ray`, which is `0.5`; `face_mono_color` is
white everywhere except `ColorChange`, which is `0x808080`;
`edge_crease_angle` is `179` on `Conceptual` and `40` on `Hidden`,
`Sketchy` and `Shades of Gray`.

`examples/probe_field_list.rs` is the tool that measures one candidate
list against one record:

```sh
cargo run --release --example probe_field_list -- \
    samples/arc_2010.dwg 42 BS,BS,B,BS,BS,BD,BD
```

### 7d.1a The same object one generation earlier — VISUALSTYLE on R2004

(§7d.1b below shows this same list also covers R14, R2000 and R2007.)

R2004 stores the same 24 styles *without* the per-property flags, with
one fewer property and in a visibly different order, so the R2010 list
misses its `RL` object-data-size boundary by hundreds of bits. Deriving
the earlier list needed two things the R2010 pass did not: an anchor at
each end of the record, and a second file to cross-check values
against.

Both ends were anchored from the record's own redundancy. At the front,
`examples/probe_token_scan.rs` reports the self-identifying tokens —
full-form `BD`s that decode to short decimals, `CMC`s whose true-colour
word carries a real method octet — and the first three of those pin
`face_opacity`, `face_specular` and `face_mono_color`. At the back, a
reverse scan for the *unique* offset at which `BS, BL, BS, B` lands
exactly on the boundary returns one position per record, and its four
values reproduce R2010's `edge_style_apply` (`1` / `5` / `13`),
`display_shadow_type` (`0`) and `is_internal_use_only` — the last of
which splits the 24 styles into the ten AutoCAD's Visual Styles Manager
lists and the fourteen it hides. That is the discovery that the
flag-less generation moved `is_internal_use_only` from the record's
head to its **final bit**, and that `display_brightness` is a `BL`
there where R2010 spends a `BD`: `Dim` decodes `-50`, `Brighten` `50`,
the other 22 records `0`, against R2010's `-50.0` / `50.0` / `0.0`, and
that `BL` is the entire reason `Dim`'s record is 32 bits longer than
its neighbours' and `Brighten`'s 8.

With both ends fixed, the middle closes as `arc_2010.dwg`'s field
values, in `arc_2004.dwg`'s order — 30 fields, delta 0 on all 24
records of each of the three `*_2004.dwg` files. Twelve `BS` slots,
all five `CMC`s and both remaining `BD`s reproduce R2010's decoded
value for the *same style* on all 24 records, including the ones that
vary style by style (`edge_modifier` `0`/`8`/`10`/`11`/`12`,
`edge_silhouette_width` `3`/`5`/`6`, `edge_obscured_linetype` `2` on
`Hidden`, `7` on `Linepattern`). Two slots differ informatively rather
than worryingly: `face_opacity` and `face_specular` are **signed** on
R2004, their magnitudes agreeing with R2010 on all 24 while the sign
tracks whether the property applies to the style.

One 13-bit run — between `edge_silhouette_width` and
`edge_intersection_linetype` — is constant on all 72 records, so its
internal boundaries cannot be measured, only its width. The module
documents that explicitly, names the three properties R2010 places
there (all of which decode zero), and surfaces the run's leading byte
as `edge_unknown_byte` rather than inventing a name for it.

### 7d.1b One field list, four releases — VISUALSTYLE on R14, R2000 and R2007 (2026-08-30, #73)

#110/#104 made the R14 / R2000 / R2007 object walk work, and their
VISUALSTYLE records reached a decoder for the first time. Neither
shipped list closed on any of the 216, and the issue opened on the
obvious reading: a third generation of the layout. The measured answer
is narrower and better. **There is no third generation.** R14, R2000,
R2004 and R2007 write one field list — §7d.1a's flag-less 30 — and the
216 records miss the boundary for two reasons that have nothing to do
with the field order.

**First, the corpus.** Every one of the nineteen files holds exactly 24
VISUALSTYLE records, R14 included, so the built-in styles are not a
2007-era addition and the 216 split 72 / 72 / 72 across R14 / R2000 /
R2007 — not, as the issue allowed for, 108 / 108 / 0.
`examples/probe_visualstyle_layout.rs` with no `--spec` is the census.

**Second, §2.11.** Before R2004 a colour is the bare `BS` index; the
`BL` true-colour word and `RC` colour byte are R2004's addition. Five
colour slots × 40 bits is most of the 154-to-170-bit gap between an
R2000 record and its R2004 counterpart. Read the colours the older way
and the *same* 30 fields land all 144 records of the six R14 / R2000
files exactly on their boundary — delta 0, 144 of 144. The two bands
are bit-identical in shape: the 24 records of an R14 file give the same
budget list as the 24 of the R2000 file built from the same drawing.

The colour change is also where the corroboration gets interesting,
because the two encodings have to agree *through* the change, and they
do, on all 144 records:

| Style / slot | R2004 `CMC` | R14 / R2000 index |
|---|---|---|
| every face mono colour but `ColorChange` | `0xC2FFFFFF` | `7` |
| `ColorChange` face + edge colour | `0xC2808080` | `8` (ACI dark grey) |
| `Shaded` silhouette colour | `0xC2787878` | `8` |
| obscured colour, 22 of 24 styles | `0xC8000000` ("none") | `257` |
| obscured colour, `Shades of Gray` + `Sketchy` | `0xC3000007` | `7` |

`internal_style_type` (`0` `Flat` … `27` `Shaded`), the signed
`face_opacity` (`-0.6`, `+0.5` on `X-Ray`), `edge_crease_angle`
(`40` / `179` / `1`), the `BL display_brightness` (`-50` `Dim`, `50`
`Brighten`, `0` elsewhere) and the final-bit `is_internal_use_only`
ten/fourteen split all agree with R2004 record for record.

**R2007 adds exactly one slot.** It keeps the R2004 colour form, so
with the R2004 list its records land **2 bits short** on all 72 — and
the leftover bits are `10` followed by `is_internal_use_only`, i.e.
`101` on the fourteen hidden styles and `100` on the ten listed ones,
on every record of all three files. One more 2-bit read before that
final `B` closes all 72.

The slot's position is measured, not chosen, and `Dim` is what measures
it: it is the one record whose `display_brightness` is not the 2-bit
zero form but the full 34-bit `BL` holding `-50`, so it discriminates
placements the other 23 cannot. Putting the slot before
`display_brightness` closes 23 of 24 and fails on `Dim`; putting it in
the head fails on all 24; putting it after `is_internal_use_only` fails
on all 24. What is *not* determined is the slot's token type or its
side of `display_shadow_type` — both are two bits reading `0` on all
72 records — so the module surfaces it as `display_unknown_short`
rather than guessing a name, the same way it treats the R2004 13-bit
constant run. It is not R2010's `BS format_version`: that decodes `2`,
this decodes `0`.

**On the limits of a width-only search.** `probe_visualstyle_layout
--search` measures every one-token neighbour of a candidate list
(insert / delete / substitute over `B BS BL BD RC CMC`) against every
record at once. On R2007 it reports 33 neighbours that also close; on
the R14 / R2000 band, 64. Every one is an encoding alias rather than a
rival layout — pre-R2004 a colour *is* a `BS`, and `BS` / `BL` / `BD`
share bit-prefix grammar so they coincide on the small-value forms
these records use. Width evidence alone cannot separate them. The
cross-release value agreement above is what does, and saying so is the
point: the search narrows the field, the values pick the answer.

The net effect on the corpus is 216 records moving from *unhandled* to
*decoded*, VISUALSTYLE leaving the unhandled histogram entirely, and no
change to the nine errors:

| Corpus slice | Before (`6cc9b13`) | After |
|---|---|---|
| R14 (AC1014) × 3 | 426 / 450 / 0 / 48.6 % | **498 / 378 / 0 / 56.8 %** |
| R2000 (AC1015) × 3 | 534 / 99 / 0 / 84.4 % | **606 / 27 / 0 / 95.7 %** |
| R2007 (AC1021) × 3 | 462 / 96 / 0 / 82.8 % | **534 / 24 / 0 / 95.7 %** |
| **Aggregate (19 files)** | **3695 / 741 / 9 / 83.1 %** | **3911 / 525 / 9 / 88.0 %** |

### 7d.2 Checking a field list the spec *does* carry — LAYOUT

VISUALSTYLE had to be derived because §20.4 has no entry for it.
LAYOUT is the opposite case: **§20.4.84** prescribes the whole record,
opening with the plot-settings block (every row glossed
`plotsettings …`) and continuing into LAYOUT's own fields. What the
crate previously carried was neither — `objects::acad_layout` cited a
"§19.6.12 (L6-12)" that does not exist (the spec has no §19.6 chapter)
and listed a field order missing the six margin/paper `BD`s, the
paper-size `TV` and the shade-plot triple. It could not close on any
real record, which is why LAYOUT stayed `Unhandled` on all 31 of them.

Reading the prescription straight and measuring it against the boundary
closes every record on the first attempt, on four release bands at
once — 9 records on the R2004 files (inline `TV`s, `RL` boundary), 9 on
R2010, 9 on R2013 and 4 on `sample_AC1032.dwg` (string-stream `TV`s).
The boundary then earns its keep twice over, by catching the two places
the prescription and the bytes disagree:

- **`viewport_count` is a `BL`, not the `RL` §20.4.84 prints.** On
  `sample_AC1032.dwg` handle 89 the field starts 10 bits before the
  string-stream start: a 32-bit `RL` cannot fit, and a `BL` in its
  8-bit form is exactly 10 bits and decodes `2`. The 28 records with no
  viewport spend 2 bits there, where an `RL` would overrun by 30.
- **The R2013+ `has_ds_binary_data` bit carries no further bits.**
  (Corrected 2026-10-26; see "LAYOUT's plot flags, measured against
  AutoCAD's DXF" below.) This section first read 16 bits after the flag
  on the four LAYOUTs of `sample_AC1032.dwg` that set it, because that
  reading closes. Reading nothing closes too, and only that reading
  gives the plot-layout flags AutoCAD exports.

Because §20.4.84 is the only place the spec prescribes the
plot-settings block, `objects::acad_plot_settings` and
`objects::acad_layout` share one implementation of it: a standalone
PLOTSETTINGS record decodes through the same `read_fields`, so the two
cannot drift apart. The decoded values are what make the ordering
believable rather than merely arithmetically consistent — paper sizes
that match their own name string (`215.9 × 279.4` mm for
`ANSI_A_(8.50_x_11.00_Inches)`, `210 × 297` for `A4`), limits that are
the sheet less its margins in inches, `(1,0,0)` / `(0,1,0)` UCS axes on
all 31 records, and AutoCAD's `±1e20` uninitialised-extents sentinel on
the empty layouts.

### 7d.3 A second prescribed record — MLINESTYLE (§20.4.73)

**§20.4.73 MLINESTYLE** prescribes the whole record: `TV` name, `TV`
description, `BS` flags, `CMC` fill colour, `BD` start and end angles,
`RC` line count, then per line a `BD` offset, a `CMC` colour and —
"Before R2018" — a `BS` linetype index, which R2018 replaces with a
handle. The module previously cited a "§19.6.4 (L6-13)" that does not
exist and read `BS` where the spec prints `CMC` for both colour fields,
which is 42 bits short per record on every real file; the citation is
withdrawn.

Read straight, the prescription closes all ten corpus records:

| Release | Records | Budget | Delta |
|---|---|---|---|
| R2004 | 3 (`{arc,circle,line}_2004.dwg` handle 96) | 526 | 0 |
| R2010 | 3 (`*_2010.dwg` handle 96) | 442 | 0 |
| R2013 | 3 (`*_2013.dwg` handle 96) | 442 | 0 |
| R2018 | 1 (`sample_AC1032.dwg` handle 24) | 406 | 0 |

The R2013 → R2018 drop is exactly the two `BS ltindex` words R2018
moves into the handle stream: both records carry two line elements,
both write the index in the 16-bit `BS` form, and 442 − 2 × 18 = 406.
Every record decodes `startang = endang = π/2`, a ByLayer fill colour,
elements at `+0.5` and `−0.5`, and the `32767` no-linetype sentinel.

### 7d.4 The joint-boundary search — MLEADERSTYLE and the view styles

§20.4 has no entry for MLEADERSTYLE, ACDBDETAILVIEWSTYLE,
ACDBSECTIONVIEWSTYLE or TABLESTYLE. The first three were derived the
way VISUALSTYLE was, with one addition: because the corpus stores the
same style in four release bands, a candidate token sequence has to
close **every** band, and the pre-R2007 band is the strongest
constraint of the four — there a `TV` is inline and costs real bits, so
a string in the wrong slot moves everything after it by hundreds of
bits. `ACDBSECTIONVIEWSTYLE`'s four strings cost 82, 146, 730 and 66
bits on R2004; that is what fixes their positions, and what settles the
order of `hatch_pattern_name` and the byte beside it, which are
interchangeable on R2010+ where a `TV` is free.

`H` fields are invisible to this search on every band the walker
reaches: R2000+ puts the handle references past the object-data-size
boundary, so a handle costs no data-stream bits and leaves no trace.
The field lists in `objects::acad_mleader_style`,
`objects::acad_detail_view_style` and `objects::acad_section_view_style`
are therefore the data stream only.

| Object | Records | R2004 | R2010 | R2013 | R2018 |
|---|---|---|---|---|---|
| MLEADERSTYLE | 11 | 744 | 692 | 693 | 693 / 749 |
| ACDBDETAILVIEWSTYLE | 11 | 1209 | 667 | 667 | 677 / 605 |
| ACDBSECTIONVIEWSTYLE | 11 | 2325 | 1301 | 1301 | 1263 / 1367 |

Every one of those 33 records closes with delta 0. The version switches
are measured: MLEADERSTYLE gains a leading `BS version` and three
trailing attachment-direction words in R2010 and one further `B` in
R2013; both view styles gain a second name `TV` and two `B`s in R2018,
and only placing those two bits *after* the record's leading `BS` keeps
`flags` reading the `32` (detail) / `44` (section) that R2004, R2010 and
R2013 all carry — the other placement turns it into `72` and
desynchronises the first `CMC` on both R2018 records.

Corroboration is in the values. MLEADERSTYLE's `Standard` decodes
AutoCAD's shipped defaults to the last digit — landing gap `0.09`,
dogleg `0.36`, arrowhead `0.18`, text height `0.18`, break gap `0.125`,
block scales `1, 1, 1`, ByBlock (`0xC1……`) for all three colours and
`-2` for the leader lineweight. The view styles decode text heights of
`5` on every `Metric50` record and `0.24` on every `Imperial24` one,
lineweights of `25` and `50` (0.25 mm and 0.50 mm), ByLayer for every
colour but the section hatch background, which is the `0xC8……` "no
colour" method. ACDBSECTIONVIEWSTYLE ends in five consecutive
full-width doubles that decode to 90°, 15°, 75°, −15° and 105° in
radians — the cutting-plane angle set — and carries `I, O, Q, S, X, Z`,
the identifier letters AutoCAD excludes, and `ANSI31` as the cut-surface
pattern.

**TABLESTYLE is declined.** Its R2013 record on `arc_2013.dwg` budgets
6,844 bits and its structure is measurable —
`examples/probe_token_scan.rs` shows a 52-bit header followed by four
cell-style blocks (the string stream names them `Table`, `_TITLE`,
`_HEADER`, `_DATA`) of 1738 / 1696 / 1696 / 1662 bits, each ending in
six 168-bit border sub-records of the shape `CMC` + 36 bits + `BD` + 22
bits — but the blocks' internal token sequence is not determined: each
corpus file carries exactly one TABLESTYLE record, and one record with
a 52-bit header admits thousands of parses over `B/BS/BL/BD/RC/CMC`.
Measured budgets, for whoever picks this up: R2004 1849, R2010 6836,
R2013 6844, R2018 5820. The records stay `Unhandled`.

*Superseded:* §20.4.101 prints TABLESTYLE after all, and its
until-R21 layout closes on every record of R14 to R2007 (§7ad); the
R2010+ record is §20.4.101.2's different layout.

## 7e. `AcDb:Classes` — where the class list actually starts

`Custom(N)` object type codes (≥ 500) resolve through the class table,
so a broken class parser makes every custom object undispatchable. The
list does not start at a fixed offset:

```text
[0..16]   sentinel
[16..20]  RL  size of the class data area, in bytes
[20..24]  RL  (R2010+) unknown, observed 0
[24..28]  RL  (R2010+) size of the class data area, in bits
-- bit stream: byte 20 on R2004, byte 28 on R2010+ --
BL max_class_number, B, then one record per class
```

From R2007 the records themselves split the same way object records do:
every record's non-string fields first, then `3 × N` strings as a block
in record order. `ClassMap::parse` accepts a table only when its class
numbers run consecutively from 500, so a mislocated stream reports no
classes rather than a desynchronised list a dispatcher would trust.

### The class record's width fields are `BL`, not `BS` (2026-08-30, #37)

The spec describes this record twice and the two descriptions disagree:
§10.2 (R18+) gives `BS max_class_number` + `RC 0x00` + `RC 0x00`,
`BS dwg_version` and `BS maintenance_version`; §5.8 (R2007) gives
`BL max_class_number` and `BL` for both version fields. The bytes side
with §5.8, and the readings are bit-identical almost everywhere:

| Tag | `BS` consumes | `BL` consumes |
|---|---|---|
| `10` | 2 bits (value 0) | 2 bits (value 0) |
| `01` | 10 bits (one byte) | 10 bits (one byte) |
| `00` | 18 bits (`RS`) | 34 bits (`RL`) |

`BS max_class_number` + two `RC 0x00` is 34 bits over the same four
little-endian bytes as a tag-`00` `BL`, so the header never
discriminates. The record tail does — but only when a version field
exceeds 255, which no class on `arc_2004` / `arc_2010` / `arc_2013`
does. On `sample_AC1032.dwg` four classes do:

| Class | DXF name | `num_objects` | `dwg_version` | `maintenance_version` | record width |
|---|---|---|---|---|---|
| 508 | `MLEADERSTYLE` | 2 | 33 | 329 | 113 bits |
| 516 | `ACDBDETAILVIEWSTYLE` | 2 | 33 | 329 | 113 bits |
| 520 | `WIPEOUT` | 1 | 33 | 329 | 113 bits |
| 526 | `MULTILEADER` | 15 | 33 | 329 | 113 bits |

Every other record on that file is 57-89 bits wide. Reading the
329 as a `BS` consumes 18 bits instead of 34, so record 8 ran 16 bits
long, record 9 hit a reserved `BL` tag, and the consecutiveness check
threw the whole table away — leaving 194 `Custom(N)` records
undispatchable.

Three independent measurements confirm the corrected field list. With
it, exactly `max_class_number - 500 + 1` records decode with
consecutive class numbers, and the `3 × N` strings that follow end
precisely where the section's §19.4.1 string-stream trailer says they
should:

| File | records | records end | strings end | trailer at `size_in_bits − 32` | trailer length word |
|---|---|---|---|---|---|
| `arc_2010.dwg` | 10 | 877 | 8665 | 8682 | 7788 = 8665 − 877 |
| `arc_2013.dwg` | 9 | 788 | 7826 | 7843 | 7038 = 7826 − 788 |
| `sample_AC1032.dwg` | 50 | 4093 | 49897 | 49930 | 45804 = 49897 − 4093 |

(bit offsets relative to the start of the bit stream). On the
pre-R2007 layout, where the names are inline and there is no string
block, the equivalent check is the declared size: `arc_2004.dwg`'s ten
records consume 5161 of the 5168 bits its header declares, the
remaining 7 being byte padding. A fourth check
is the `num_objects` field the record tail now carries: summed over the
corpus and compared with what `all_objects()` actually walks, it agrees
exactly on the high-count classes — VISUALSTYLE 240 / 240, SCALE
186 / 186, ACDBDETAILVIEWSTYLE 11 / 11, ACDBSECTIONVIEWSTYLE 11 / 11,
MLEADERSTYLE 11 / 11, TABLESTYLE 10 / 10,
BLOCKGRIPLOCATIONCOMPONENT 6 / 6 — a coincidence the mis-aligned
reading could not produce. The classes that disagreed when this was
first measured (DICTIONARYVAR 104 / 72, MULTILEADER 15 / 10,
LAYOUT 4 / 0, CELLSTYLEMAP 18 / 3) have since split two ways: the
MULTILEADER and LAYOUT gaps were genuine walk failures and #43/#44
closed them, while DICTIONARYVAR and CELLSTYLEMAP turned out to be
files that declare instances they do not contain — see §7's
*Is a declared-but-unwalked object missing, or absent?* (#76). Either
way the shortfall was never a class-table one.

Reproduce with `cargo run --release --example probe_class_layout --
samples/sample_AC1032.dwg`, which prints the per-field bit offsets,
the record widths, the resolved names, and the trailer arithmetic.

## 7f. The entity graphics-preview block (finding, 2026-08-30)

The common entity preamble (§19.4.1) opens with the EED chain, then a
single `B` "graphics present" flag. When that flag is set, a size field
and that many bytes of proxy graphics follow — and **the size field
changes width across the release bands**:

| Release band | Graphics size field |
|---|---|
| R13 - R2007 | `RL` (32 bits) |
| R2010+ | `BLL` (§2.4 — a three-bit byte count, then that many LE bytes) |

Read as an `RL` on R2018 the field yields sizes in the millions of
bytes for records a few hundred bytes long, and the byte-skip loop runs
off the end of the record inside the preamble, before any
entity-specific field.

**Custom classes were a correlation, not a cause.** AutoCAD writes
proxy graphics for entities whose class it does not implement natively,
so the flag is set on MULTILEADER / MESH / ACAD_TABLE / WIPEOUT / IMAGE
and clear on LINE / TEXT / MTEXT. The preamble is byte-for-byte the same
field list either way; the graphics branch simply had no coverage until
the R2018 class table started resolving (#41) and those records began
reaching a decoder.

**The `BLL` byte count is three raw bits.** The crate previously read
the `3B` code as a stop-at-zero prefix, which can only produce
`{0, 2, 6, 7}` — a set with no encoding for "one byte", which the file
requires. Measured on `sample_AC1032.dwg`:

| record | prefix bits | size bytes | preamble ends at bit |
|---|---|---|---|
| IMAGE `0x662` | `001` + `8C` | 140 | 1213 |
| WIPEOUT `0x44D` | `001` + `8C` | 140 | 1213 |
| MULTILEADER `0x66E` | `010` + `50 03` | 848 | 6885 |
| MULTILEADER `0x818` | `010` + `98 02` | 664 | 5413 |
| MESH `0x343` | `010` + `28 25` | 9512 | 76197 |
| MESH `0x380` | `010` + `08 24` | 9224 | 73893 |

The check that this is right is what follows the block: every one of
those records then reads `entmode = 2`, `num_reactors = 0`,
`no_xdictionary = true`, colour `0x0100`, linetype scale `1.0`, all
flag fields `0`, `invisibility = 0`, `lineweight = 0x1D` — bit-for-bit
the values every plain LINE / TEXT / MTEXT record in the same file
carries. `examples/probe_entity_preamble.rs` prints both readings side
by side for any record.

Two more things fall out of the same two records. The IMAGE and the
WIPEOUT are **bit-identical for their first 175 bits**, which is how we
know `AcDbWipeout` stores a raster-image record verbatim; and an IMAGE
clip boundary of type 1 (rectangle) carries **no vertex count** — the
two corners follow the boundary type directly, while the polygon form
(type 2) does carry the count.

## 7g. The last 39 R2018 errors (finding, 2026-08-30)

After the handle-map fix (#53) and the LAYOUT field list (#52), 39
records of `sample_AC1032.dwg` still errored and no other file in the
corpus errored at all. All 39 are now decoded. Six findings did the
work; each is stated with the evidence that fixes it, and each decoder
now closes **exactly** on the record's data-stream boundary.

### The `strings present` trailer bit is not an entity field

A record with no string stream still writes the one-bit trailer flag
that says so. It is the last bit before the handle stream, and it is
not one of the record's own fields, so the data fields of such a record
end **one bit before** `string_stream::data_section_end`.

Every LWPOLYLINE (20 records), 3DFACE (1), INSERT (4) and SPLINE (2) of
`sample_AC1032.dwg` carries no strings, and every one of them ended its
field list exactly one bit short of the old boundary. Modelling that as
a trailing `B` would have put a fabricated field at the end of four
different field lists. `string_stream::data_field_end` returns the
string-stream start bit when there is one and `data_section_end - 1`
when there is not; `object::data_end_bit` and
`tables::modern::open_entity` both route through it.

### Object references consume no data-stream bits

The same rule that moves `TV` into the string stream moves every `H`
into the handle stream. MULTILEADER read three handles inline, HATCH
read its per-path boundary handles inline, UNDERLAY read its definition
handle inline — each one shifted every field after it. §20.4.75 is
explicit for HATCH: the boundary handles are "Common Entity Handle
Data", after the data stream, so the data stream carries only the count.

### MULTILEADER embeds a whole `MLeaderAnnotContext`

§20.4.48 says so directly, and §20.4.86 gives the embedded field list:
leader roots, their leader lines, the vertices, and the text or block
content. The inherited `AcDbAnnotScaleObjectContextData` /
`AcDbObjectContextData` prefix (§20.4.71 / §20.4.89) is **not** present
when embedded — the `BL` leader-root count follows the `BS 270` version
directly. The `-R2007` arrowhead / block-label block is genuinely
absent on R2018; in R2010+ the per-arrowhead data moved into the leader
line (`H 341` arrow symbol per line).

All 15 MULTILEADER records close on their string-stream start bit under
that reading, and the text they recover from the string stream is the
authored text: `"MLeader text, hello!"`, `"MULTILEADER TEST"`,
`"MULTILEADER\PTEST\P123"`.

**Not established:** 17 bits sit between `B 293 is annotative` and the
R2010+ `BS 271` that §20.4.48 does not list — an `MC` (two bytes on all
15 records) then a `B`. The `MC` holds only `274`, `530` and `786`
across the file and the `B` is `true` on every record. `B` + `RS`, or
`B` + two `RC`s, consume the same bits; `MC` is taken because it is the
only one that stays correct for a larger value. The fields are surfaced
as `MLeader::undocumented_mc` / `undocumented_flag` rather than named.

### The HATCH gradient block is unconditional

§20.4.75 lists `Is Gradient Fill` as a `BL` (not a `BS`) and does not
guard the rest of the gradient record behind it. All eight HATCH
records of `sample_AC1032.dwg` carry **two** strings — `"LINEAR"` then
the pattern name (`"ANSI31"`, `"AR-PARQ1"`, `"HVEGE100"`, …) — even
though six have the flag clear. Returning early on a clear flag
consumed one `TV` too few and shifted the whole boundary-path tree.

Four more HATCH corrections came from the same section: path points and
seed points are `2RD` (raw doubles), not `BD` pairs; a `BS pattern type`
follows `BS style`; the whole pattern-definition block is written only
`if (!solidfill)`; and the pixel size is a `BD` written only when some
path flag has bit `4` set.

### Field types the previous readings had wrong

| Entity | Was | Is (§) | How it shows |
|---|---|---|---|
| INSERT | `BD` scales, `BE` extrusion, no owned count | `BB` data flags selecting `RD`/`DD` forms, `3BD` extrusion, `BL` owned count only when `has_attribs` (§20.4.9) | 3 of 4 records overran; the fourth mis-scaled |
| SPLINE | `BD` degree, branch on the stored `Scenario` | `BL` degree; on R2013+ the branch is derived — scenario is 1 if the knot parameter is Custom or there is no fit data, else 2 (§20.4.40) | both records store `1`, yet `0x434` is a fit spline |
| LWPOLYLINE | `0x01` elevation … `0x20` const width, width read last | `0x001` normal, `0x002` thickness, `0x004` const width (read first), `0x008` elevation, `0x010` bulges, `0x020` widths, `0x400` vertex ids (§20.4.85) | 19 of 20 records have flag `0` or `0x200` and survive either reading; `0x4A6` has flag `4` and does not |
| 3DFACE | `BD` deltas added to the previous corner, corner 4 dropped when `has_no_flag_ind` | `3DD` triples defaulting to the previous corner; only the invisible-edge `BS` is conditional (§20.4.32) | reserved `11` bit pattern |
| IMAGEDEF | `BD` image size and pixel size | `2RD` for both (§20.4.81) | reserved `11` bit pattern |
| DIMENSION (ang 2-line) | 16-point read after the four `3BD`s | `2RD` 16-point read **first** (§20.4.27) | reserved `11` bit pattern |
| LTYPE dash | `BD`, `BS`, `BD`, `BD`, `BD`, `BD`, `BS` | `BD` length, `BS` shape code, two `RD` offsets, `BD` scale, `BD` rotation, `BS` shape flag; the 512-byte text area only when some `shapeflag & 0x02` (§20.4.58) | cursor exhausted 6 bits from the end |
| UNDERLAY | insertion, scale, rotation, normal | normal, insertion, rotation, scale — measured, the spec has no UNDERLAY section | old order put the scale on `(0,0,1)` and the normal on `(1,1,1)` |

### MTEXT's R2018 tail, and ATTRIB's embedded MTEXT

§20.4.46 gives MTEXT an R2018+ block after the background flags: a `B`
"is NOT annotative" and, when set, a version, a default flag, a handle
slot, and a redundant copy of the record's own attachment point, axes,
insertion point, rectangle and extents, then the column data. That is
the 567 undecoded bits of #29 — they repeat the record's own values
because the spec says they are a redundant copy. MTEXT now closes
exactly instead of asserting the weaker `<= string_start`.

§20.4.4 gives ATTRIB an R2010+ `RC` version and an R2018+ `RC`
attribute type, and a multi-line attribute (`type != 1`) then embeds a
whole MTEXT record "starting from the Entmode (entity mode)" — no
length, no type code, no handle, no EED chain, no graphics block.
`sample_AC1032.dwg` handle `0x79D` spends 683 data bits on that
embedded record and carries three strings: the (empty) TEXT value, the
MTEXT text `"my multi line text for the attrrib"`, and the tag
`"MULTI_LINE_ATT"`. ATTDEF is "Common ATTRIB Entity Data" plus its own
`RC` version and a `TV` prompt, which is why the old field list — one
unexplained `RC` after the lock bit — closed on the four single-line
ATTDEFs and failed on the multi-line one: three bytes cancelled to one.

### Measured effect

| Corpus slice | Before (`fa7fcc8`) | After |
|---|---|---|
| R2004 (AC1018) × 3 | 498 / 99 / 0 / 83.4 % | 498 / 99 / 0 / 83.4 % |
| R2010 (AC1024) × 3 | 519 / 24 / 0 / 95.6 % | 519 / 24 / 0 / 95.6 % |
| R2013 (AC1027) × 3 | 372 / 24 / 0 / 93.9 % | 372 / 24 / 0 / 93.9 % |
| R2018 (AC1032) × 1 | 716 / 87 / 39 / 85.0 % | **755 / 87 / 0 / 89.7 %** |
| **Aggregate** | **2105 / 234 / 39 / 88.5 %** | **2144 / 234 / 0 / 90.2 %** |

`examples/probe_decode_errors.rs` lists every erroring record with its
handle, body-start bit and data boundary;
`examples/probe_entity_field_list.rs` walks a candidate token list
against one entity record and prints the delta from that boundary.

## 7h. The ACIS entities, and what settles the AcDs marker (2026-08-30, #61 / #54)

`REGION` (37), `3DSOLID` (38) and `BODY` (39) are one entry in the
specification — §20.4.41 prescribes a single record shape for all three
— and until this work the crate decoded only the first bit of it. The
three records were `Unhandled`, and because they were, they could not
answer the one question the corpus had been holding open since #52.

### What the corpus actually holds

Across all 19 files there are exactly **three** §20.4.41 records, all in
`sample_AC1032.dwg` (R2018): 3DSOLID `0xD65`, REGION `0xD69`, 3DSOLID
`0xD6A`. All three set the R2013+ `has AcDs binary data` bit of the
common entity data. There is no ACIS record anywhere in the corpus that
*clears* it, and none outside R2018.

That bit is not decoration. §24.2.2.3 says it plainly: "For each ACIS
entity (REGION, 3DSOLID), a data record is created with the SAB stream
of the object." From R2013 the geometry payload leaves the entity
record and becomes a data record in `AcDb:AcDsPrototype_1b`, keyed by
handle. The entity keeps everything *except* the geometry.

### The field list

Measured from bit 82 — where the common entity preamble ends on all
three records — to each record's data-stream boundary:

```text
B     wireframe data present          1
B     point present                   1
3BD   point
BL    num isolines                    4
B     isolines present                1
BL    num wires                       0
BL    num silhouettes                 0
B     ACIS empty (2)                  1     -- §20.4.41 "Normally 1"
BL    unknown (R2007+)                0
--- R2013+ data-store block, measured ---
B     unknown_a                       1
BB    unknown_b                       0
BB    unknown_c
BB    unknown_d
RC×16 revision GUID
BL    unknown_e                       0
```

The inline ACIS envelope (`B ACIS empty`, `B unknown`, `BS version`,
the block loop) is **absent**: only two bits stand between the preamble
and the point, and both are 1.

| record | data ends | point | isolines | revision GUID | delta |
|---|---|---|---|---|---|
| 3DSOLID `0xD65` | bit 437 | `(17.7767…, -220.8501…, 2.5)` | 4 | `833111a1-b7ac-4dd4-824d-78b33668f9e7` | **0** |
| REGION `0xD69` | bit 373 | `(24.9857…, -220.4000…, 0)` | 4 | `2b80e3b3-b594-475e-8593-6a36b15e7945` | **0** |
| 3DSOLID `0xD6A` | bit 437 | `(31.6857…, -220.1073…, 2.1902…)` | 4 | `f21ff2a0-ff9c-4ed1-9b2e-7a1e6518d595` | **0** |

Four things corroborate the list independently of the arithmetic:

1. **The points are drawing geometry.** Three bodies in a row at
   `y ≈ -220`, `x` stepping `17.8 → 25.0 → 31.7`. A list off by one bit
   produces `1e+88`-scale doubles, which is what every rejected
   candidate produced.
2. **`num isolines` is 4 on all three** — AutoCAD's default `ISOLINES`
   system-variable value.
3. **The 16 bytes are a valid RFC-4122 version-4 UUID on all three**
   (version nibble `4`, variant bits `10`). Scanning every 128-bit
   window of all three records, `data_end - 130` is the *only*
   alignment at which all three satisfy both constraints; three
   independent windows doing so by chance is 1 in 2^18.
4. **Everything else is the spec's own grammar**, decoding the values
   §20.4.41 says to expect (`ACIS empty (2)` "normally 1", empty wire
   and silhouette lists, the R2007+ `BL` as 0).

Only the *width* of the seven bits before the GUID is measured; the
three records agree on `1, 0` for the first two slots and differ after,
so `B, BB, BB, BB` is a labelling, not a measurement. The module says
so.

### The AcDs marker is a flag, and nothing follows it

#52 found that four LAYOUT records of the same file need **16** further
bits after the `has AcDs binary data` flag, and `objects/modern.rs`
consumed them there. `tables/modern.rs` consumed an `RC` (8 bits) on
DIMSTYLE evidence. `common_entity.rs` consumed nothing — untested,
because no entity record whose field list closed had ever set the bit.
Three readings of one field: issue #54.

The three ACIS records arbitrate it, because their field list closes.
`examples/probe_acis_records.rs` re-reads the preamble at each candidate
width and runs §20.4.41 from wherever it lands:

| marker width | preamble ends | preamble values | §20.4.41 list |
|---|---|---|---|
| **0 bits** | bit 82 | `colour 0x0100, lts 1.0, invis 0, lw 0x1D` | **delta 0** |
| 8 bits | — | preamble does not read (reserved `BD` pattern `11`) | — |
| 16 bits | bit 148-212 | `colour 0xEE10, lts 0` (or `1.5e119`), `lw 0x10/0x12/0xBC` | does not close |

Both halves fail together for every non-zero width. The preamble values
at 0 bits are exactly the ones every other entity in the file carries —
BYLAYER colour `0x0100`, linetype scale `1.0`, invisibility `0`,
lineweight `0x1D` — and at 16 bits they are none of those.

So the flag is a flag. The 16 bits LAYOUT needs are LAYOUT's own, and
they moved to `src/objects/acad_layout.rs` as a two-byte data-store
block at the head of its §20.4.84 list, still conditional on the flag
and still closing all 31 corpus LAYOUT records. `tables/modern.rs`
kept its `RC` at the time: that path also omitted the `BL num_reactors`
`objects/modern.rs` reads, so its bit accounting had a second unresolved
variable. §7x resolved both: the `RC` was that `BL`. Three record types needing three
different widths is itself the argument that the width belongs to the
record, not to the flag.

### Two spec-conformance fixes that fell out

The envelope reader had been carrying two departures from §20.4.41 that
no corpus record had ever reached:

- the `B unknown` bit between `ACIS empty` and `BS version` was not
  read at all;
- the block loop read a `B has_more_blocks` flag before each `BL block
  size`, where the spec has the loop terminate on a block size of `0`
  and no flag.

Both are corrected, and version 2 — "immediately following will be an
acis file … no length is given" — is now refused rather than
mis-stepped.

### Measured effect

| Corpus slice | Before (`7418d25`) | After |
|---|---|---|
| R2004 (AC1018) × 3 | 510 / 87 / 0 / 85.4 % | 510 / 87 / 0 / 85.4 % |
| R2010 (AC1024) × 3 | 531 / 12 / 0 / 97.8 % | 531 / 12 / 0 / 97.8 % |
| R2013 (AC1027) × 3 | 384 / 12 / 0 / 97.0 % | 384 / 12 / 0 / 97.0 % |
| R2018 (AC1032) × 1 | 762 / 80 / 0 / 90.5 % | **765 / 77 / 0 / 90.9 %** |
| **Aggregate** | **2187 / 191 / 0 / 92.0 %** | **2190 / 188 / 0 / 92.1 %** |

`examples/probe_acis_records.rs` reproduces the census, the field list
and the arbitration table from the bytes.

## 7i. The exact boundary, everywhere (2026-08-30, #63)

§7a introduced the self-validating decode for the records that had to
have it — the ones whose `TV` fields moved into the string stream. §7g
tightened those from `<=` to `==`. Everything else still decoded with
**no boundary check at all**: the POLYLINE family, MESH, IMAGE,
WIPEOUT, VIEWPORT, LEADER, MLINE, the swept / lofted / extruded /
revolved surfaces, RAY, XLINE, POINT, CIRCLE, ARC, LINE, ELLIPSE,
SOLID, TRACE, ENDBLK, BLOCK — and, on the R2000/R2004 band, *every*
entity type. Their zero error count was therefore a property of the
dispatcher, not of the bytes.

### One check, three boundary sources

`entities::dispatch::checked_inline` is now the single path every
fixed-code and custom-class entity decoder runs through. It asks
`entity_data_end` where the record says its data fields stop:

| Band | Boundary | Source |
|---|---|---|
| R2010 / R2013 / R2018 | string-stream start bit, or handle-stream start − 1 when the record carries no strings | `string_stream::data_field_end` (§7a) |
| R2000 / R2004 | `RL` object-data-size-in-bits from the object prologue | `RawObject::obj_size_bits` (§7b) |
| R2007 | none | the `RL` spans data **and** strings, and this crate cannot locate an AC1021 string stream yet (#110) |
| R13 / R14 | none | the prologue has no size field |

The same call also hands the decoder the record's string reader, which
is what let BLOCK stop reading its name inline. Nothing about the check
is version-specific beyond that table: a decoder writes one field list
and is held to whichever boundary its release states.

### What the check found

Turning it on produced 55 errors across the corpus in five types, and
a sixth — MLINE — surfaced at the same time because #63 also wired it
into the dispatcher for the first time. Three of the six were cheap and
are fixed; three are not and are reported.

| Type | Records | Delta before | Cause | Outcome |
|---|---|---|---|---|
| BLOCK | 45 (R2010/R2013/R2018) | +58 … +266 | the one `TV` was read inline; on R2010+ it lives in the string stream and the record's field budget is **0 bits** on all 33 R2010+ records measured | fixed — names now decode as `*Model_Space`, `_ArchTick`, `MyBlock`, matching the BLOCK_RECORD entries they pair with |
| POLYLINE_PFACE | 1 (R2018) | +52 | five `BS` counts and two inline `H` handles, where the record holds `BS`, `BS`, `BL` in 30 bits | fixed — reads 5 vertices, 2 faces, 7 owned objects |
| MLINE | 3 (R2018) | n/a (counts decoded as `521` / `-11716`) | `num_lines` read as `BS` where the record writes one `RC` | fixed — 2 lines on all three, `delta 0` |
| VIEWPORT | 6 (R2018) | −819 | the decoder reads 266 of 1125 bits by design | reported; closed 2026-10-26 by the whole §20.4.38 list (§7u) |
| MESH | 2 (R2018) | −2 | two trailing bits, `10` on both records | reported; closed 2026-10-26 by the DXF twin (§7w) |
| LEADER | 1 (R2018) | −12 | field list stops before the text-box block | reported; closed 2026-10-26 by the whole §20.4.47 list (§7ab) |

The three fixes each meet the same bar as #58: `delta 0` **plus** a
value that corroborates it independently of the bit count. BLOCK's
names match the symbol table; POLYLINE_PFACE's counts match the object
stream (below); MLINE's `num_lines = 2` on every record is what a
two-element multiline style produces, where the `BS` reading produced a
negative vertex count.

The three reports each meet the other bar: documented offsets, and
stop. VIEWPORT's six records are six copies of one shape, so there is
no variation to separate candidate token sequences with. MESH's two
bits are `10`, which is the zero/default code of `BS`, `BL` and `BD`
alike — a zero corroborates nothing. LEADER's twelve bits admit several
continuations of §19.4.19's list and there is one record.

### The POLYLINE family reads itself back

The check made five previously-`Unhandled` types decodable, and the
object stream cross-checks them without reference to any bit count.
Handles `0x422`..`0x431` of `sample_AC1032.dwg` are contiguous:

```text
0x422  POLYLINE_PFACE   BS 5, BS 2, BL 7          (30 bits, delta 0)
0x423  VERTEX_PFACE     RC 0xC0, 3BD              (142 bits)   ┐
 ...                                                            │ 5 vertices
0x427  VERTEX_PFACE                                            ┘
0x428  VERTEX_PFACE_FACE  BS [ 1, 2, 3, -4]       (48 bits)    ┐ 2 faces
0x429  VERTEX_PFACE_FACE  BS [-1, 4, 5,  0]       (40 bits)    ┘
0x42A  SEQEND           (no fields at all — 0 bits)
0x42B  POLYLINE_3D      RC 0, RC 0, BL 5          (26 bits, delta 0)
0x42C  VERTEX_3D        RC 0x20, 3BD              (142/206 bits) ┐
 ...                                                             │ 5 vertices
0x430  VERTEX_3D                                                ┘
0x431  SEQEND           (0 bits)
```

Every declared count matches the records that follow it: 5 + 2 = the
`BL 7` owned-object count, and 5 = POLYLINE_3D's. The face indices are
all inside the 1..=5 range the mesh declares, with the negative-index
invisible-edge convention and a `0` terminating the three-corner face.
The flag bytes name themselves — `0x20` is the flag table's "3D
polyline vertex" bit, `0xC0` its "polyface mesh vertex" and "3D polygon
mesh vertex" bits together. None of that comes from the bit budget.

`BS` and `BL` are indistinguishable for a value below 256 (both spend
`01` plus one byte), so the owned-object counts are read as the `BL`
`tables::block_record` already uses for the same purpose; only the
width and the value are claimed.

### Three readings that stay undetermined

Gathered here because they are the crate's whole set of "the boundary
closes, but the bits admit more than one grammar" cases. None is
changed by this work; each waits on a second file.

| Reading | Where | What is undetermined | What would settle it |
|---|---|---|---|
| MULTILEADER's 17 bits | §7g above, `entities/mleader.rs` | 17 bits between `B 293 is annotative` and the R2010+ `BS 271` that §20.4.48 does not list. Read as `MC` + `B`; `B` + `RS` and `B` + two `RC`s consume the same bits. The `MC` holds only `274`, `530`, `786` across 15 records and the `B` is `true` on all of them | one record whose value exceeds `RS` range, or whose `B` is false |
| UNDERLAY clip-vertex encoding | `entities/underlay.rs` | whether a clip vertex is `2BD` or `2RD`. The single corpus record has a zero clip-vertex count, so no vertex is ever read | any UNDERLAY with a clip boundary |
| LWPOLYLINE `0x200` | §7g table, `entities/lwpolyline.rs` | `0x200` is not one of §20.4.85's presence bits, and every record carrying it also carries no optional field, so it survives either reading. Treated as "closed" | a record with `0x200` set *and* an optional field present |
| ~~MESH's trailing `10`~~ | `entities/mesh.rs` | settled 2026-10-26 (§7w): the DXF twin writes a group `90` (`BL`) equal to 0 there | — |

### Measured effect

| Corpus slice | Before (`7b2afe2`) | After |
|---|---|---|
| R2004 (AC1018) × 3 | 582 / 15 / 0 / 97.5 % | 582 / 15 / 0 / 97.5 % |
| R2010 (AC1024) × 3 | 531 / 12 / 0 / 97.8 % | 531 / 12 / 0 / 97.8 % |
| R2013 (AC1027) × 3 | 384 / 12 / 0 / 97.0 % | 384 / 12 / 0 / 97.0 % |
| R2018 (AC1032) × 1 | 765 / 77 / 0 / 90.9 % | **776 / 57 / 9 / 92.2 %** |
| **Aggregate** | **2262 / 116 / 0 / 95.1 %** | **2273 / 96 / 9 / 95.6 %** |

(Both columns cover the ten files that were walkable at the time. §7j
adds the other nine.)

The R2004, R2010 and R2013 slices are unchanged in count — but every
one of their entity records is now checked where none was before, and
all of them pass. `examples/probe_entity_budgets.rs` prints the budget
each record's field list has to fill; `examples/probe_decode_errors.rs`
prints the nine that do not fill it.

## 7j. The three containers that had no walk (2026-08-30, #104 / #110 / #65)

Nine of the nineteen corpus files — `{line,arc,circle}_{R14,2000,2007}.dwg`
— reported `n/a (no-handle-map)` in every coverage run this document has
ever quoted. Not "decoded badly": *not walked at all*. Three whole
release bands, and with them the pre-2004 half of every field list in
the crate, had no evidence behind them. This section is what it took to
open them.

### R13-R15: the file *is* the object stream

§3.2.6 gives R13/R14/R2000 a flat list of `(record number, absolute
seeker, size)` locators instead of a page map, and §3.1 puts the object
records loose in the file between the class definitions and the object
map. There is no `AcDb:AcDbObjects` *section*: the object map's offsets
are **absolute file offsets**.

So the walk needed no new machinery at all. `DwgFile::object_stream`
hands back the whole file for these releases, the §4.4 handle-map parser
runs unchanged on the locator's byte range, and
`ObjectWalker::with_handle_map` seeks to each offset exactly as it does
on R2018. The verification is the walker's own oracle: every record
repeats its handle, and on all six R14/R2000 files **every** entry of
the map — 292 per R14 file, 211 per R2000 file — lands on a record whose
handle field matches. Zero mismatches, zero skips.

The locators are given the canonical `AcDb:` names (`AcDb:Header`,
`AcDb:Classes`, `AcDb:Handles`, `AcDb:Template`) so that
`handle_map()`, `class_map()` and `header_vars()` are version-agnostic
one level up.

### R13/R14 put the object size somewhere else

§20.1 and §20.4.1 each list `RL — size of object in bits, not including
end handles` **twice**: once under "R2000+" between the object type and
the object handle, and once under "R13-R14" after the EED chain (objects)
or after the graphics-present flag (entities). The two placements are
mutually exclusive.

The walker settles the first half by construction: it reads the handle
straight after the type code on R13/R14, and 876 of 876 handles across
the three R14 files then match the map. The second half is measured —
`examples/probe_r13_r14_prefix.rs` reads the candidate `RL` at the
post-EED position for all 285 non-entity records of each R14 file and
reports that every one of them is `> 0`, no larger than the record's
payload, and past the cursor that produced it; 135 of them then have a
field list that closes on it exactly.

That field is the R14 band's data-stream boundary, which is why
`checked_inline` and `objects::modern::open` both take it from the
prefix they have just read rather than from `RawObject::obj_size_bits`.

### R2007: four mechanisms, none of them the R2004 ones

The crate previously carried a module describing R2007 as "two-layer
Sec_Mask" — a byte XOR plus a 7-byte bit rotation. **R2007 uses neither.**
§5.1-§5.4 describe a container that shares only vocabulary with §4:

| layer | R2004 / R2010+ | R2007 |
|---|---|---|
| file header | 0x6C bytes at 0x80, XOR against the magic sequence | 0x400 bytes at 0x80, Reed-Solomon (255, 239) + §5.10 LZ |
| page header | 32-byte `Sec_Mask`-XOR'd header per page | none — pages are bare |
| compression | LZ77 §4.7 | a different LZ variant, §5.10 |
| system pages | plain | RS (255, 239) interleaved, the data repeated `factor` times |
| data pages | plain | RS (255, 251) interleaved when the section's `encoding` is 4 |

Because a valid file needs no error *correction*, only the interleaving
matters: gather codeword `j`'s bytes from positions `blocks·i + j` and
keep the first `message` of them. The block count is not `page ÷ 255` —
it is `ceil(factor · align8(stored) / 239)`, read backwards out of
§5.3's construction. Getting that wrong is silent: the page-map page
happens to have the same count either way, and only the section map
(15 blocks in a 18-block-wide page) exposes the difference.

**The §5.10 literal copy is a permutation.** The spec prints a table of
"sub byte blocks" per copy length and one sentence — "the order of bytes
in source and target buffer are different" — that is easy to read as an
implementation note about a hand-optimised `memcpy`. It is not. The
blocks are the smaller copy functions applied *recursively*, so a
2-byte block comes out reversed, a 16-byte block comes out with its
halves swapped, and an 11-byte block (`2 [9], 8 [1], 1 [0]`) comes out
`src[10], src[9], src[1..9], src[0]`.

Two independent measurements pin it, and both are load-bearing:

| where | source bytes | straight copy | permuted copy |
|---|---|---|---|
| file header, first literal | `00 70` | `00 70` → header size 0x7000 | `70 00` → **header size 0x70**, the value §5.2 names |
| section map, an 11-byte literal | inside a section name | `AcDb:Ap\0pInfoHistory` — invalid UTF-16 | `AcDb:AppInfoHistory` |

With the permutation right, the decoded file header reproduces **every**
constant §5.2 documents on all three corpus files — `0x70`, `0x20`,
`0x40`, `0xf800`, `4`, `1`, `0x60100` — and its `File size` field equals
the file's own byte count (66304 / 66304 / 66560). The section map then
yields thirteen sections whose hash codes match the §5.2 table for all
twelve the table lists; the thirteenth is `AcDb:AppInfoHistory`, which
AutoCAD writes and ODA does not.

`examples/probe_r2007_container.rs` prints all of it with a
`ok` / `MISMATCH` verdict per constant.

### R2007's split-stream boundary (#65)

R2010+ locate the string-stream trailer from the leading `MC`
handle-stream size (§7a). R2007 has no `MC` — and §19.1's `RL`
object-data-size, which R2000 and R2004 use as their data-field end,
is on R2007 the end of the data **and** string area, i.e. exactly the
`endbit` the §19.1 trailer prose is written around.

Running the trailer backwards from it produces a string stream whose
first `TU` is the record's own name on every named record of every
R2007 corpus file: `0`, `Standard`, `ACAD`, `*Active`, `ISO-25`,
`Annotative`, `ByBlock`, `ByLayer`, `Continuous`, `AcadAnnotative`,
`*Model_Space`, `*Paper_Space`, `ACAD_NAV_VCDISPLAY`. That is
`string_stream::data_section_end`'s R2007 arm, and it is what lets every
R2007+ decoder in the crate — the whole `tables::modern` /
`objects::modern` family — run on AC1021 without a line of new decoder
code.

### What the three bands then said about the pre-2004 field lists

Opening them turned four decoders that had never seen a real record
into decoders that had to close on a boundary. Each was wrong, and each
is wrong on R2000 and R2004 too — these are fixes to the *existing*
bands that only the new files could surface:

| decoder | was | §20.4.x says | delta before |
|---|---|---|---|
| LAYER (inline) | `BS values`, `B plot`, `BS lineweight`, `BS colour` | `BS values` (which *contains* plot + lineweight), then `CMC` | overshoot; `line_2004.dwg` reported lineweight −32765, colour −31231 |
| LTYPE (inline) | `RC flags` + `RS used_count` before the description | neither field exists | description three bytes late — `lid line` for `Solid line` on every pre-R2007 file |
| BLOCK_HEADER (inline) | stopped after the xref path | R2000+ add an insert-count run, a description `TV` and a preview blob | −12 bits on all six R2000/R2004 records |
| DIMSTYLE (inline) | 15 of ~70 fields | the whole §20.4.68 R2000+ body | −298 to −440 bits |

The corroboration is cross-version: the same drawing exists in seven
releases, so `LAYER "0"` must decode to colour 7 in all of them, `VPORT
"*Active"` to view height 288.065353, and `DIMSTYLE "ISO-25"` to the
published ISO-25 defaults. It now does, from R14 through R2018.

## 7k. The specification's own worked examples (2026-10-26, #107)

The ODA specification prints worked examples and full tables, and every
one now runs through the crate's public code:
`examples/spec_examples.rs`, on the bytes in
`tests/fixtures/spec-examples/`, in CI's Test job. Each fixture quotes
the v5.4.1 PDF (sha256 `973628f3…f504b`) verbatim and names its pages;
`tools/spec/extract_examples.py` regenerates the long ones from the PDF
and audits every one against it. A dump is only accepted when its row
addresses are contiguous, its hex agrees with its printed bit column,
its `MS` equals the length in its header line and its printed CRC is
the CRC-8 of its bytes, so a transcription error cannot pass as data.

```sh
cargo run --release --example spec_examples
```

| Spec | Examples | Crate path | Reproduced |
|---|---|---|---|
| §2.2, §2.3, §2.6 (three), §2.7, §2.13, §2.14.1 | 8 bit streams with their parsed values | `BitCursor` | 8 / 8, each consuming exactly the printed stream |
| §2.14.1, §2.14.2 | CRC-8 and CRC-32 tables, 256 values each | `crc::crc8`, `crc::crc32` | both |
| §3.2.6 | the four locator-CRC XOR values | `crc::r13_locator_seed` | 4 / 4 |
| §4.1 | the header magic sequence | `cipher::magic_sequence` | the 92 printed bytes |
| §3.2.6, §9, §10.1 | five 16-byte sentinels | the reader's and the streaming grammar's constants | 7 / 7 constants |
| §4.7 | two literal lengths, two long compression offsets | `lz77::decompress` and `streaming::Lz77Decoder` | 4 / 4, both decoders |
| §5.10.1 | the 32-row literal copy table | `r21_lz::decompress` | 32 / 32 under the recursive reading of §7j |
| §9 | "a dump of a complete R14 header", 0x245 bytes | `R13R15Header`, `crc8`, `streaming::FlatWalker`, `HeaderVars` | locator CRC `5C2D` (XOR `0x3CC4`), section CRC `275B` (seed `0xC0C1`), both sentinels; its first four `BD`s are §9's printed defaults |
| §20.4.N.1 | 78 R13/R14 object records in 77 sections | `ObjectWalker`, `decode_from_raw` | 78 / 78 type, length, handle and CRC; when this section was written, 44 close on their own §20.4.1 `RL`, 17 error, 4 have no decoder and 13 are class-numbered |

The spec prints no decoded values for the §20.4 dumps, only each
record's type, length, handle and CRC, so "close" there is the crate's
own boundary check run on the spec's bytes. The 13 class-numbered
records (type 500 and up) cannot be dispatched, because the spec prints
no `AcDb:Classes` for its example drawing, and are checked to the walk
and CRC only. `r14-objects.tsv` records each record's expected outcome;
the example fails when a result moves in either direction, so a fixed
decoder flips its row in the same pull request.

### Errata in the specification

Each was found by running a printed example, and each correction is
read from the example's own bytes.

| Where | What the spec prints | What its example says |
|---|---|---|
| §2.3, p8 | "reading 5 longs" | the stream and its parse hold four |
| §4.1, p23 | XOR "the 0x6c bytes" with a sequence it prints 92 bytes of | the generator printed beneath it yields all 0x6C; the last 16 are `32 3B A1 22 F6 22 91 9D E1 8B 1F DA B0 CA 99 02` |
| §20.1, p101 | "The CRC includes the size bytes", with no seed | `0xC0C1`, the seed §9 states for its section, on all 78 records |
| §20.4.28, p126 | DIMENSION (RADIUS): the 10 and 15 points only | one more `BD` after them, the leader length §20.4.29 prints for DIAMETER (0.0 here); the record closes only with it |
| §20.4.49, p155 | TOLERANCE R13-R14: `Height BD`, `Dimgap BD`, then `Text string BS` | one more `BD` before `Height` (0.0 here), and the text is a `TV`: height 0.18 and dimgap 0.09 (AutoCAD's imperial DIMTXT and DIMGAP), extrusion (0, 0, 1), text `{\Fgdt;j}%%v{\Fgdt;n}asdf…`. One record cannot name the extra `BD`, so it is consumed, not named |

### Where the crate disagrees with an example

For every record below, §20.4's own R13-R14 field list closes exactly
on the example (with the two corrections above for RADIUS and
TOLERANCE); the crate's decoder did not when this section was written.
The example's output, and `r14-objects.tsv`'s `expect` column, are the
current state: a decoder fix moves its rows with
`cargo run --release --example spec_examples -- --update`. VIEW
(§20.4.60, #256) was the eighteenth, and closed with #263 before this
landed.

| Example | The crate | Issue |
|---|---|---|
| TEXT, ATTRIB, ATTDEF (§20.4.3-5) | no R13-R14 branch: reads the R2000+ `DataFlags` layout | #226 |
| MTEXT (§20.4.46) | reads the R2000+ linespacing fields on R14 (+69 bits) | #226 |
| TOLERANCE (§20.4.49) | reads its DIMSTYLE handle inside the data fields | #226 |
| INSERT (§20.4.9) | reads the R2000+ `BB` scale flags on R14 | #227 |
| DIMENSION ordinate, linear, aligned, angular, radius, diameter (§20.4.22-29) | reads the R2000+ common dimension fields on R14 | #227 |
| 3DFACE (§20.4.32) | has only the R2000+ layout | #227 |
| VERTEX (2D) (§20.4.11) | reads the tangent only when `flag & 0x02`; §20.4.11 makes it unconditional | #272 |
| POLYLINE (MESH) (§20.4.34) | five `BS`, without the curve type, in another order | #273 |
| LEADER (§20.4.47) | stops after the offset to the block insertion point | #71 |
| UCS (§20.4.62) | reads the R2000+ elevation and ortho fields on R14 | #274 |

MINSERT, SHAPE, VIEWPORT ENTITY HEADER and the R13/R14 DIMSTYLE (#74)
have no decoder, so their examples return `Unhandled`.

### What does not run

- §5.12's two CRC-64 tables: the crate has no 64-bit CRC, so R2007
  header and page CRCs are not verified (#124).
- §5.1's section hashcode table: the crate reads each section's
  hashcode from the file and holds no table to compare.
- §5.13 prints Reed-Solomon configurations, not an example. Its
  system-page polynomial `0x169` is right; `src/reed_solomon.rs` used
  `0x11D` until #275 (§7l).
- §20.4.88 and §20.4.90 print "<<No example>>", and 25 other §20.4
  sections print none.

## 7l. R2007's Reed-Solomon codes, as AutoCAD writes them (2026-10-26, #275)

§5.13 gives R2007 two Reed-Solomon codes over GF(2^8) and lists each one's
primitive-polynomial coefficients, from x^0 up, in the convention of the
Rockliff implementation it cites: `1, 0, 0, 1, 0, 1, 1, 0` for the
(255, 239) system pages and `1, 0, 1, 1, 1, 0, 0, 0` for the (255, 251)
data pages, that is `0x169` and `0x11D` with x^8 implied. It does not say
where the generator's roots start, which end of a stored codeword is the
high-order coefficient, or where in the codeword the message sits. The
corpus settles all three.

On `line_2007.dwg`'s file header page (three interleaved system
codewords, §5.2 and §5.13.2), counting codewords whose syndromes are all
zero:

| Field polynomial | First root | Stored byte *i* is the coefficient of | Valid |
|---|---|---|---|
| `0x169` | α^1 | x^i | **3 / 3** |
| `0x169` | α^0 | x^i | 0 / 3 |
| `0x169` | α^0 or α^1 | x^(254-i) | 0 / 3 |
| `0x11D` | α^0 or α^1 | either | 0 / 3 |

The data pages answer the same way under `0x11D` with four parity bytes,
and in both codes the message is the codeword's first *k* bytes, which is
why `r2007::deinterleave` already gathered the right bytes without
checking them. `src/reed_solomon.rs` had been written for "R2004+ system
sections (§4.1)", a place with no Reed-Solomon coding, using `0x11D` with
the byte order reversed, so no codeword of a real file verified under it;
the reader never called it.

`examples/probe_r2007_rs.rs` checks every codeword of every R2007 drawing
it is given, through the crate's own decoder and encoder. On the eight
R2007 drawings of the pinned corpus (seven nextgis saves from AutoCAD 2016
and ACadSharp's `sample_AC1021.dwg` from AutoCAD 2025):

| Code | Codewords | Intact | Re-encoded to AutoCAD's parity | Restored after (n-k)/2 injected errors | Verify under the other code |
|---|---|---|---|---|---|
| system (255, 239), `0x169` | 187 | 187 | 187 | 187 | 0 |
| data (255, 251), `0x11D` | 4,425 | 4,425 | 4,425 | 4,425 | 0 |

The Corpus job runs it on every pinned R2007 drawing. Re-encoding is the
strongest of the four checks: an encoder built from the measured layout
reproduces every parity byte AutoCAD wrote, so the layout is not merely
consistent with the bytes but the one that produced them.

## 7m. TOLERANCE before R2007: the version branch was upside down (2026-10-26, #226)

§20.4.49 gives TOLERANCE three leading fields — an unknown short, the
height and the dimgap — under "R13-R14 Only", then the insertion point,
x direction, extrusion and text on every release. The R2007+
split-stream reader had already measured that those three are absent
from R2007 on. The inline reader for R13 to R2004 had it upside down:
it read the short only on R2007+, the height and dimgap everywhere, and
then the DIMSTYLE handle inline, past the data-stream boundary. All
three TOLERANCE records of ACadSharp's R14, R2000 and R2004 saves (#225)
errored.

Read as printed, the list closes on all 21 TOLERANCE records of the
public corpus, R14 to R2018. On R2000 and R2004 the 668, 196 and 236
bits of the three records' budgets are exactly three `3BD`s and the
`TV`; on R14 the R13-R14 fields take six more bits. Against the DXF
twins (R2000 to R2018, matched by handle) the insertion point and the
text agree on every record, 12 values per release with 0 differing. The
twins omit 11 and 210 (the defaults the records hold).

On R14 the three leading fields are `10 10 10`, the zero code of `BS`
and `BD`, on all three records, and R14 has no twin. Their widths close
the record; their values corroborate nothing.

```sh
cargo run --release --example probe_tolerance -- sample_AC1015.dwg sample_AC1015_ascii.dxf
```

## 7n. The R2004 data page header, as AutoCAD writes it (2026-10-26, #205)

§4.6 gives every data page a 0x20-byte header, masked word by word with
`0x4164536B` ^ the page's file offset. `section_map::DataPageHeader` named
its eight words after that table, but two of its labels did not describe
what the files hold, and the page-length fact it leaned on was not the one it
used. After unmasking, on every data page of the pinned corpus:

| Offset | Field | Equals |
|---|---|---|
| `0x00` | type | `0x4163043B` |
| `0x04` | section number | the section's id |
| `0x08` | data size | the stored bytes after the header; the section map's reference |
| `0x0C` | page size | the page map's size for the page (header and padding included), **not** the decompressed size |
| `0x10` | start offset | 8 bytes, not 4; the section map's reference |
| `0x18` | header checksum | §4.2 over the 0x20 header bytes with this field zeroed, seeded with `0x1C` |
| `0x1C` | data checksum | §4.2 over the stored bytes, seed 0 |

`DataPageHeader` used to read `0x14` as a page header checksum (it is the
high half of the start offset, always zero), `0x18` as the data checksum (it
is the header checksum) and `0x1C` as unknown (it is the data checksum).
`read_section_payload` only used the type, the data size and the page size,
and treated the page size as a capacity hint, so no section read was wrong.
The struct now carries the measured layout (an 8-byte `start_offset`,
`header_checksum`, `data_checksum`; a breaking change recorded in
CHANGELOG.md).

`examples/probe_data_page_header.rs` unmasks every data page of every
R2004-family drawing it is given and counts agreement with each reading.
On the 28 R2004-family drawings of the pinned corpus (21 nextgis saves from
AutoCAD 2016, 7 from ACadSharp's AutoCAD 2025 samples) and, separately, the
four canonical fixtures:

| Reading | Corpus (580 pages) | Fixtures (12 pages) |
|---|---|---|
| type, section number, data size, page size, start offset as above | 580 | 12 |
| data checksum at `0x1C`, seed 0, over the stored bytes | 580 | 12 |
| header checksum at `0x18`, seeded with `0x1C` | 580 | 12 |
| former label: checksum at `0x14` over the header, seed 0 | 0 | 0 |
| former label: data checksum at `0x18`, seed 0 | 0 | 0 |
| former label: page size at `0x0C` is the decompressed size | 0 | 0 |

The checksum rule was found on the 54 data pages of `sample_AC1032.dwg` by
trying §4.2 over the header (each checksum field zeroed or kept, seeded
with 0, `0x18` or `0x1C`), over the stored bytes, and over the header
followed by them, against both fields: one reading matched each, and the
probe then confirmed them on all 580. The checks that make the walker's section assembly safe fall out of the
same pages: a page's start offset is its index in the section times the
section's page size (580 of 580); a compressed page written by AutoCAD
decodes to a whole page, padding included (447 of 447), so the section
keeps `min(page, bytes left)` of it; a stored page is padded to a whole
page (133 of 133); and `AcDb:Template` in `sample_AC1018.dwg` declares four
bytes and lists no page, which a reader must hand out as zeros, as the
legacy reader's zero-filled buffer does. This crate's writer emits compressed
pages that decode to exactly the bytes left (the fixtures' 12 pages), the
same difference in LZ77 framing noted on #163, so the walker accepts any
length from `min(page, bytes left)` up to a whole page.

The Corpus job runs the probe with `--strict` on every pinned drawing. The
streaming section walker (`src/streaming/r2004_sections.rs`) verifies the
header checksum, then each field above against the page map entry and the
section map reference it was given, then the data checksum, before it
decodes or hands out a byte; `tools/streaming/verify_sections.py` walks the
same pages and compares every assembled section with an independent
parser and the legacy reader.

## 7o. INSERT, the dimensions and 3DFACE on R14: the R13-R14 branches (2026-10-26, #227)

ACadSharp's R14 save (`sample_AC1014.dwg`, #225) is the corpus's only
R14 drawing with INSERTs, dimensions or a 3DFACE, and every one of
those 24 records errored. Each decoder read only the R2000+ branch of
its section, and each section prints an R13-R14 branch of its own:

| Section | R13-R14 branch | The decoder read |
|---|---|---|
| §20.4.9 INSERT | three `BD` scales | the R2000+ `BB` data flags and `RD` / `DD` scales |
| §20.4.22 common dimension data | no attachment, line spacing style or factor, or actual measurement (those four are R2000+) | all four |
| §20.4.32 3DFACE | four `3BD` corners and an unconditional `BS` of invisible edges | the R2000+ flag bits, `RD` first corner and `3DD` corners |

Read as printed, all 24 records close: 12 INSERT, the 11 dimensions of
all seven types, and the 3DFACE.

R14 has no DXF export in the corpus. AutoCAD keeps handles across a
save-as, so `examples/probe_cross_release.rs` compares each R14 record
with the R2000 save of the same drawing, handle by handle. For the
dimensions it is run with `--ignore=attachment,line_spacing_style,line_spacing_factor,actual_measurement`,
the four fields R14 does not store:

| Type | R14 records | Matched in the R2000 save | Equal |
|---|---:|---:|---:|
| INSERT | 14 | 10 | 10 |
| 3DFACE | 1 | 1 | 1 |
| DIMENSION, all types | 11 | 11 | 10 |

Four R14 INSERT handles do not occur in the R2000 save. The one
dimension that differs is the angular 2-line `0x516`, and only in the
last digit of its 16-point's x: `528.6474703372945` against
`528.6474703372946`. Both are read as exact raw doubles, so the two saves
store doubles one unit in the last place apart. That is a property of
what AutoCAD wrote, not a reading the bits leave open.

```sh
cargo run --release --example probe_cross_release -- sample_AC1014.dwg sample_AC1015.dwg INSERT
cargo run --release --example probe_cross_release -- sample_AC1014.dwg sample_AC1015.dwg DIMENSION_LINEAR \
    --ignore=attachment,line_spacing_style,line_spacing_factor,actual_measurement
```

## 7p. LAYOUT before R2004 and on R2007 (2026-10-26, #229)

§20.4.84 prescribes LAYOUT (and the plot-settings block it embeds) with
three version branches the decoder had refused rather than read: a `TV`
plot view name after the plot window on R13-R2000, the three shade-plot
`BS`s from R2004, and the viewport count from R2004. R2007 was refused
because its string stream could not be located; #65 fixed that long ago.
On R14 and R2000 LAYOUT also carries a class number from AcDb:Classes
rather than the fixed code `0x52`, so those records never reached the
decoder at all. All 75 R14, R2000 and R2007 LAYOUT records of the public
corpus (#225) were therefore counted as skipped.

Read with the branches as printed, and routed by class name on R14 and
R2000, all 75 close. AutoCAD keeps handles across a save-as, so each one
can be compared with the R2004 save of the same drawing, the band whose
field list §7d.2 measured. `examples/probe_cross_release.rs` (#264)
does that, ignoring only the fields a release does not store:

| Release | Records | Close | Equal to the R2004 save |
|---|---:|---:|---:|
| R14 | 25 | 25 | 25 |
| R2000 | 25 | 25 | 25 |
| R2007 | 25 | 25 | 25 (every field but the string-stream residue) |

On R14 and R2000 the comparison ignores the shade-plot fields and the
viewport count, which those releases do not store. R14 and R2000 are
also compared with the plot view name included, and it agrees: the name
is empty on every record, so it is measured only as an empty `TV`.

```sh
cargo run --release --example probe_cross_release -- arc_R14.dwg arc_2004.dwg LAYOUT \
    --ignore=shade_plot_mode,shade_plot_resolution_level,shade_plot_custom_dpi,viewport_count,trailing_strings
```

## 7q. ACIS version-2 envelopes (2026-10-26, #228)

§20.4.41 gives the ACIS envelope two forms. Version 1 is a loop of
length-prefixed blocks; version 2 is "an acis file" written straight
into the stream with no length: a binary (SAB) file opens with
`ACIS BinaryFile` and "SAB files will end with
`End\x0E\x02of\x0E\x04ACIS\x0D\x04data`", while a text (SAT) file
"must be parsed to find the end". The decoder refused version 2
outright. ACadSharp's R2004, R2007 and R2010 saves of one drawing (#225)
write their two 3DSOLIDs and one REGION as version-2 SAB envelopes, so
all nine records errored. (R2013 and later move the SAB stream into the
data-storage section, §7h.)

The envelope is now read byte by byte up to the SAB terminator. All
nine records then close exactly on their data-stream boundary, and the
§20.4.41 tail that follows each one is identical across the three saves:
the same wireframe point per handle (`0xD65` at (17.78, −220.85, 2.5),
`0xD69` at (24.99, −220.40, 0), `0xD6A` at (31.69, −220.11, 2.19)), 4
isolines, no wires and no silhouettes. The SAB files differ between
releases only in their ACIS file-version bytes and length (7,620 bytes
for `0xD65` on R2004, 7,660 on R2007 and R2010). AutoCAD writes the ACIS
version each target release reads.

A version-2 envelope that is not `ACIS BinaryFile` (a text SAT file) is
refused with `Unsupported` rather than scanned for an end the spec says
needs a parser.

## 7r. LWPOLYLINE and HATCH on R14 are classes (2026-10-26, #229)

§20.4.85 types LWPLINE `(varies)` and §20.4.75 types HATCH `(varies)`:
both entered AutoCAD with R14 as classes and got the fixed codes `0x4D`
and `0x4E` from R2000. On R14 their type code is a class number from
AcDb:Classes, and dispatch routed only the fixed codes, so all 24 R14
LWPOLYLINE records of the public corpus (23 in ACadSharp's
`sample_AC1014.dwg`, 1 in nextgis `polyline2d_line_R14.dwg`) and the 8
R14 HATCH records were counted as skipped. Routing the class names to
the existing decoders measures them.

HATCH closes as it stands. LWPOLYLINE needs one more branch from
§20.4.85: R13-R14 write every vertex as a `2RD`, where R2000+ write the
first as `2RD` and the rest as `2DD` against the previous vertex. With
it, all 24 records close.

R14 has no DXF export in the corpus, but AutoCAD keeps handles when it
saves a drawing as another release, so the R14 records can be compared
with the R2000 save of the same drawing. `examples/probe_cross_release.rs`
does that, comparing the two decoded values handle by handle:

| Drawings | Type | Records | Equal values |
|---|---|---:|---:|
| `sample_AC1014.dwg` / `sample_AC1015.dwg` | LWPOLYLINE | 23 | 23 |
| `sample_AC1014.dwg` / `sample_AC1015.dwg` | HATCH | 8 | 8 |
| `polyline2d_line_R14.dwg` / `polyline2d_line_2000.dwg` | LWPOLYLINE | 1 | 1 |

The vertices are equal as doubles, bit for bit, so the R14 `2RD` reading
and the R2000 `2DD` reading recover the same coordinates.

```sh
cargo run --release --example probe_cross_release -- sample_AC1014.dwg sample_AC1015.dwg LWPOLYLINE
```

## 7s. AcDb:Header: the variables, in three streams (2026-10-26, #126)

`src/header_table.rs` holds §9 as one table, a row per slot in stream order
with its bit code and the first and last release that carry it;
`src/header_vars.rs` walks the rows of the file's release and nothing else.
Names starting with `$` are AutoCAD's (spelled as DXF's `HEADER` spells them,
so §9's `MENUNAME` is `$MENU` and its `(PSPACE)` group is `$PINSBASE`,
`$PEXTMIN`, ...), a name without it is a slot that is not a variable (control
objects, dictionaries), and `UNKNOWN_<code>_<n>` is a slot §9 itself calls
unknown. `examples/probe_header_vars.rs` decodes every pinned drawing and holds
it to the frame, the boundary of every stream and AutoCAD's DXF export of the
same drawing; the Corpus job runs it.

### The frame

```
R14, R2000, R2004   sentinel(16) RL size            data ... RS crc  sentinel(16)
R2007               sentinel(16) RL size RL bits    data ... RS crc  sentinel(16)
R2010, 2013, 2018   sentinel(16) RL size RL 0 RL bits  data ... RS crc  sentinel(16)
```

`size` counts from byte 20 (R2010 and later: byte 24, the zero `RL` being the
high word of a 64-bit size, as in `AcDb:Classes`) to the end of the data; the CRC
is seed `0xC0C1` over every byte after the first sentinel up to itself. All 52
corpus sections satisfy `20 (24) + size + 2 + 16 = length`, so the length decides
whether R2010 and R2013 carry the zero word (§9 says "if the maintenance version
is greater than 3"; both header bytes are above 3 on every file, so that
condition is not separable here). §9 lists the size in bits as "R2007 Only"; it
is present on R2010, R2013 and R2018 as well.

### R2007 and later: the same split as an object

The size in bits counts from 32 bits before the data. It ends at the first
handle bit, and the string stream is found backwards from there exactly as in an
object (§19.1): the bit before is *strings present*, the 16 (or 32) before that
the size. Three boundaries close on all 36 R2007 to R2018 sections: the data
stream ends where the string stream starts, the strings end where the stream
does, and the handles end inside the last byte. `HANDSEED` is the one handle in
the data stream, as §9 says.

### Where §9 differs from the files

| §9 says | The files |
|---|---|
| `MENUNAME` is `R13-R18` | present in the string stream on R2007 to R2018 as well (read as `$MENU`) |
| four `BS` "unknown short (type 5/6 only)", "do not seem to be required" | present on every release from R14, always the last four slots, a fresh random value in each file; from R2007 they sit after the last handle, in the handle stream |
| nothing after them | R2004 and later: five more bits, always `10101` (five `B` slots), then padding |
| padding "random bits" | all ones on all 52 corpus files; the spec's own 1998 R14 dump differs, so the probe counts it and does not require it |
| `TDCREATE`, `TDUPDATE` | R2000 and later hold universal time: they equal DXF's `$TDUCREATE` and `$TDUUPDATE` to the millisecond, and DXF's `$TDCREATE` is that plus the exporting machine's offset, which the DWG does not store. R14 holds local time (its `$TDCREATE` equals the AC1015 export's) |
| `CMC`: `BS` index, `BL` RGB, `RC` | from R2004 the `BS` is 0 and the colour is the top byte of the `BL`: `0xC0` BYLAYER, `0xC1` BYBLOCK, `0xC3` an index in the low byte, `0xC2` a true colour (DXF writes an approximate index, so only its kind is checkable) |
| unknown `B` before `INTERFERECOLOR`, last unknown `BD` | `$REALWORLDSCALE` and `$SHADOWPLANELOCATION`, from the order DXF writes them and agreement on all 26 R2007+ files that have a twin |
| first unknown `BL` "default 24L", first unknown `TV` default `""` | the TV is `"m"` on every file including the spec's dump; the BL is 0 on all 52 corpus files and 70 in the spec's own R14 dump |
| `CELWEIGHT`: 5 bits of the flags word | index 29 is -1 (BYLAYER) on every R2000 and later file; 0 to 23 map to the published lineweight set and 30, 31 to -2, -3 are not measured |

### Against AutoCAD's DXF (`probe_header_vars`)

| Release | Files | Closed | DXF twins | Slots | Agree | Relation | Disagree | DWG-only | DXF-only |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| AC1014 | 8 | 8 | 8 (AC1015) | 1,592 | 1,279 | 57 | 0 | 264 | 392 |
| AC1015 | 8 | 8 | 8 | 1,888 | 1,670 | 42 | 0 | 184 | 40 |
| AC1018 | 8 | 8 | 8 | 2,072 | 1,765 | 51 | 0 | 264 | 48 |
| AC1021 | 8 | 8 | 8 | 2,432 | 2,093 | 51 | 0 | 296 | 48 |
| AC1024 | 8 | 8 | 8 | 2,472 | 2,109 | 43 | 0 | 328 | 48 |
| AC1027 | 8 | 8 | 8 | 2,488 | 2,103 | 57 | 0 | 336 | 48 |
| AC1032 | 4 | 4 | 2 | 1,244 | 525 | 15 | 0 | 84 | 12 |

*Agree*: equal values, reals to the precision DXF printed, a handle whose DXF
form is a name through the DXF record with that handle (exact name), the control
objects and dictionaries through the DXF `TABLE` records and root dictionary.
*Relation*: a rule between the two files rather than equality. Symbol names are
case-insensitive (`$CMLSTYLE` is `Standard`, the MLINESTYLE record holds
`STANDARD`; R14 writes its names in capitals) and an arrowhead block is named
with a leading underscore (`$DIMBLK` `BoxBlank`, block `_BoxBlank`). The twin is
a different save, so `$TDUUPDATE`, `$TDINDWG` and `$TDUSRTIMER` may
not be earlier in it (the last two by equal amounts) and `$HANDSEED` not lower;
`$VERSIONGUID` is a new GUID on every save while `$FINGERPRINTGUID` agrees
everywhere; before R2010 DXF writes `$XCLIPFRAME` 2 as false; DXF fills the
default visual style into `$INTERFEREOBJVS` and `$INTERFEREVPVS` where the
R2013 and R2018 files hold the null handle (the R2010 file of the same drawing
holds it, and no bit of the R2013 section matches); a true colour (a top byte of `0xC2`)
is an approximate index with no group 420 beside it, so its RGB is not
checkable in DXF's `HEADER`. *DWG-only*: slots DXF's `HEADER` does not write, the unknown
slots and `FLAGS`. *DXF-only*: for AC1014 the 392 are variables of later releases, the twin being
an AC1015 export; otherwise `$ACADVER` is the version magic (checked),
`$DWGCODEPAGE` is in the file header, `$MEASUREMENT` is in `AcDb:Template`
(§22), local `$TDCREATE` and `$TDUPDATE` are not stored, and `$ACADMAINTVER`
and `$LASTSAVEDBY` are not in this section.

AC1014 has no DXF export in the corpus. It is held to the frame, the boundary
and the spec's own R14 dump (`examples/spec_examples.rs` decodes all 199 slots of
it), and compared against the AC1015 export of the same drawing, which agrees
on all but the release-and-save-dependent variables. R13 (AC1012) has no file at
all, so its rows are the table's and unmeasured. `$MAXACTVP` is 48 in the
spec's R14 dump and in all eight R14 files, 64 from R2000.

### dwg-to-dxf writes it back

`src/dxf_header.rs` turns the decoded header into the `$VARIABLE` groups
AutoCAD's own export writes for the same drawing, and nothing more: a variable
the export omits is omitted (`$PELLIPSE`, `$FACETRES`, the unknown slots, the
control-object handles), and so is one the DWG cannot give. The group codes and
the first release of each variable are measured on the 50 AutoCAD exports of
the corpus, which agree with each other (`$XCLIPFRAME` alone changes code, 290
before R2010 and 280 after, and before R2010 a 2 is written false).
`examples/probe_dxf_header.rs` converts each drawing that has an export, reads
`dwg-to-dxf`'s output back with `dxf_reader` and compares every variable; the
Corpus job runs it.

| Release | Files | Written | Agree | Relation | Disagree | Extra | Omitted |
|---|---:|---:|---:|---:|---:|---:|---:|
| AC1015 | 8 | 1,527 | 1,494 | 33 | 0 | 0 | 65 |
| AC1018 | 8 | 1,622 | 1,589 | 33 | 0 | 0 | 66 |
| AC1021 | 8 | 1,915 | 1,883 | 32 | 0 | 0 | 93 |
| AC1024 | 8 | 1,923 | 1,890 | 33 | 0 | 0 | 93 |
| AC1027 | 8 | 1,931 | 1,898 | 33 | 0 | 0 | 93 |
| AC1032 | 2 | 479 | 470 | 9 | 0 | 0 | 27 |

*Relation* is the later-save rule above for `$TDUUPDATE`, `$TDINDWG`,
`$TDUSRTIMER`, `$HANDSEED` and `$VERSIONGUID`. *Extra* is a variable written that
the export omits: none. Every *omitted* variable has a stated reason: the local
`$TDCREATE` and `$TDUPDATE` (the DWG holds universal time), `$ACADMAINTVER` (the
file header says 15 where AC1015 exports 20, and 0 where AC1032 exports 228 or
329), a true-colour `$CECOLOR`, `$CMATERIAL`, `$INTERFEREOBJVS` and `$INTERFEREVPVS`
(handles to MATERIAL and VISUALSTYLE objects, which the converter exports as
untyped placeholders, so the reference would point at the wrong thing), and every name for a symbol record the output does not
define: the converter writes layer 0 and no other symbol table, so `$TEXTSTYLE`,
`$CELTYPE`, `$DIMSTYLE`, `$CMLSTYLE`, a non-zero `$CLAYER` and the like are
omitted until those tables are written (a DXF whose header names a style it never
defines is not a valid reference); an empty name, from a null handle, is kept. Three variables live elsewhere in the DWG and are
read from there: `$MEASUREMENT` (`AcDb:Template`, §22; from R2007 the description
length counts UTF-16 units, AutoCAD writes a length of 1 and two bytes, where §22
counts bytes), `$DWGCODEPAGE` (the file header's code page, 29 and 30 measured)
and `$LASTSAVEDBY` (`AcDb:SummaryInfo`). A handle DXF writes as a name goes
through the decoded symbol-table record; `$CMLSTYLE` takes the case of its
`ACAD_MLINESTYLE` dictionary key and a standard arrowhead block `_BoxBlank` is
`$DIMBLK` `BoxBlank` (only that name is measured; the list of standard arrowheads
is AutoCAD's documented one, and any other block keeps its name, underscore
included). `$ANGBASE` is radians in the database and degrees in DXF group 50, and
before R2007 non-ASCII text is written as `\U+XXXX` escapes; the corpus holds a
zero `$ANGBASE` and only ASCII header text, so neither is measured. Control
characters and the caret are written in DXF's caret notation (a line feed is
`^J`, a caret `^ `) so a value stays on its own line; no corpus header text has
one. R12 and R14 targets keep `$ACADVER` alone: no AutoCAD
export of either exists to measure the set against.

## 7t. MTEXT before R2007: the version branches (2026-10-26, #226)

The inline MTEXT reader, which serves R13 to R2004, read the R2000+ line
spacing on every release and stopped there. ACadSharp's saves of one
drawing (#225) hold 36 MTEXT records on every release:

| Release | Before | What the reader missed | After |
|---|---|---|---|
| R14 | 36 errors, `delta` +21 / +69 / +77 | §20.4.46's line spacing `BS`, `BD` and unknown `B` are R2000+; R14 does not write them | 36 close |
| R2000 | 36 close | nothing: R2000 ends at the unknown bit | 36 close |
| R2004 | 36 errors, `delta` −2 / −122 | the R2004+ `BL` background flags (`−2` is a `BL` 0), and for two records the background block (scale `BD`, `CMC`, transparency `BL`) | 36 close |

One reader now serves every release, with `TV` inline before R2007 and
from the string stream after. All 257 MTEXT records of the public corpus
close. Against the DXF twins (R2000 to R2018, matched by handle) every
value the record holds agrees, including the two R2004+ backgrounds:
flags `1`, scale `1.25`, true colour `0x367542` (group 421 `3568962`)
and transparency `0`.

| Release | Values compared | Differ |
|---|---:|---:|
| R2000 | 368 | 0 |
| R2004 | 376 | 0 |
| R2007 / R2010 / R2013 / R2018 | 411 each | 0 |

The comparison reads R2007+ DXF as UTF-8 and undoes the DXF's caret
escapes (`^J` where the record holds a line feed). One MTEXT per save
(`0x1032`) has no entity of its handle in the DXF and is counted, not
compared. R13/R14 store no line spacing, so `MText::linespace_style`
and `linespace_factor` read 0 there rather than a default the record
does not hold.

```sh
cargo run --release --example probe_mtext -- sample_AC1018.dwg sample_AC1018_ascii.dxf
```

## 7u. VIEWPORT: §20.4.38 is right as printed (2026-10-26, #71)

§7i left VIEWPORT reported rather than fixed: six records of one
shape on one file, a 1125-bit budget, and a decoder that stopped after
the centre, width and height. The public corpus (#225) adds the same
drawing saved by AutoCAD 2025 as R14, R2000, R2004, R2007, R2010 and
R2013, each with its DXF export, and a second R2018 drawing whose nine
viewports differ in size, centre and view height.

The decoder now reads §20.4.38 field for field, with its version
branches exactly as the section prints them: the R2000+ view block,
the R2007+ `BS` grid major *between* the circle zoom and the R2000+
frozen-layer count, the R2004+ `BS` shade plot mode, and the R2007+
lighting block ending in a `CMC`. Nothing is measured against the
spec; nothing is added to it.

| File | Release | Records | Close (`delta 0`) | Values compared with the DXF twin | Differ |
|---|---|---:|---:|---:|---:|
| `sample_AC1014.dwg` | R14 | 6 | 6 | no twin | |
| `sample_AC1015.dwg` | R2000 | 6 | 6 | 252 | 0 |
| `sample_AC1018.dwg` | R2004 | 6 | 6 | 258 | 0 |
| `sample_AC1021.dwg` | R2007 | 6 | 6 | 294 | 0 |
| `sample_AC1024.dwg` | R2010 | 6 | 6 | 294 | 0 |
| `sample_AC1027.dwg` | R2013 | 6 | 6 | 294 | 0 |
| `sample_AC1032.dwg` | R2018 | 6 | 6 | 294 | 0 |
| `export_sample.dwg` | R2018 | 9 | 9 | no twin | |

Values are matched by handle (DXF group 5) against the `AcDbViewport`
group of the twin and compared to 1e-9 relative. Among them are values
no default could produce: the status word (`819232` and `819808`,
group 90), the circle zoom (`1000`, 72), the lens length (`50`, 42),
the view centres and view heights, and the ambient light colour
`0xC2333333` — RGB 51/51/51, the true colour group 421 carries as
`3355443`. The two `B` UCS flags are read in the spec's order (74, then
71) and land on `0` and `1` as the DXF has them.

What is not measured: the 15 records of the R14 save and
`export_sample.dwg` close but have no DXF twin; no R13 file exists;
every viewport in the corpus has an empty style sheet name, so the `TV`
is measured only as an empty string, inline before R2007 and from the
string stream after; every snap angle and view twist angle is `0`, so
the two angle fields agree with the DXF trivially (the probe converts
the record's radians to the degrees DXF writes, which no record
exercises); and every ambient light colour is the true colour
`0xC2333333`, so an indexed (`0xC3`) colour has not been seen.

`examples/probe_viewport.rs` prints every VIEWPORT of a file with its
values keyed by DXF group, and with a DXF twin as its second argument
reports every disagreement:

```sh
cargo run --release --example probe_viewport -- sample_AC1032.dwg sample_AC1032_ascii.dxf
```

## 7v. TEXT, ATTRIB and ATTDEF before R2007: the data flags were inverted (2026-10-26, #226 / #62)

The 19-file corpus held text only on R2018, so the R13-R2004 TEXT
reader had never met a record. ACadSharp's saves of one drawing (#225)
hold 29 TEXT, 4 ATTRIB and 5 ATTDEF records on every release, and on
R14, R2000 and R2004 all 114 of them errored, with 4 more TEXT records
on `export_sample.dwg` (R2018).

Three defects, each fixed toward §20.4.3 as printed:

| Defect | Was | §20.4.3 / §20.4.5 says | Where it showed |
|---|---|---|---|
| R2000+ data flags | a field read when its bit is **set** | "present if !(DataFlags & bit)": a set bit means absent | R2000 / R2004: the record with flags `0xFF` (every field absent) read every field |
| R13-R14 | the R2000+ reader | its own branch: no data flags, `BD` elevation and angles, `2RD` points, `3BD` extrusion | all R14 text records |
| oblique, rotation, width factor | `BD` on every release (#62) | `RD` (and the R2000-R2004 reader also read the height as `BD`) | the 4 R2018 records that write one of them |
| ATTDEF prompt | read before the tag | after the ATTRIB fields ("Common ATTRIB Entity Data", then the prompt) | every R14 to R2004 ATTDEF |

#62 had asked for a file that writes the three optional fields. That
file is `export_sample.dwg`. Its four failing records describe their
own values in their text, and the `RD` reading recovers those values
exactly:

| Handle | Data flags | Field written | `RD` value | The record's own text |
|---|---|---|---|---|
| `0x44A` | `0xFB` | oblique | 0.17453292519943295 (10°) | "Text obliquing = 10" |
| `0x44B` | `0xF7` | rotation | 0.5235987755982988 (30°) | "Rotated text angle = 30" |
| `0x454` | `0xEF` | width factor | 2.0 | "Text width = 2.0" |
| `0x465` | `0xAD` | alignment point, width factor, horizontal alignment | — | "Justification fit" |

A `BD` reading of the same bits ends 62 bits short or 2 bits long, or
hits the reserved `11` code.

With the three fixes, every TEXT, ATTRIB and ATTDEF record of the public
corpus closes:

| Type | Records | Close | Values compared with the DXF twin (R2000-R2018) | Differ |
|---|---:|---:|---:|---:|
| TEXT | 224 | 224 | 870 | 0 |
| ATTRIB | 28 | 28 | 189 | 0 |
| ATTDEF | 35 | 35 | 276 | 0 |

The comparison matches records to the twin by handle. It covers
insertion point and elevation, alignment point, height, width factor,
rotation and oblique (in degrees in the DXF), thickness, the three
alignments, the text, and for attributes the tag, flags, field length
and prompt, wherever the DXF writes the group. In this drawing every
TEXT the DXF twin covers leaves rotation, oblique and width factor at
their defaults, so the `RD` reading rests on `export_sample.dwg`, which
has no twin. The R14 save has no twin, so its 38 records are measured
on the boundary only.

`examples/probe_text.rs` prints the closure and the comparison:

```sh
cargo run --release --example probe_text -- sample_AC1015.dwg sample_AC1015_ascii.dxf
```

## 7w. MESH: the DXF twin names the trailing bits, and MESH is on every release (2026-10-26, #71)

§7i left MESH's last two bits, `10`, unread: the zero code of `BS`, `BL`
and `BD` alike, on two records that both encode zero, so the bits alone
could not say which field they were or whether they were a field at
all. The public corpus (#225) holds the same two meshes in ACadSharp's
saves of one drawing as R14, R2000, R2004, R2007, R2010, R2013 and
R2018, with AutoCAD's DXF export of every save but R14.

**The DXF says what the bits are.** Each `AcDbSubDMesh` group in the
twins ends `95` (crease count), `140` once per crease, then one more
`90` with the value `0`. AutoCAD writes every other `BL` of the record
as a group 90 (the face-list entries and the edge endpoints), and in
the same order the record holds them, so the two bits are a `BL`, and
it is `0`. The decoder now reads it and refuses a non-zero value, whose
contents nothing in the corpus shows.

**MESH is not R2010-only.** AutoCAD 2025 writes the same record into
its R14, R2000, R2004 and R2007 saves: same class name, same budget
(93,941 bits for `0x343` on R2000, R2004 and R2010), not a proxy. The
decoder refused every pre-R2010 MESH on the assumption that those
releases never hold one. Without that gate:

| Release | Records | Close (`delta 0`) | Values compared with the DXF twin | Differ |
|---|---:|---:|---:|---:|
| R14 | 2 | 2 | no twin | |
| R2000 | 2 | 2 | 926 + 904 | 0 |
| R2004 | 2 | 2 | 926 + 904 | 0 |
| R2007 | 2 | 2 | 926 + 904 | 0 |
| R2010 | 2 | 2 | 926 + 904 | 0 |
| R2013 | 2 | 2 | 926 + 904 | 0 |
| R2018 | 2 | 2 | 926 + 904 | 0 |

The comparison is complete, not sampled: version, blend crease,
subdivision level, every vertex coordinate, every face-list entry, every
edge endpoint, every crease and the trailing count, in DXF order (926
values for the 62-vertex `0x343`, 904 for the 64-vertex `0x380`).

Still undetermined: the `BS` widths of version, subdivision level and
vertex count, since every value is below 256 and a `BS` and a `BL`
spend the same bits there.

```sh
cargo run --release --example probe_mesh -- sample_AC1015.dwg sample_AC1015_ascii.dxf
```

## 7x. The symbol-table prefix holds the reactor count (2026-10-26, #40 / #35)

§20.4.x prints the same common data for every table entry, R2000 on:
the EED chain, `BL Numreactors`, `B XDic Missing Flag` (R2004+), and the
`B` §20.1 adds for AcDs binary data (R2013+). Then `TV name, B 64-flag,
BS xrefindex+1, B Xdep`. The inline path (`tables::read_table_entry_header`,
R2000 and R2004) reads that. The R2007+ split-stream path
(`tables::modern`) skipped the `BL`, read the flags as `B, B, BS` in a
release-dependent order, and on R2013+ read an `RC` after a set
binary-data bit (§7a's table). Both closed on every entry that had no
reactors, because the `BL` of 0 is the code `10` and the shifted reads
consumed the same bits:

| Release | Bits after the EED | Old reading | Measured |
|---|---|---|---|
| R2007, R2010 | `1 0` | `B` xdictionary-missing, `B` Xdep | `BL` 0 |
| | next | `B` 64-flag | `B` xdictionary-missing |
| | `1 0` | `BS xrefindex+1` 0 | `B` 64-flag, `B` Xdep |
| R2013, R2018 | `1 0` | `B` xdictionary-missing, `B` binary data | `BL` 0 |
| | next two | `B` 64-flag, `B` Xdep | `B` xdictionary-missing, `B` binary data |
| | `1 0` | `BS xrefindex+1` 0 | `B` 64-flag, `B` Xdep |

So the old 64-flag was the xdictionary-missing bit (clear on every entry
that has an xdictionary), and on R2013+ the old Xdep was the binary-data
bit. The "release-dependent order" was this shift, not the format. A
reactor count above 0 is the code `01` and a byte. On R2013+ the old
reading took the `01` for its two flags and the byte for its `RC`. On
R2007 and R2010 nothing absorbed it, and DIMSTYLE `ISO-25` (`0x39A`, 8
reactors), `custom_dim_style` (`0x994`, 2) and `custom_style_ltypes`
(`0xC88`, 1) errored in ACadSharp's saves of both releases (#228).

`tables::modern::skip_object_prefix` now reads the `BL`, the prefix
`objects::modern` reads for every other non-entity object (§7d), and
nothing after the binary-data bit. Two bits then remain on every R2007+
table entry, read as `B 64-flag, B Xdep` with no `xrefindex+1`.

### Measured against AutoCAD's DXF

`examples/probe_table_prefix.rs` reads the prefix and the flags of every
table entry and compares them, by handle, with AutoCAD's DXF export of
the same drawing. Over the 32 R2000-R2018 drawings of the public corpus
(#225) that have a twin, 1,078 table entries, 1,020 with a twin record:

| Compared | Agree | Differ |
|---|---:|---:|
| `BL num_reactors` = the `330`s in `{ACAD_REACTORS` (18 non-zero) | 1,020 | 0 |
| R2004+ xdictionary-missing = no `{ACAD_XDICTIONARY` (53 present) | 840 | 0 |
| Xdep = bit 16 of group 70 (BLOCK_RECORD's 70 is its insertion units) | 786 | 0 |
| R2000/R2004 inline name = group 2 | 360 | 0 |

Three twins carry an xdictionary the drawing lacks: LTYPE `GAS_LINE`
(`0x6B2`) in the R2004, R2007 and R2010 exports points at dictionary
`0x1455` (`ACAD_XREC_ROUNDTRIP` holding an `ACAD_ROUNDTRIP_2011_LINETYPE`
xrecord), which is not an object of those DWGs and is absent from the
R2013 export. The export adds round-trip data when it writes an older
release. The DWG bit says "missing" on all three, so the probe counts
them apart rather than as differences.

Every R2007-R2018 entry (676) reads the two flag bits `1, 0`, and every
R2000/R2004 entry (402) reads 64-flag set, `xrefindex+1` 0 and Xdep
clear. That also answers #35's labelling question for the inline path:
the first inline bit cannot be Xdep, since it is set on all 402 entries
and group 70 bit 16 is clear on all of them. For R2007+ the export rules
out the swapped order for the same reason, but a lone `BS xrefindex+1`
of 0 is the same two bits `1 0`. No drawing in either corpus has an
xref-dependent table entry to tell them apart (#290).

The six DIMSTYLE records decode to identical values on R2004, R2007,
R2010, R2013 and R2018, in all fifteen fields `DimStyleEntry` surfaces. `ISO-25`'s ten non-default groups in the R2007
export (`41` 2.5, `42` 0.625, `44` 1.25, `73` 0, `74` 0, `77` 1, `140`
2.5, `141` 2.5, `143` 0.03937007874016, `283` 0) equal the decoded
values, and the records the export writes no overrides for decode to
the defaults.

Coverage of the public corpus moves only on those six records:
`sample_AC1021.dwg` 834/76/15 → 837/76/12 and `sample_AC1024.dwg`
851/57/15 → 854/57/12 (decoded/skipped/errored).

## 7y. A HATCH spline edge without fit points has no tangents (2026-10-26, #228)

§20.4.75 ends the R2010+ ("R24") spline edge of a HATCH boundary with
`BL numfitpoints`, the fit points, and then `2RD` start and end tangents,
with no condition. The corpus has one spline edge: HATCH `0xAA8` of
ACadSharp's `export_sample.dwg` (R2018), a closed boundary of degree 3
with 13 knots, 9 control points and 0 fit points. Read with the
tangents, its `num_boundary_handles` decodes to 298,029,779 and the
record errors. Without them it closes:

| Field | Value |
|---|---|
| `numfitpoints` | 0 (then no tangents) |
| `numboundaryobjhandles` | 1 |
| style, pattern type | 1, 1 |
| pattern angle, scale, double | 0, 1, false |
| definition line | 45°, through (0, 0), offset (−0.0884, 0.0884): ANSI31's 0.125 spacing |
| pixel size | 0.0177 (the path is derived, flag 4) |
| seed point | (5.13, 2.59) |
| end bit | 2827, the string-stream start; `"LINEAR"` and `"ANSI31"` both consumed |

The boundary's control polygon starts and ends at (4.43, 2.59), the first
and last of the 7 fit points SPLINE `0xA7F` of the same drawing decodes
to, and the seed point lies inside that curve. So
`hatch::decode_spline_edge` reads the tangents only when there are fit
points, which keeps §20.4.75's order for that case. No corpus record has
a spline edge with fit points, so that case is unmeasured.

Reproduce both readings with the entity field-list probe from bit 2374,
just after `numfitpoints` (0, at bit 2372):

```sh
D=../dwg-corpus/acadsharp/export_sample.dwg   # fetched by tools/corpus/fetch.py
cargo run --release --example probe_entity_field_list -- $D 0xAA8 BL,BS,BS,BD,BD,B,BS,BD,2BD,2BD,BS,BD,BL,2RD --at=2374   # delta 0
cargo run --release --example probe_entity_field_list -- $D 0xAA8 2RD,2RD,BL --at=2374                                 # BL = 298029779
```

## 7z. The R13/R14 DIMSTYLE block (2026-10-26, #74)

§20.4.68 gives R13 and R14 their own DIMSTYLE layout. It starts with the
flags, several of them `RC` where R2000 has `BS`. Then come the reals,
five `T` strings (`DIMPOST`, `DIMAPOST` and the three arrow-block names,
which R2000 moves to handles) and three `BS` colours. The dispatcher
reported every R14 DIMSTYLE as unhandled, since the order had never been
measured. `tables::dimstyle::decode_r13_r14` now reads the block as
printed, followed by the `B` §20.4.68 lists as common after `DIMLWE`.

| Check | Result |
|---|---|
| `RL` boundary of the common object data | all 20 R14 records close: 6 in `sample_AC1014.dwg`, 2 in each of the 7 nextgis R14 drawings |
| Without the trailing `B` | the records fail the check; `0x27` of `sample_AC1014.dwg` ends one bit short (delta −1) |
| The 15 surfaced fields against the R2000 save, handle for handle | 20 / 20 equal. Only the names differ: R14 stores `STANDARD` where R2000 has `Standard` |
| The spec's own example, §20.4.68.1 | closes; `r14-objects.tsv` moves from `unhandled` to `closes` |

`DIMALTRND` has no slot in the R13/R14 block, so it reads 0 there.
`examples/probe_dimstyle_r14.rs` repeats the cross-release comparison:
`probe_dimstyle_r14 <r14.dwg> <r2000.dwg>` for each of the eight pairs.

## 7aa. GEODATA, a non-entity object read as §20.4.78 prints it (2026-10-26, #229)

GEODATA attaches a geographic coordinate system to the model-space
block. §20.4.78 lists it among the objects: a class number from
`AcDb:Classes` and the common object data of §20.1. The crate decoded it
as an entity with an invented field list (an origin, two axis vectors,
two scales, five strings) keyed on a fixed code, 0x4FB. The corpus's one
record, handle `0x241` of ACadSharp's `geoloc.dwg` (R2018), carries a
class number, so it never reached that decoder.

`entities::geodata` now reads §20.4.78's field list after the common
object data. The dispatcher routes the class name `GEODATA` to it, and
the fixed code to the object path. Read that way, the record closes on
its string stream with every string consumed, and all 77 values
`examples/probe_geodata.rs` compares equal AutoCAD's DXF export of the
drawing: object version 3, design coordinate type 1, the design and
reference points, both unit scales and units (0.0254, inches), the up
and north directions, the scale estimation, the sea-level fields, the
projection radius 6,362,228.65, the 3,195-character MapGuide coordinate
system definition, the GeoRSS tag, the three empty observation tags,
and all 12 mesh points. There are 0 mesh faces.

### Errata in the specification

| Where | What the spec prints | What the record says |
|---|---|---|
| §20.4.78, p194 | `North direction 3RD` (object version 2 and later) | a `2RD`. At bit 566, `0x241`'s two `RD`s read (2.4e-16, 1.0), DXF `12`/`22`. The next 64 bits read as a third `RD` give −3.1e-151. They are the scale estimation `BL` (1, DXF `95`), the user scale factor `BD` (1.0, `141`), the sea-level `B` (0, `294`) and `BD` (0.0, `142`), and the first bits of the projection radius. Read as a `3RD`, the radius becomes −288,169,236 for DXF's 6,362,228.65, the mesh-point count 50 for DXF's 12, and the record overruns its 3,859-bit boundary. Read as a `2RD`, it ends exactly on it |

The object version 1 (AutoCAD 2009) layout and the civil block §20.4.78
adds to R2007 and earlier files are read as printed. The corpus has
neither, so both are unmeasured. A record they do not fit fails the
boundary check rather than decoding to invented values.

```sh
cargo run --release --example probe_geodata -- ../dwg-corpus/acadsharp/geoloc.dwg ../dwg-corpus/acadsharp/geoloc.dxf
cargo run --release --example probe_field_list -- ../dwg-corpus/acadsharp/geoloc.dwg 577 BL,BS,3*BD,3*BD,BD,BL,BD,BL,3*BD,2*RD,BL,BD,B,BD,BD,TV,TV,TV,TV,TV,BL,48*RD,BL   # delta 0
```

## 7ab. LEADER: one `3BD` missed, and no text box from R2010 (2026-10-26, #71)

§7i reported LEADER's twelve missing bits on its one R2018 record and
stopped, because one record could not choose between the continuations
that fit. The public corpus (#225) holds the same LEADER (handle
`0x512`) in ACadSharp's saves of one drawing as R14, R2000, R2004,
R2007, R2010, R2013 and R2018, and every save but R14 has AutoCAD's
DXF export. Seven releases settle what one could not.

The old field list read four `3BD`s after the points where §20.4.47 has
five — origin, extrusion, x direction, offset to the block insertion
point, then the R14+ end point projection — so the value it called the
end projection was the origin, which equals the first point, the
default the section gives it. With the fifth `3BD` read, §20.4.47's own
branches close the record on every band but one:

| Release | Delta before | Closes as | Box height / width |
|---|---:|---|---|
| R14 | −282 | the R13-R14 branch: `BD` dimgap, box, `B`, `B`, `BS` arrowhead type, `BD` dimasz, `B`, `B`, `BS`, `BS` byblock colour, `B`, `B` | 1.844821… / 16.374004… |
| R2000 | −144 | the R2000+ branch: box, `B`, `B`, `BS`, `B`, `B` | 1.844821… / 16.374004… = DXF 40 / 41 |
| R2004 | −144 | same | same, = DXF 40 / 41 |
| R2007 | −144 | same | same, = DXF 40 / 41 |
| R2010 | −12 | the R2000+ branch **without** the box | not in the record |
| R2013 | −12 | same | not in the record |
| R2018 | −12 | same | not in the record |

**The departure is measured, not borrowed.** §20.4.47 files box height
and box width under "Common". On R2010, R2013 and R2018 the record is
exactly 132 bits shorter than on R2007 — two non-default `BD`s — and
the six bits after the end point projection are `011000`, the same six
bits that follow the box width on R2000-R2007: hook line on the x
direction `0` (DXF 74 is `0`), arrowhead on `1`, then the R2000+ `BS`
`0` and two `B`s `0`. The R2010+ DXF exports still write 40 and 41 with
the R2007 values, so AutoCAD carries them somewhere else; this record
does not.

On R14 the two creation-time values corroborate the R13-R14 branch
independently of the bit count: dimgap `0.625` and dimasz `2.5` are the
DIMGAP (group 147) and DIMASZ (group 41) of the drawing's ISO-25
DIMSTYLE, the style the leader names (DXF group 3). Both `BS`s that
follow the `B`s read `256`; only the width is claimed for them.

The DXF twins agree with every value they write: groups 73 and 74, the
box on R2000-R2007, and the vertices. AutoCAD's export holds four
vertices where the record holds three, because with a hook line (group
75 `1`) it inserts the hook line's start, 2.5 (DIMASZ) before the last
point; the record's first two points and last point equal the DXF's
first two and last. The DXF omits groups 71 and 72, so the arrowhead
flag and path type are compared with nothing.

`examples/probe_leader.rs` prints each record and, given the twin, the
comparison:

```sh
cargo run --release --example probe_leader -- sample_AC1015.dwg sample_AC1015_ascii.dxf
```

## 7ac. The object stream's records, framed from the stream (2026-10-26, #205)

The streaming object walker (`src/streaming/records.rs`,
`r2004_objects.rs`) frames an R2004-family object section without the
`AcDb:Handles` map: after the four-byte `RL 0x0DCA` prologue, each record is
`MS` size, on R2010 and later an `MC` handle-stream size, `MS` bytes of
payload and a two-byte CRC, and the next record follows at once. Three
properties of that framing were not on record before it was coded; each was
measured on every object section of the 28 R2004-family drawings of the
pinned corpus (21 nextgis saves from AutoCAD 2016, 7 from ACadSharp's
AutoCAD 2025 samples) with an independent parse, and then enforced by the
walker.

| Property | R2004 | R2010 | R2013 | R2018 |
|---|---|---|---|---|
| Records walked | 2,417 | 2,188 | 1,787 | 1,615 |
| The records end exactly at the section's size (no padding, no zero-length trailer) | 8 of 8 drawings | 8 of 8 | 8 of 8 | 4 of 4 |
| The CRC (§2.14.1, seed `0xC0C1`) over the bytes from the `MS` through the last payload byte, `MC` included, equals the two bytes after the payload | 2,417 | 2,188 | 1,787 | 1,615 |
| The `MC` handle-stream size is at most 8 times the record's payload bytes | n/a | 2,188 | 1,787 | 1,615 |

The CRC rule is the same one §20.1 gives the R13 to R2000 records, and the
`MC` is inside it. Three other readings were tried on the same 8,007 records
and agree with none: a CRC over the `MS` alone, over the payload alone (the `MC`
included, the `MS` not), and, on R2010 and later, over the `MS` and the payload
without the `MC`. The legacy `ObjectWalker` never checked the CRC. Its comment that "trailing padding in some R2018 files" ends the
stream at a zero `MS` describes no drawing of this corpus: the four ACadSharp
R2018 saves, `empty.dwg`, `export_sample.dwg` and `geoloc.dwg` tile exactly;
the streaming walker refuses a zero `MS` instead (`RecordSize`).

Four larger drawings, `sample_AC1018`, `sample_AC1024`, `sample_AC1027` and
`sample_AC1032`, hold object sections of 1.2 to 1.6 MB in 41 to 54
`0x7400`-byte pages. 71 of their 3,644 records straddle a page boundary, 36 of
those are larger than a page (the largest is 150,148 bytes) and the framing
carries across them unchanged. The pages of the object section appear in
section order in the page map of all 28 drawings, 28 of 28 contiguous from
offset 0. `AcDb:AcDsPrototype_1b`, in the three saves that have one
(`sample_AC1027`, `sample_AC1032` and `export_sample`), is the only section of
any drawing whose pages the page map lists out of order: its two pages come
second first.

The same large drawings corrected a limit of the R13 to R2000 container walk:
it refused an object map chunk of more than 2,032 bytes. A chunk's size counts
its own two bytes, so the spec's 2,032 bytes of entries make a chunk of 2,034,
and AutoCAD's first chunk in `sample_AC1014.dwg` is 2,033 and in
`sample_AC1015.dwg` 2,034. The walk's evidence had been limited to files under
160 KB, whose maps are one short chunk.

## 7ad. TABLESTYLE until R2007: the spec prints it (2026-10-26, #151)

The notes above call TABLESTYLE "declined" (§7d) because "§20.4 has no
entry" for it. It has: §20.4.101 prints the object, in two layouts.
§20.4.101.1 is the one "until R21", and it is the one a record on R14 to
R2007 follows; §20.4.101.2 is R2010's, a different object, and stays
`Unhandled` here (the next section's job). The corpus holds one TABLESTYLE,
`Standard`, per drawing, so the 32 records of R14 to R2007 are every one
there is, and each closes on its data-stream boundary.

The field list is the spec's: `TV` description, `BS` flow direction, `BS`
flags, two `BD` margins, two `B` suppress flags, then three row styles
(data, title, header) of `BD` text height, `BS` alignment, two colours, `B`
background and six borders of `BS` line weight, `B` visible and a colour.
Two readings are not in the spec's table and are read from the bits:

| Where | What the spec prints | What the bytes say |
|---|---|---|
| §20.4.101.1, p233-234 | `Data type BL`, `Data unit type BL` under "R2007+", then `Format string TV` with no gate | all three are R2007+. `arc_2004.dwg` handle `0x65`: 1849 bits of data, 220 of header and 3 x 543 of rows, with every row ending on its sixth border colour; the title row's `BD 6.0` starts at bit 842, exactly where a `TV` would. `arc_2007.dwg` handle `0x65`: 1875 bits, 138 of header and 3 x 579, the extra 36 per row being `BL 512`, `BL 0` and a `TV` that costs no data-stream bits (`unread strings: []`) |
| §2 key, `TC`; §20.4.101.1 on R2000 | `Text color CMC`, `Fill color CMC`, `Border color CMC`, and a `CMC` is one `BS` before R2004 | on R2000 the colours are the 44-bit `TC` (`BS`, `BL`, `RC`) of R2004: `arc_2000.dwg` handle `0x65` has the same 1849-bit budget as R2004's and closes only so. Read as a one-`BS` `CMC`, the second colour lands on `BS 0`, then `BS 2054`, which is the tail of `0xC1000000` and the zero `RC` |

The colours are `TC` on R14 too: the eight R14 records close with the
R2000 reading. R14's boundary is the `RL` inside the common object data.

Against AutoCAD's DXF export of the same drawings (R2000 to R2007, matched
by the handle in group 5), every decoded value equals the group that carries
it: 76 values per record on R2000 and R2004 and 85 on R2007 (the data type,
unit type and format string), 1896 in all, none differing and none missing
from the DXF. A colour is compared as the index its `TC` stands for (`0xC1`
by block is 0, `0xC3` carries the index, `0xC0` by layer is 256), which is
what groups 62 to 69 hold. The row order is the DXF's order of its groups
7: data, title, header. The text style of each row (group 7) is an object
handle in the handle stream, which the decoder does not read; the DXF
writes the style's name, so that group is not compared. R14 has no twin
(nextgis exported it to R12 DXF, ACadSharp has no AC1014 export), so its 8
records are boundary-only evidence.

```sh
python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
cargo run --release --example probe_tablestyle -- ../dwg-corpus
```

| Release | Records | Close | Not decoded (§20.4.101.2) | Values compared | Differ |
|---|---|---|---|---|---|
| R14 | 8 | 8 | 0 | no twin | 0 |
| R2000 | 8 | 8 | 0 | 608 | 0 |
| R2004 | 8 | 8 | 0 | 608 | 0 |
| R2007 | 8 | 8 | 0 | 680 | 0 |
| R2010, R2013, R2018 | 20 | 0 | 20 | - | - |

The oracle (`examples/dxf_oracle`) does not score TABLESTYLE yet.

## 7ae. IMAGEDEFREACTOR and RASTERVARIABLES (2026-10-26, #150)

§20.4.82 and §20.4.91 print the two small objects an IMAGE entity drags
along, both after the common object data and `BL` class version: the
reactor an IMAGEDEF keeps for each IMAGE (`IMAGEDEF_REACTOR` in the DXF and
the class table, `IMAGEDEFREACTOR` in the spec) holds nothing else, and
RASTERVARIABLES adds three `BS`, the display frame, the display quality and
the units. Their notes in §7d and the STATUS table counted them unreached;
the dispatcher had no class-name arm for either.

Each drawing of the corpus that holds an IMAGE holds one of each, the
seven ACadSharp `sample_AC10xx` saves, and all 14 records close on their
data-stream boundary from R14 to R2018: after the common object data the
reactor has 10 bits, its `BL` class version, and RASTERVARIABLES 32, that
`BL` and its three `BS`, on every release.

Against AutoCAD's DXF export (R2000 to R2018, matched by the handle in group
5) all 30 values equal the groups that carry them: the 6 class versions
(group 90), and for RASTERVARIABLES 4 values on each of 6 releases (groups 90,
70, 71, 72). R14 has no twin.

```sh
cargo run --release --example probe_imagedef_reactor -- ../dwg-corpus
cargo run --release --example probe_rastervariables -- ../dwg-corpus
```

| Release | IMAGEDEF_REACTOR close | RASTERVARIABLES close | Values compared | Differ |
|---|---|---|---|---|
| R14 | 1 / 1 | 1 / 1 | no twin | |
| R2000 | 1 / 1 | 1 / 1 | 5 | 0 |
| R2004 | 1 / 1 | 1 / 1 | 5 | 0 |
| R2007 | 1 / 1 | 1 / 1 | 5 | 0 |
| R2010 | 1 / 1 | 1 / 1 | 5 | 0 |
| R2013 | 1 / 1 | 1 / 1 | 5 | 0 |
| R2018 | 1 / 1 | 1 / 1 | 5 | 0 |

## 7af. ACDBPLACEHOLDER by class number on R14 and R2000 (2026-10-26)

§5 lists ACDBPLACEHOLDER with the fixed object types (`0x50`) and
prints no field list for it: the record is the common object data and
nothing else (`src/objects/placeholder.rs`, §7a). R2004 and later write it
under `0x50` and have decoded since §7d. R14 and R2000 do not: their
drawings give it a class number from `AcDb:Classes`, as they do XRECORD, and
the class-name dispatch did not route it, so the 16 placeholders of the
corpus's R14 and R2000 drawings (one per drawing) were skipped. They are
the same record, and each closes on its data-stream boundary: the `RL` inside
the common object data on R14, the prologue's `RL` on R2000.

All 52 placeholders of the corpus close, by either route. Against AutoCAD's
DXF export (R2000 to R2018, matched by handle) the one value the record
holds, its `BL` reactor count, equals the number of reactors in the
object's `{ACAD_REACTORS` bracket on all 42 records that have a twin; R14
and three ACadSharp R2018 drawings have none.

```sh
cargo run --release --example probe_placeholder -- ../dwg-corpus
```

| Release | Records | Close | Reactor counts compared | Differ |
|---|---|---|---|---|
| R14 | 8 | 8 | no twin | |
| R2000 | 8 | 8 | 8 | 0 |
| R2004 | 8 | 8 | 8 | 0 |
| R2007 | 8 | 8 | 8 | 0 |
| R2010 | 8 | 8 | 8 | 0 |
| R2013 | 8 | 8 | 8 | 0 |
| R2018 | 4 | 4 | 2 | 0 |

## 7ag. SORTENTSTABLE (2026-10-26, #149)

§20.4.93 prints the draw-order table of a block as `BL` numentries and that
many sort handles, typed `H` but "CODE 0, i.e. part of the main bit stream,
not of the handle bit stream": the one place in a non-entity object where
a handle sits in the data stream. A code-0 handle's counter bytes are the
absolute handle, so each reads as one `H` with no offset to resolve; the
decoder refuses another code instead of reading an offset as a handle. The
entity each sorts is the matching handle of the handle stream, which is not
read.

The dispatcher had no class-name arm for it, so all 27 SORTENTSTABLE records
of the corpus were skipped. All 27 close on their data-stream boundary: three
on each release from R14 to R2013 and nine on the R2018 drawings.

Against AutoCAD's DXF export (R2000 to R2018, matched by the handle in group
5) the entry count and every sort handle equal the groups the DXF writes as
pairs, the entity under group 331 and the sort handle as a further group 5:
66 values, none differing. R14 has no twin.

```sh
cargo run --release --example probe_sortentstable -- ../dwg-corpus
```

| Release | Records | Close | Values compared | Differ |
|---|---|---|---|---|
| R14 | 3 | 3 | no twin | |
| R2000 | 3 | 3 | 11 | 0 |
| R2004 | 3 | 3 | 11 | 0 |
| R2007 | 3 | 3 | 11 | 0 |
| R2010 | 3 | 3 | 11 | 0 |
| R2013 | 3 | 3 | 11 | 0 |
| R2018 | 9 | 9 | 11 | 0 |

## 7ah. SPATIAL_FILTER (2026-10-26, #149)

§20.4.94 prints the clip of an xref or block insert as `BS` numpts, that many
`2RD` boundary points, an extrusion and an origin (`3BD` each), three `BS`
(the display boundary and the front and back clip switches), a `BD` distance
after each switch that is 1, and two matrices of 12 `BD`. The dispatcher had
no class-name arm for it, so the one record of each release was skipped.

The field list closes as printed on all 7 records, R14 to R2018. The spec
calls each matrix "double [4][3], column major order"; the values arrive as
three rows of four with the translation last. `sample_AC1018.dwg`'s inverse
block transformation reads `25.4 0 0 -1087.95`, `0 25.4 0 942.62`,
`0 0 25.4 0`, an inch-to-millimetre scale with a shift, and its clip boundary
transformation is the identity. They are kept in stream order, the order of
the DXF's twenty-four groups 40. Neither clip switch is on in the corpus, so
the two optional distances are never read there.

Against AutoCAD's DXF export (R2000 to R2018, matched by the handle in group
5) the point count, the boundary points, the extrusion, the origin, the display
boundary, both switches and all 24 matrix values equal the groups that carry
them: 234 values, none differing. R14 has no twin.

```sh
cargo run --release --example probe_spatial_filter -- ../dwg-corpus
```

| Release | Records | Close | Values compared | Differ |
|---|---|---|---|---|
| R14 | 1 | 1 | no twin | |
| R2000 | 1 | 1 | 39 | 0 |
| R2004 | 1 | 1 | 39 | 0 |
| R2007 | 1 | 1 | 39 | 0 |
| R2010 | 1 | 1 | 39 | 0 |
| R2013 | 1 | 1 | 39 | 0 |
| R2018 | 1 | 1 | 39 | 0 |

## 7ai. R13 (AC1012): the container, the walk, and where the bytes depart from the text (2026-10-26, #119)

Until this section the crate refused every AC1012 file at the version gate,
and the corpus held none. Twenty-two real R13 drawings are now pinned
(CLEANROOM.md, "Public corpus: the R13 drawings"): SketchUp's `Cabin.dwg`
and 21 sheet-metal, machined-part, foam and rail drawings of Sovol3d/SOVOL-ZERO.
AutoCAD can no longer save R13 and none of the 22 has a DXF twin, so everything
below is a **boundary** claim: a field list ends exactly on the record's own
data-stream boundary, or it does not. No value is compared with AutoCAD's, and
nothing here is marked `complete`.

### What R13 shares with R14, and the one thing that changed

`Version::R13` is a new variant; every place the crate tested "is R14" for an
R13-R14 branch (the `RL` object size after the EED chain or the graphics block,
`Isbylayerlt`, `Nolinks`, the R13-R14 INSERT, 3DFACE, TOLERANCE, LINE and
table-entry layouts) now asks `Version::is_r13_r14`. Nothing else in the
container needed code: the §3.2.6 locator list, the unframed object stream,
the §4.4 object map with absolute offsets, the §10.1 class list and the
object prologue are the R14 ones.

| | `Cabin.dwg` | the 21 Sovol drawings |
|---|---|---|
| locator records | 4 (the R13 C3 layout without record 4) | 5 |
| locator CRC XOR (§3.2.6) | 0x8101, the first count-4 file measured | 0x3CC4 |
| bytes 0x06-0x0C (§3.2.1) | `00 00 00 00 00 04 01` | `00 00 00 00 00 00 01` |
| code page (0x13) | 30 | 39 |
| image data (§14.2 sentinel at the §3.2.2 seeker) | at the end of the file, 0x2F9FC | right after the file header, 0x58 (overall size 18,623 in all 21) |
| §26 second header | at the end of locator 3, 53 bytes, as §3.2.6 says for C3 | the same |

`examples/probe_r13_container.rs` re-reads the framing independently of the
reader and checks, per file, the locator CRC and the sentinel after it, the
image sentinel, the §9 and §10.1 sections (both sentinels, the `RL`, the CRC
seeded 0xC0C1, and locator size = 38 + `RL`) and every §23.1 chunk's CRC with
the map ending on its locator. All six pass on all 22 files. No three-locator
(R13 C2) drawing is in the corpus, so that layout, and the PRE-R13C3 image
position, are untested.

The §9 header variables decode strictly on all 22, to the stated size. The
R13 rows of the table close `Cabin.dwg` as they are (195 slots, the R14
files' 199 less §9's four "R14+" shorts). The 21 Sovol headers read their R13
slots plausibly (the control-object handles are 3.1.06 to 3.1.0B, the
dictionaries 5.1.0D, 5.1.17, 3.1.0C) and then hold 84 bits: four BS of
`00` + `FFFF`, 18 bits each (72 bits), and 12 bits of padding. The four are
§9's "R14+" block, "unknown short (type 5/6 only) ... these do not seem to be
required", so §9 itself says a writer may omit them; this one writes -1 in all
four where AutoCAD's R14 files hold arbitrary values (`line_R14.dwg`: -32079,
7245, -8010, 30738), and writes 0 where AutoCAD writes 256 in the first short
(§9's "R13-R14 Only" unknown short). The 12 bits are the 4 bits that end the
byte holding the last slot (always `1010`; AutoCAD R14 writes `1111`) and one
whole byte more (`00` in 19 files, `52` and `7F` in the others): the Sovol
section size is `ceil(bits / 8) + 1` (403 to 404 bytes, 395 to 396), where
AutoCAD's is `ceil(bits / 8)`. The decoder reads an AC1012 header with the R14
rows when the R13 rows leave a whole byte or more, and lets that header end
one byte before its data does. That early end is a tolerance for this
writer's R13 variant, confined to AC1012: the Sovol drawings are not
AutoCAD-written (AutoCAD's R14 pads with `1111` and sizes the section
`ceil(bits / 8)`), so it is not AutoCAD's format and no other release gets it.
`Cabin.dwg` is untouched by either rule, and the end-inside-the-last-byte rule
for every other release is unchanged.

The §26 second header's head is verified on all 22 (its sentinel sits where
locator 3 ends; the self-address and `AC1012` read as printed). Its body
follows §26 on `Cabin.dwg` only: the locator copies agree with the first
header and 14 handle records follow, but the bytes after `AC1012` read
`00 00 00 00 00 04` and the four fixed bytes `18 30 01 04`, where §26 prints
`18 78 01 04` for R13. The 21 Sovol second headers do not follow the order §26
prints after the version string. The reader does not use this section.

### The walk

`DwgFile::open` now opens all 22. Every handle-map entry lands on a record
whose own handle repeats it (27,714 records, 0 mismatches, 0 skipped), the
records tile the file from the end of the class section to the start of the
object map with **0 bytes between them**, and every class reaches its declared
`num_objects` (`probe_class_census`). `tools/streaming/verify_walk.py` walks
`Cabin.dwg` and two Sovol drawings with the independent streaming grammar and
gets the same records as the legacy reader and as its own framing parse; the
same holds for the other 20 (`stream_walk`, each against `stream_walk --legacy`).

| source | records | decoded | skipped | errored |
|---|---|---|---|---|
| `Cabin.dwg` | 3,590 | 3,353 | 0 | 237 |
| Sovol, 21 drawings | 24,124 | 23,901 | 21 | 202 |

(Measured with main at 4ec356a merged in.) The 439 errors are ATTRIB and ATTDEF
(179 and 57, `Cabin.dwg`), the R13/R14 DIMSTYLE block (161, one in `Cabin.dwg`)
and LEADER (42, Sovol); the 21 skipped are one `SUN` class record per Sovol
file. Each error is a decoder that reached main with its R13-R14 branch tested
as `== Version::R14`, so R13 takes the R2000 branch and misses its boundary:
on R14 and every later release the same decoders close all of them. MTEXT is the
one that asked `is_r13_r14()`: all 3,968 MTEXT records of the Sovol drawings
error until it does, and close once it does.

### Where R13 departs from the text

1. **§23.1 chunk length.** "Each section is cut off at a maximum length of
   2032" is a limit on the *entries*: the big-endian size field counts its own
   two bytes, so a full chunk reads 2034 (§7ac found the same on ACadSharp's
   AutoCAD-saved `sample_AC1014.dwg`, 2033, and `sample_AC1015.dwg`, 2034).
   Seven of the 22 R13 drawings hold such chunks too (`Cabin.dwg` 2033; three
   Sovol at 2034, three at 2033), which the streaming walk's old bound of 2032
   rejected with `MapChunk`. The bound is 2034 on main; `verify_walk.py` now
   also walks `Cabin.dwg` and two Sovol drawings through the same bound.
2. **§2.8 and §2.10: BE and BT on R13-R14 are a bare `3BD` and `BD`.** The
   helpers read the R2000+ flag-then-value forms on every release, which agrees
   with the R13-R14 form only when the thickness is 0 and the extrusion is
   (0,0,1), the case every R14 file of the corpus is in. LINE `0x28` of
   `Cabin.dwg` (RL boundary 605): six BDs end at bit 469; read as BD + 3BD the
   record gives thickness 1.0, extrusion (−1, −3.0e-16, 0) and ends on bit 605;
   read as BT + BE it reads a reserved `11` BD tag and ends on bit 543. LINE
   `0x30` (boundary 413) gives thickness 80, extrusion (0,0,1), bit 413. 18 LINE
   and 2 POLYLINE_2D records of `Cabin.dwg` close that errored before;
   `read_bt_for` and `read_be_for` take the version, and LINE, POINT, CIRCLE,
   ARC, SOLID and POLYLINE_2D use them. No R14 or R2000 row of the baseline
   moved.
3. **§20.4.44 DICTIONARY: the unknown byte is "R14 Only".** R13 writes nothing
   between `Numitems` and the first name. All 88 R13 DICTIONARY records missed
   their `RL` boundary (by exactly 8 bits when the dictionary is empty); with the byte read
   only on R14 they all close.
4. **§20.4.47 LEADER: `Endptproj` is "R14+", not "R13-R14".** R13 has no end
   point projection. With the LEADER decoder as merged, all 42 Sovol LEADER
   records error when the three `BD`s are read on R13, and all 42 close when
   R13 skips them.
5. **The R13-R14 branches other decoders wrote as `== Version::R14`.** MTEXT
   and LWPOLYLINE are fixed here. TEXT, ATTRIB, ATTDEF, LEADER and the R13/R14
   DIMSTYLE block reached main as `== Version::R14`; with each read as
   `is_r13_r14()` (and point 4), **every one of the 27,714 R13 records decodes
   or is one of the 21 `SUN` records**: `Cabin.dwg` 3,590 decoded, 0 skipped, 0
   errored; the 21 Sovol drawings 24,103 decoded, 21 skipped (the `SUN` class),
   0 errored.
   Where a field is "R13-R14 Only" the two releases share it; no other R13
   difference turned up.

§9 prints a block of four unknown shorts (types 5 and 6 of the header
variables) as "R14+", so it is not R13's, and §20.4.58's text-area note ("first byte is
always 0 for R13 ... in R14 it is not a valid data start from byte 0") is a
difference in content, not in framing; neither is measured here, and both
belong to the decoders that read them.

### What is not measured

No AutoCAD DXF of an R13 drawing exists, so no R13 value has been compared. The
R13 files are not known to be AutoCAD's: the Sovol drawings carry a `SUN`
object, which §20.4 gives to R2007 and later, so something newer than AutoCAD
R13 wrote them. They are evidence of the format as it is found, not of what
AutoCAD R13 itself wrote. Reference closure (#84) is also open:
R13 and R14 records still count as `references_unchecked` (27,714 of 27,714).

Reproduce:

```sh
python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
cargo build --release --features dxf-reader --example coverage_report \
    --example probe_class_census --example probe_reference_closure \
    --example probe_r13_container --example probe_r13_entities
python3 -I tools/corpus/measure.py run --corpus ../dwg-corpus --bin target/release/examples --out r.json
python3 -I tools/corpus/measure.py check --results r.json
target/release/examples/probe_r13_container ../dwg-corpus/sketchup/Cabin.dwg ../dwg-corpus/sovol/*.dwg
python3 tools/streaming/verify_walk.py target/release/examples/stream_walk
```

## 7ae. R13 and R14 reference closure: the handle stream starts at the RL (2026-10-26, #84)

`probe_reference_closure` decides whether a class the walk reaches fewer of
than `AcDb:Classes` declares is a missing record or an over-declaring file: it
decodes every handle reference in every record's handle stream and checks the
target against the handle map. It was blind on R13 and R14, because nothing in
the object prologue says where the data stream ends: all 292 records of an R14
file, and later all 27,714 records of the 22 R13 drawings, counted as
`references_unchecked`.

§20.1 and §20.4.1 do say it, under "R13-R14 Only": an `RL` "size of object in
bits, not including end handles", after the EED chain of a non-entity object
and after the graphics block of an entity. `object::r13_r14_data_end` reads
`BS` type, `H` handle, the EED chain, for an entity the graphics-present flag
and any graphics, then the `RL`, and refuses a value that is not a bit
position inside the record after the end of its own prefix. The handle stream
runs from there to the end of the record, as on R2000 and R2004.

Whether a record is an entity decides whether the graphics flag is there. For
a fixed type code that is `ObjectType::is_entity`. For a record of a custom
class (type code 500 and up) it is the class's item class id: §10.1 gives
`0x1F2` to "classes which produce entities" and `0x1F3` to "classes which
produce objects", and all 321 class entries of the 30 R13 and R14 drawings of
the corpus carry one of the two (15 entity classes, 306 object classes), unlike
R2004+, where a vendor class has neither. The walker takes the class table from
`DwgFile::all_objects_lossy` (`ObjectWalker::with_classes`); without it a
custom-class record stays unchecked rather than guessed.

Result on the corpus (`measure.py`, the `refs`, `unanswered`, `hard` and
`no_boundary` columns of `tools/corpus/baseline.tsv`):

| release | drawings | references decoded | unanswered | hard | records with no boundary |
|---|---|---|---|---|---|
| R13 | 22 | 72,799 | 0 | 0 | 0 (was 27,714) |
| R14 | 8 | 9,213 | 0 | 0 | 0 (was 3,297) |

So the walk is now closed on all 74 pinned drawings. The check is not vacuous:
a handle stream read from the wrong bit decodes handles that do not exist. The
same walk with the boundary moved one bit later leaves 38 of 295 references
unanswered (23 hard) on `line_R14.dwg`, 583 of 1,725 (28 hard) on
`sample_AC1014.dwg`, 711 of 4,437 (9 hard) on `Cabin.dwg` and 140 of 874 (1
hard) on `JXHMP4602-11083-b 铁片.dwg`, where the right boundary decodes 789,
3,657, 7,866 and 1,304 references with none unanswered.

## 8. Write pipeline (current scope)

The inverse pipeline is partially shipped. Stage-1 (per-section
compression + framing) works today:

```
 caller provides: section_name, decompressed_bytes, page_offset
     │
     ▼
 lz77_encode::compress(bytes)      ──> LZ77 stream (literal-only)
     │
     ▼
 section_writer::build_section:
   - compose 32-byte header (page_type, section_number, sizes, checksums)
   - apply Sec_Mask XOR at target page_offset
   - pad to 32-byte boundary
     │
     ▼
 Built section bytes (drop-in to a page buffer)
```

Stages 2-5 — rewriting Page Map, Section Info, system pages, and
the 0x80-byte file-open header — are scaffolded in `file_writer.rs`
with an explicit roadmap. The current `WriterScaffold` is sufficient
for round-trip testing individual sections and will be the Stage 1
input to a full `DwgFile::to_bytes()` once Stages 2-5 are completed.

## 9. Error handling philosophy

- **Every parser takes `&[u8]` and returns `Result<T, Error>`.**
  No panics on malformed input (the test suite includes fuzz-style
  truncation + bit-flip inputs that exercise every error path).
- **Defensive caps** on claimed counts: dictionaries ≤ 1M entries,
  XRECORDs ≤ 16 MB, spline control points ≤ 1M. These are orders
  of magnitude above any realistic drawing; their purpose is to
  bound the work a malformed file can force.
- **`#![forbid(unsafe_code)]`** in `lib.rs` — all code is safe Rust.
  Reed-Solomon + LZ77 + GF(256) all implemented without `unsafe`.

## 10. Test strategy

Evidence comes from real executables run on real files. New work is
shown on the corpus, not with new unit tests; the suites already in
the tree stay and must keep passing.

1. **Self-validating decoders.** Every entity and object decoder must
   end exactly on its record's data-stream boundary (§7i), so a wrong
   field list surfaces as an error on any file that contains the
   type.
2. **The real-file corpus.** `tests/samples.rs`, `block_names.rs`,
   `corpus_roundtrip.rs` and `r2013_entity_values.rs` read the 19-file
   corpus at `../../samples` relative to the crate and skip silently
   when it is absent. `examples/coverage_report.rs` measures decoded,
   skipped and errored records per file; its numbers are the ones the
   README quotes.
3. **CI gates on the committed canonical fixtures**
   (`tests/fixtures/canonical/`, generated deterministically by
   `examples/build_fixtures.rs`): the fixtures must match the
   generator, the coverage report must stay above its floor, and the
   class-census and reference-closure probes must pass. CI does not
   read the real corpus yet (#95).
4. **Property, golden and fuzz checks.** `tests/proptest_roundtrip.rs`
   round-trips the bit-level primitives, `tests/integration_svg_goldens.rs`
   pins SVG output, and `fuzz/` runs nightly against every byte-level
   attack surface.
5. **Streaming E2E.** `tools/streaming/verify.py` drives the built
   `stream_header` example in separate processes under generated chunk
   schedules (docs/streaming/README.md).

`cargo test --release --all-features` must pass before a PR merges.

## 11. Legal posture

DWG is a trademark of Autodesk, Inc. This crate is implemented from
the Open Design Alliance's freely-redistributable *Open Design
Specification for .dwg files* (v5.4.1). Executable code from the
following sources was not consulted or imported:

- Autodesk's proprietary DWG SDKs (RealDWG, ObjectARX, ObjectDBX).
- The Open Design Alliance's `Teigha` / Drawings SDK.
- LibreDWG or any other GPL-licensed DWG implementation.

One scoped exception is documented in `CLEANROOM.md`: algorithm-
description comments (not executable code) in the MIT-licensed
[ACadSharp](https://github.com/DomCR/ACadSharp) were consulted to
resolve one LZ77 offset-encoding spec ambiguity. Every cross-check
is annotated at the affected source file.

17 U.S.C. § 1201(f) (DMCA interoperability exception), Article 6 of
EU Directive 2009/24/EC (Software Directive), and the line of U.S.
fair-use cases from *Sega v. Accolade* (9th Cir. 1992) through *Sony
v. Connectix* (9th Cir. 2000) all support independent file-format
reverse engineering for interoperability. Nothing in this repository
is offered as legal advice; see `NOTICE` for the fuller reference
set.

The term "clean-room" as used elsewhere in this project refers to
the project's solo-developer, spec-only, no-reference-source
discipline — it is not a formal two-team clean-room protocol in the
IBM-BIOS sense. The scope is defined precisely in `CLEANROOM.md`.

## 2026-10-26 — LAYOUT's plot flags, measured against AutoCAD's DXF

The DXF value oracle (#99) pairs every DWG record with the record of the
same handle in AutoCAD's own DXF export of the drawing and compares their
values. On the LAYOUT records that set the R2013+ `has AcDs binary data`
bit, it found the "data-store block" `objects::acad_layout` read after the
flag to be a misreading that the data-stream boundary cannot catch.

`plot_layout_flags` is the first `BS` of the §20.4.84 plot-settings
block. A `BS` whose value needs 16 bits is written as the code `00` and an
`RS`, 18 bits. The old reading took the first 16 of them as two `RC`s and
the last two, `10`, as a `BS` of value 0: the same 18 bits, so both
readings close. The export decides it:

| Record | DXF 70 (plot flags) | Full-form `BS` bits | Old reading | Old `plot_layout_flags` |
|---|---|---|---|---|
| `Model` (`sample_AC1032.dwg`, `sample_AC1027.dwg`, `geoloc.dwg`, handle `22`) | 11952 = `0x2EB0` | `00` `B0 2E` | `RC 0x2C`, `RC 0x0B`, `BS 10` | 0 |
| paper layouts (handles `59`, `5E`, `25A` of the two samples) | 688 = `0x02B0` | `00` `B0 02` | `RC 0x2C`, `RC 0x00`, `BS 10` | 0 |

Reading no bits after the flag decodes 688 and 11952 on all nine records,
equal to the export, and every other LAYOUT field stays equal. Coverage
is unchanged (`coverage_report` totals identical on both corpora), since
the two readings consume the same bits. The flag is followed by nothing
on objects, as on the three ACIS entities that set it.

Reproduce with `cargo run --release --features dxf-reader --example
dxf_oracle -- <corpus-dir>` over the pinned corpus of #225; the LAYOUT
table's `plot_layout_flags` row reads `=` on every release.
