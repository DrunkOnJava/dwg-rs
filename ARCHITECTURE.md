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
(handles to MATERIAL and VISUALSTYLE objects, which OBJECTS does not write yet,
so the reference would point at nothing), and every name for a symbol record the output does not
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

## 7ai. MULTILEADER before R2010 (2026-10-26, #229)

§20.4.48 says MULTILEADER was introduced in version 21 (R2007), and the
crate decoded it only from R2010. AutoCAD 2025 writes the record into
every release it saves, as a class on R14 to R2004, so ACadSharp's
saves of one drawing (#225) hold its 15 multileaders on R14, R2000,
R2004 and R2007 too. All 60 were counted as skipped.

§20.4.48 and its embedded §20.4.86 MLeaderAnnotContext print every
version branch the earlier releases need:

| Where | R2010+ only | Up to R2007 only |
|---|---|---|
| the record's start | `BS 270` class version | |
| each leader line | `BS 170`, `CMC 92`, `BL 171`, `BD 40`, `BL 93` | |
| each leader root | `BS 271` | |
| the context's end | `BS 273`, `BS 272` | |
| after `B 293` is annotative | `BS 271`, `BS 273`, `BS 272` (and the 17 unnamed bits of §7g) | `BL` arrow-head count with a `B` each, `BL` block-label count with a `TV`, `BS`, `BD` each, then `B 294`, `BS 178`, `BS 179`, `BD 45` |

Two rules from elsewhere in the spec also apply: before R2004 a `CMC` is
the bare `BS` index (§2.11), and before R2007 the `TV`s are inline. Read
that way, all 105 MULTILEADER records of the public corpus close, R14 to
R2018, with no change at all to how R2010 and later are read.

`examples/probe_mleader.rs` compares the decoded values with the
`AcDbMLeader` group of the DXF twin, part by part: the context, each
leader root and line, and the entity's own fields.

| Release | Records | Close | Values compared | Differ |
|---|---:|---:|---:|---:|
| R14 | 15 | 15 | no twin | |
| R2000 | 15 | 15 | 1290 | 0 |
| R2004 | 15 | 15 | 1365 | 0 |
| R2007 | 15 | 15 | 1365 | 0 |
| R2010 | 15 | 15 | 1305 | 0 |
| R2013 | 15 | 15 | 1305 | 0 |
| R2018 | 15 | 15 | 1305 | 0 |

Colours are compared from R2004 on, so R2000 has fewer. The `-R2007`
tail's four scalars (294, 178, 179, 45) are compared on R2000 to R2007.

The releases do not store identical values. Record `0xB27` holds its
content base point at x 527.42 with left/right attachments 4/0 on R2007
and earlier, and at x 556.64 with 0/8 on R2010 and later. Each release's
DXF agrees with its own record, so AutoCAD converts the leader when it
saves the older format, and the probe compares each release with its
own twin rather than across releases.

```sh
cargo run --release --example probe_mleader -- sample_AC1018.dwg sample_AC1018_ascii.dxf
```

## 7aj. R2007's CRCs, checksum and page sizes, as AutoCAD writes them (2026-10-26, #205)

§5.2 to §5.4 give R2007 a lot of integrity data and say how it was made, not
what each value covers or how its seed is built: the legacy reader checks none
of it (§"What does not run" in §7k: "the crate has no 64-bit CRC"). The
streaming walk (`src/streaming/r2007*.rs`) verifies all of it before it hands
out a byte, so each reading below was measured first, on the eight R2007
drawings of the pinned corpus (seven nextgis saves from AutoCAD 2016 and
ACadSharp's `sample_AC1021.dwg` from AutoCAD 2025), by
`tools/streaming/probe_r2007_checks.py`, which uses only the independent
reader `tools/streaming/r2007_ref.py` and prints, for each field, how many
agree with the reading and how many with the nearest wrong ones (the other CRC
flavour, the other seed update, plain byte order).

| Field | Reading that holds | Agree | Any other reading |
|---|---|---|---|
| File header, compressed data CRC (decoded block `0x10`) | normal CRC, `UpdateSeed2(0, length)`, over the `ComprLen` bytes | 8 of 8 | 0 |
| File header, header CRC (`0x108`) | normal CRC, `UpdateSeed2(0, 0x110)`, over the header with the CRC field zeroed | 8 of 8 | 0 |
| File header, checking sequence CRC (decoded block `0x00`) | normal CRC, `UpdateSeed1(0, 16)`, over the key and the key rotated by itself | 8 of 8 | |
| File header, check data (`0x3D8`, five values) | both CRCs of the eight-word buffers §5.2.1.1.5 builds from the two random values; seeds `~random2` (normal) and `~random1` (mirrored), no seed update | 8 of 8 | |
| System page (page map, section map), stored bytes | mirrored CRC, `UpdateSeed1(seed, length)`, `seed` the ten bits of the header's encoded seed | 16 of 16 | 0 |
| System page, decompressed bytes | the same reading over the decompressed bytes | 16 of 16 | 0 |
| Data page, stored bytes | mirrored CRC, `UpdateSeed1(CrcSeed, compressed size)`, over the first `compressed size` bytes after the codewords are de-interleaved | 137 of 137 | 0 |
| Data page, checksum | §5.4.1's checksum, seed 0, over the decompressed bytes | 137 of 137 | |
| Syndromes of every codeword (§7l) | all zero | 24 header, 34 page map, 129 section map, 4,425 data | |

Both CRCs take eight-byte blocks in the order `6 7 4 5 2 3 0 1` and a 1 to 7
byte remainder in §5.12's table order (`2 3 0 1 4 5 6` for seven); the
checksum takes its bytes in the same orders. Taken in plain byte order none of
the CRCs matches (0 of 137 data pages), which is the part of §5.12 that is easy
to read past. Both tables follow from one polynomial each: ECMA-182
(`0x42F0E1EBA9EA3693`) most significant bit first for the normal CRC, and
`0x95AC9329AC4BC9B5` shifting the other way for the mirrored one; the tables
§5.12 prints are reproduced exactly.

What is not protected, and so not checked: bytes `0x2FD` to `0x3D7` of the file
header block (random padding after its three codewords; a flipped byte there
changes nothing), and the padding after the stored bytes in a data page's last
codeword, which is not zero (0 of 4,425 codewords' padding is) because §5.13.2
fills it from the random encoder. The repeated copies of a system page's data
are equal to the first (16 of 16) and protected by their own codewords; the walk
reads only the first, and checks the others' syndromes in its first pass.

Three facts about page sizes the legacy reader never needed, and a bounded walker
cannot ignore:

- **A data page is not bounded by its section's maximum size.** Every section of
  `sample_AC1021.dwg` and the nextgis drawings names `0xF800` as its maximum,
  yet 16 pages of `AcDb:AcDbObjects` decompress to more (up to 70,912 bytes)
  and the largest has a compressed size of 68,365 bytes: 273 codewords. A
  fixed buffer sized to the maximum would refuse real files, so the walker
  holds no page: it gathers one codeword at a time (the stride of the
  interleave is the page's codeword count, so a codeword's 255 bytes are not
  adjacent) and decodes through a 64 KiB window, the largest distance §5.10
  can encode.
- **A page's section-map "size" is its decompressed size only for some pages**
  (34 of 137), so it is carried and printed but not relied on; its page map
  entry is the codewords' span rounded up to `0x20` (98 of 98 Reed-Solomon
  pages), and for every page (137 of 137) at least the span.
- **Every section's pages end exactly at its data size** (103 of 103), so a
  section needs no tail of zeros; a gap before a page (§5.2: "a page that
  contains zeroes only is not written") is a hole, as in the legacy reader.

Not measured: a header whose `ComprLen` is negative (stored), a non-zero
`CrcSeed` (every pinned header has 0, so the seed plumbing is checked only
against files `r2007_ref.build` writes from the spec's text), and any file
with an encrypted section.

## 7ak. dwg-to-dxf: the TABLES section (2026-10-26, #126)

`src/dxf_tables.rs` writes the VPORT, LTYPE, LAYER, STYLE, VIEW, UCS, APPID,
DIMSTYLE and BLOCK_RECORD tables from the decoded symbol-table records, each `TABLE` block
under its control object's handle with its records under their own and owned by
the table, in the group order of AutoCAD's exports (VPORT, LTYPE, LAYER, STYLE,
VIEW, UCS, APPID, DIMSTYLE, BLOCK_RECORD). The header then names a layer, text
style, linetype or block only if a written table defines it
(§7s, "dwg-to-dxf writes it back"). `examples/probe_dxf_tables.rs`
converts each drawing that has an AutoCAD export, reads the output back and pairs
every record written with the export's record of the same handle; the Corpus job
runs it.

| Table | Written | Paired | Groups written | Agree | Relation | Disagree | Extra | Groups omitted | Records omitted |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| VPORT | 42 | 42 | 2199 | 2197 | 2 | 0 | 0 | 137 | 0 |
| LTYPE | 156 | 156 | 1146 | 1146 | 0 | 0 | 0 | 4 | 0 |
| LAYER | 156 | 156 | 964 | 964 | 0 | 0 | 0 | 496 | 0 |
| STYLE | 108 | 108 | 972 | 972 | 0 | 0 | 0 | 232 | 0 |
| VIEW | 6 | 6 | 190 | 190 | 0 | 0 | 0 | 4 | 0 |
| UCS | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| APPID | 393 | 393 | 786 | 786 | 0 | 0 | 0 | 0 | 0 |
| DIMSTYLE | 108 | 108 | 1449 | 1445 | 4 | 0 | 0 | 320 | 0 |
| BLOCK_RECORD | 270 | 270 | 1134 | 1131 | 3 | 0 | 0 | 137 | 0 |
| All | 1239 | 1239 | 8840 | 8831 | 9 | 0 | 0 | 1330 | 0 |

Every group written has the export's code and value, over the 42 drawings of the
public corpus that have an AutoCAD export of the same release (R2000 to R2018).
What is omitted has a reason the probe checks, and an unexplained omission fails
it: the extended data (1000 to 1071) of a record, which the decoder does not read;
a true-colour layer's 420 and the 62 beside it, a viewport's 63 beside its 421
and a dimension colour (176 to 178), because the DWG stores no index for a true
colour; a layer's colour name
(430); a record the export registers again under another handle (an APPID, a
layer, a block), which pairs by name; and the
`{BLKREFS` bracket of a block, because ENTITIES does not carry the INSERT handles
it lists. `ACAD_SPLINE_DATA` is not written: AutoCAD's export omits that
application on all four drawings that hold it. UCS is written
as AutoCAD's empty table when the drawing has no UCS record, which is every
drawing of the corpus; a drawing with UCS records gets no UCS table, because the
decoder has never read a real record (§20.4.62 lists a second orthographic `BS`
that `tables::ucs` does not read).

A LTYPE with a text or shape dash is written with its shape number (75; 0 for a text
dash, whose `shape_number` is an offset into the text area, not a shape: the export
writes 0 on all six text dashes of the corpus, which all sit at offset 0, so the
corpus does not tell the offset from 0 and DXF's 75 settles it), style
handle (340), scale, rotation, offsets and text (44 to 46, 50, 9). The style is the
handle after the xref block's, one per dash; the text is the NUL-terminated string at
the dash's `shape_number` bytes into the linetype's text area (256 bytes to R2004,
512 of UTF-16 from R2007), decoded as the crate decodes every pre-R2007 string (lossy
UTF-8, not the drawing's code page: #370; the six corpus text dashes read `GAS`, all
ASCII), measured at offset 0 only. The DWG sets bit 8 of a text
dash's flags on every release; AutoCAD's export writes it from R2013 and clears it
before, so it is cleared for targets before R2013. All 156 LTYPE records pair, the
four complex ones of each release included.

### The handle stream of a table entry

`tables::handles::refs` reads what the other decoders leave: the handle stream.
Every entry starts with the table's control object (soft pointer, the owner), one
soft pointer per reactor (the `BL` of the common object data), and the extension
dictionary (always on R2000, unless the `XDic Missing Flag` is set from R2004).
The entry's own handles follow, as §20.4.x lists them, and this is what was
measured against the exports' handle groups:

- **LAYER**: the xref block (NULL), the plot style (390), from R2007 the material
  (347), the linetype (6, written by name) and, from R2013, a visual style (348,
  written as `348 0` when NULL). That is 3, 4 and 5 handles on R2000 and R2004,
  R2007 and R2010, R2013 and R2018.
- **VPORT**: the xref block, then from R2007 the background (332), visual style
  (348) and sun (361), then the named UCS (345) and base UCS (346): 3 handles on
  R2000, 6 from R2007. Only 348 is ever set in the corpus.
- **DIMSTYLE**: the xref block, the text style (340), the leader block (341), the
  two arrowhead blocks (342 to 344) and, from R2007, the dimension linetype and
  the two extension line linetypes (345 to 347): 6 handles to R2004, 9 from
  R2007, a NULL one left out.

- **VIEW**: as VPORT's, the xref block, then from R2007 the background, visual
  style (348) and sun, then the named and base UCS. A view is the one record written
  with the view's associated UCS (72, 110 to 112, 79, 146) and, from R2007, the
  camera-plottable flag (73); a record from R13 or R14, which store neither a render
  mode nor a UCS, gets no 281 or 72.
- **BLOCK_RECORD**: a NULL, the `BLOCK` entity, on R2000 the first and last entity
  (unless an xref), on R2004 and later the owned entities (their count is in the
  data stream), the `ENDBLK` entity, one handle per non-zero `RC` of the insert
  count, and last the layout (340, `0` for a block that is not a layout). A list
  whose length is not what that order gives is not read. The layout handle equals
  the export's 340 on all 270 records.

### DIMSTYLE: what AutoCAD leaves out

`tables::dimstyle::decode_with_vars` returns every variable of the record in
stored order by AutoCAD's name (the 15 fields of `DimStyleEntry` are unchanged).
AutoCAD writes a DIMSTYLE group only when its value differs from the imperial
default, and the defaults are the ones in `DIM_GROUPS`: measured by pairing the
decoded value of each variable with the export's record of the same handle over
the 108 records, a variable the export omits has the same value on every record
that omits it, and a record whose value differs carries the group (`DIMSCALE`
1.0, `DIMASZ` 0.18, `DIMEXO` 0.0625, `DIMDLI` 0.38, `DIMDEC` 4, `DIMTAD` 0,
`DIMTIH` and `DIMTOH` 1, `DIMJOGANG` 45 degrees, and so on). A colour is written
as its index (176 to 178; the fill colour as a second `70`) unless it is ByBlock.
The `DIMFIT` slot of §20.4.68 is the group `DIMATFIT` (289) on every release.
The table header carries `100 AcDbDimStyleTable`, a `71` and a `340` per handle
the control object lists after its `numentries` dimstyles (non-NULL ones), and
the probe checks that header against the export's on every drawing.

A handle group is written only when the output defines the object it names, under
its own type: the OBJECTS section writes dictionaries and plot style placeholders,
so 390, 360 and the xdictionary bracket are written (a NULL as `0`). It does not write
a layout, a material or a visual style yet (§7aw lists the kinds it does),
so a block's 340 (when it names a layout), a layer's 347 and 348 and a
viewport's 348 are left out, as the header leaves out the equivalent `$CMATERIAL`
and visual style names. An arrowhead block is named once BLOCKS writes it, which it
now does for every block record (`$DIMBLK` and its family, a DIMSTYLE's 341 to 344).
The entities that react to a record and the INSERTs of a block (`{ACAD_REACTORS`,
`{BLKREFS`, 331) are named too, when every one of them is written: the tables are
built twice, the second time knowing the handles of the entities and block markers
written, so a record whose reactors or inserts include an entity nothing writes yet (a
DIMENSION) has no such bracket. The probe lists each omission with its reason.

### The order of a table

Each table lists its records in the order its control object does: the `n` handles
after the `BL` numentries, except that the LTYPE control (ByBlock, ByLayer) and the
BLOCK control (model space, paper space) hold their two built-in records after the
entries and AutoCAD's export writes those first. A BLOCK_RECORD's groups run name,
layout (340), preview bitmap (310), `{BLKREFS`, then insert units, explodability and
scaling (70, 280, 281); a DIMSTYLE's run in ascending code order, the fill colour (a
second 70) after the fill mode (69) and the lineweights (371, 372) after the handles.
The tables probe checks both orders against the export's, record by record and
group by group.

### What the exports do that is not the DWG's doing

Three differences are relations the probe names, not disagreements:

- The export of `geoloc.dwg` is a later save (its `$TDUUPDATE` is 1.94 days later,
  `$HANDSEED` 0x249 against 0x243), and the `*Active` viewport of the later save has a
  different view height (40) and centre (22).
- AutoCAD loads the dynamic block `my-dynamic-block` of the R2000, R2004 and R2007
  saves as the anonymous block `*U17`; the R2010 to R2018 saves keep the name.
- The R2000 save holds an extension dictionary on three true or book colour
  layers (`Layer_true_color`, `Layer_color_book`, `color_125_33_79`) that
  AutoCAD's export drops. They are written, with the dictionary, which OBJECTS
  writes.

The probe also prints the numbers per release.

## 7al. The entity colour's true colour and transparency words (2026-10-26, #126)

§2.11 stores an R2004+ entity colour as a `BS` whose high byte flags what
follows it: bit 0x80 a `BL` with the true colour (`0xC2RRGGBB`), bit 0x40 a colour
book handle (the RGB and name are then in the AcDbColor object), bit 0x20 a `BL`
with the transparency. `read_common_entity_data` read both `BL`s and dropped
them, taking the transparency first. Two `BL`s are the same 32 bits either way,
so the order moved no cursor and no decoded count, and nothing showed it until
the words were kept: `CommonEntityData` now carries `color_raw`, `color_rgb` and
`color_alpha`, with the RGB word first, as §2.11 lists them.

`examples/probe_entity_colour.rs` pairs every entity of a drawing with the
record of the same handle in AutoCAD's own export of it and compares the colour
index (62, absent for BYLAYER, left out where the export has a true colour:
AutoCAD derives a nearest index the DWG does not store), the true colour (420),
and the transparency (440). The Corpus job runs it. An entity whose common data
does not read is a failure, named by drawing and handle, not a skipped record, and a
run that reads and pairs no entity prints `NOT MEASURED`; `PROBE_ENTITY_COLOUR_TRUNCATE`
cuts every record short to show it (2,609 entities named, exit 1).

| Release | Entities paired | Index | Agree | True colour | Agree | Colour book | Transparency | Agree | Both words | Agree, RGB first | Agree, exchanged |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| AC1015 (2000) | 437 | 437 | 437 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| AC1018 (2004) | 437 | 403 | 403 | 33 | 33 | 1 | 148 | 148 | 14 | 14 | 0 |
| AC1021 (2007) | 437 | 403 | 403 | 33 | 33 | 1 | 148 | 148 | 14 | 14 | 0 |
| AC1024 (2010) | 437 | 403 | 403 | 33 | 33 | 1 | 148 | 148 | 14 | 14 | 0 |
| AC1027 (2013) | 437 | 403 | 403 | 33 | 33 | 1 | 148 | 148 | 14 | 14 | 0 |
| AC1032 (2018) | 394 | 360 | 360 | 33 | 33 | 1 | 148 | 148 | 14 | 14 | 0 |

The order matters only for an entity that carries both words, and the corpus
has 14 of them in each R2004+ release: read RGB first, all 14 match the export;
with the two words exchanged (the transparency first), none does. One of them is
the LINE `9D4` of `sample_AC1032.dwg`, whose export reads `62 183`,
`420 8404418` and `440 16777216` where the stream holds `0xC2802142` and then
`0x01000000`: read the other way round it would be a colour `0x01000000` with
the transparency `0xC2802142`. The colour book colour (one entity, flag 0x40) is
counted and not compared, since its RGB is not in the entity. R13 and R14 colours
are the bare index, and the probe skips AC1014 drawings.

## 7am. R2007's object records are R2000's (2026-10-26, #205)

The streaming object walker for R2007 (`src/streaming/r2007_objects.rs`) frames
the `AcDb:AcDbObjects` section of an `AC1021` file into records without the
`AcDb:Handles` map, exactly as §7ac's walker does for the R2004 family, and it
needed one answer first: which record layout. R2007 is neither R2004 (§7ac's
`RL`-sized record, which it shares) nor R2010 (the `MC` handle-stream size and a
two-bit-tagged type), and the legacy walker's comment that "the R2004 and R2007
corpus files tile their object stream exactly" had never been counted. It was,
on every object section of the eight R2007 drawings of the pinned corpus (seven
nextgis saves from AutoCAD 2016 and ACadSharp's `sample_AC1021.dwg` from AutoCAD
2025), with an independent parse (`tools/streaming/verify_objects.py`'s framer
over `r2007_ref.py`'s section bytes):

| Property | `AC1021` |
|---|---|
| Records walked | 2,237 (186, 186, 198, 186, 185, 185, 186 in the seven nextgis drawings, 925 in `sample_AC1021`) |
| The section opens with `CA 0D 00 00` (an `RL` of 0x0DCA), as R2004's does | 8 of 8 drawings |
| Records (`MS`, `MS` bytes of payload, two-byte CRC) end exactly at the section's size, with no padding and no zero-length trailer | 8 of 8 |
| The CRC (§2.14.1, seed `0xC0C1`) over the `MS` and the payload equals the two bytes after the payload | 2,237 |
| The (handle, offset) pairs of the `AcDb:Handles` map, sorted by offset, equal the records' handles and starts | 8 of 8 drawings, 2,237 records |
| The same bytes read as R2010 records (an `MC` after the `MS`, a two-bit-tagged type) | the first record's CRC fails in 8 of 8 |

So an R2007 record is the R2000 record (`RecordFormat::R2000`: a `BS` type, an
`RL` size in bits, then the handle), and the framer needed no new format. The
`RL` and the type are read from the payload's first 16 bytes as before and the
record is committed only after the CRC. The legacy reader reads the same bytes
the same way, and the streaming walker's records equal its (handle, offset, `MS`,
type, payload length, payload CRC-32) on all 2,237.

Two facts about the pages that carry the records, which a bounded walker cannot
ignore. The section's pages arrive in the order its section map lists them (a
rising `data_offset` is enforced by the file walk, which looks each page up in
the page map, §7aj), so unlike the R2004 family's page map the order of a page in
the file does not matter and no out-of-order page can reach the framer; a stretch
of the section that no page covers arrives as a `Hole` step, which the object
walker refuses (`ObjectsOrder`) instead of framing zeros. And records do not
respect pages: 34 of the 2,237 records straddle a page boundary (two in each
nextgis drawing, twenty in `sample_AC1021`, whose 21 object pages hold up to
70,912 decompressed bytes), the largest record takes 150,151 bytes of the section (`sample_AC1021`),
and the framing carries across all of them unchanged.

Not measured: an R2007 drawing larger than the 1.06 MB `sample_AC1021` (none is
pinned or available locally), a section the file lists in a different page order
(not a possible layout, per the above), and an encrypted section, which the file
walk refuses.

## 7an. dwg-to-dxf: the ENTITIES section (2026-10-26, #126)

`src/dxf_entities.rs` writes the entities of model space and of the active paper
space, and the children of a POLYLINE (its VERTEXes and SEQEND), from the decoded
records: 17 kinds so far (LINE, POINT, CIRCLE, ARC, ELLIPSE, SOLID, RAY, XLINE,
3DFACE, TEXT, ATTRIB, INSERT, LWPOLYLINE, MTEXT, POLYLINE in its 2D, 3D and polyface
forms, VERTEX, SEQEND). An INSERT, with the ATTRIBs and SEQEND under it, is written
only when BLOCKS (§7ar) writes the block it names (the blocks written are decided
first; an INSERT of any other is counted as `INSERT (its block is not written)`), so
no output holds a reference to a block it lacks. Any other kind, and an entity whose fields do not all
decode, is counted in a `999 skipped` comment. `examples/probe_dxf_entities.rs`
converts each drawing that has an AutoCAD export, reads the output back and pairs
every entity written with the export's record of the same handle; the Corpus job runs
it, and expects every kind `dxf_entities::kinds` lists for the target release of
every export record, whatever the output holds.

| Kind | Written | Paired | Groups written | Agree | Disagree | Extra | Groups omitted |
|---|---:|---:|---:|---:|---:|---:|---:|
| 3DFACE | 6 | 6 | 78 | 78 | 0 | 0 | 0 |
| ARC | 11 | 11 | 77 | 77 | 0 | 0 | 0 |
| ATTRIB | 23 | 23 | 223 | 223 | 0 | 0 | 16 |
| CIRCLE | 53 | 53 | 271 | 271 | 0 | 0 | 38 |
| ELLIPSE | 11 | 11 | 143 | 143 | 0 | 0 | 0 |
| INSERT | 36 | 36 | 293 | 293 | 0 | 0 | 0 |
| LINE | 161 | 161 | 1377 | 1377 | 0 | 0 | 6 |
| LWPOLYLINE | 131 | 131 | 810 | 810 | 0 | 0 | 42 |
| MTEXT | 25 | 25 | 295 | 295 | 0 | 0 | 89 |
| POINT | 46 | 46 | 243 | 243 | 0 | 0 | 4 |
| POLYLINE | 12 | 12 | 84 | 84 | 0 | 0 | 0 |
| RAY | 6 | 6 | 42 | 42 | 0 | 0 | 0 |
| SEQEND | 24 | 24 | 24 | 24 | 0 | 0 | 0 |
| SOLID | 6 | 6 | 83 | 83 | 0 | 0 | 0 |
| TEXT | 174 | 174 | 1140 | 1140 | 0 | 0 | 396 |
| VERTEX | 72 | 72 | 402 | 402 | 0 | 0 | 0 |
| XLINE | 6 | 6 | 42 | 42 | 0 | 0 | 0 |
| All | 803 | 803 | 5627 | 5627 | 0 | 0 | 591 |

Every group written has the export's code and value, over the 42 drawings that have
an AutoCAD export of the same release (R2000 to R2018): 803 entities, 5,627 groups.
The export holds 1,145 entities that are in the DWG: 336 are of the 16 kinds nothing
writes yet (DIMENSION, HATCH, MULTILEADER, VIEWPORT, MLINE, TOLERANCE, SPLINE, MESH,
ACAD_TABLE, 3DSOLID, REGION, SHAPE, WIPEOUT, LEADER, IMAGE, PDFUNDERLAY), which the
probe counts by kind; and six are R2018 records with an embedded object (five MTEXT, one
multi-line ATTRIB) that nothing writes yet. What an entity omits has a reason the probe checks:
extended data (1000 to 1071); a colour book colour's 420 and 430, which are in its
`AcDbColor` object; a material's 347, which OBJECTS does not write yet; reactors that are
entities not all written; an extension dictionary the export made itself (its handle is
at or above the DWG's `$HANDSEED`).

### The common data and handles of an entity

`entity_refs` reads the handle stream the entity decoders leave: the owner (only when
the entity mode is 0; mode 2 is model space and 1 paper space), the reactors, the
extension dictionary, a colour book colour, the layer, the linetype, the material, the
plot style and, from R2010, the visual styles, then the entity's own handles (a TEXT's
style, an INSERT's block, ...). Measured against the export:

- **R2000** has `Nolinks` clear on entities, which puts the previous and next entity
  handles (two relative soft pointers) before the layer; the common decoder read the bit
  and dropped it. From R2004 it is always set.
- From **R2007** the flag pair the common data calls `plotstyle_flag` then
  `material_flag` is read the other way round: the first names a MATERIAL (its handle,
  group 347; `1` is the ByBlock material, `0` ByLayer, which is not written), the second
  the plot style. Handle `0xDC0` of the R2007 sample is a class-505 MATERIAL object.
- The colour `BS` keeps the index in its low nine bits (256 is ByLayer, not written); a
  transparency (440) and a true colour (420) follow when the high bits announce them.
- The lineweight `RC` maps through the lineweight table; 29 (ByLayer) is not written and
  30 (ByBlock) is `-2`.

Group rules the export shows, each checked on every record of its kind: the linetype is
`ByBlock` or `Continuous` in mixed case, a LINE's thickness and an extrusion of `(0, 0,
1)` are omitted but an ELLIPSE always writes its 210; a TEXT writes its style only when
it is not `Standard`, and the vertical alignment (73) under its second `AcDbText`; an
ATTRIB writes the lock position (280) from R2007 and a version 280 before it from R2010;
an LWPOLYLINE writes the constant width (43) unless it has per-vertex widths, and is
not written at all when its flag word has a bit the decoder does not name (the
presence bits of §20.4.85 and the closed bit are named; PLINEGEN is none of them, no
record of the 143 LWPOLYLINEs of the corpus exports sets it, whose group 70 is 0 or 1,
and the spec does not say which bit it is); an MTEXT
writes its rectangle height (46) only from R2007 and splits its text in 250-character
`3` groups ahead of the `1` (a line feed, written `^J`, counts one character; no chunk
passes 2,000 written characters, a DXF line's limit, where `\U+XXXX` escapes before
R2007 widen it). An MTEXT of an R2018 target is not written: the export
appends an embedded object (101) with the column data, which is not written yet.
Text goes through the header's release-aware escaping (a newline is `^J`).

A layer of the `ENTITIES` section is the entity's own decoded layer; the placeholder
writer, which puts every entity on layer 0, runs only when no table is written.

The groups come in AutoCAD's order, which the probe checks record by record against the
export: the `AcDbEntity` groups run 67, 8, 6, 347, 62, 420, 430, 440, 60, 370 (measured
pairwise on every entity of the corpus; the linetype scale, 48, is placed after the colour
because no corpus record holds it with 440 or 60), then the groups of the entity's own
subclasses, and the entities in the order of the export, with a SEQEND after the last child
of its parent (the object stream may hold it before a child created later). A colour's
true-colour word (420) and transparency (440) are stored RGB word first and transparency
second: the common data read them the other way round, which closed every record on its
boundary (both are a `BL`) and was never compared.

## 7ao. dwg-to-dxf: the CLASSES section (2026-10-27, #126)

`src/dxf_classes.rs` writes a `CLASSES` section from the class table of `AcDb:Classes`,
for a target of R2000 or later (R12 has none; R14 is not measured), one `CLASS` record
per class in the table's order: the DXF name (1), the C++ name (2), the application
(3), the proxy capability flags as an unsigned 16-bit word (90; the table stores a
`BS`, so a flag word of `0x8401` reads as `-31743` signed), for a target of R2004 or
later the instance count (91), the was-a-proxy flag (280) and the entity flag (281:
item class id 0x1F2). Group 91 follows the target release; the count follows the
source: the table's from an R2004 or later drawing, and for an earlier one, whose table
stores none, the objects of the stream whose type code is the class number.
`examples/probe_dxf_classes.rs` pairs every class with the export's of the same DXF
name and compares groups and order, on the CLASSES section alone; the Corpus job runs it.

| Release | Written | Paired | Groups | Agree | Relation | Disagree | Export only |
|---|---:|---:|---:|---:|---:|---:|---:|
| AC1015 (2000) | 135 | 135 | 675 | 675 | 0 | 0 | 0 |
| AC1018 (2004) | 119 | 119 | 714 | 695 | 19 | 0 | 0 |
| AC1021 (2007) | 118 | 118 | 708 | 690 | 18 | 0 | 1 |
| AC1024 (2010) | 118 | 118 | 708 | 697 | 11 | 0 | 1 |
| AC1027 (2013) | 111 | 111 | 666 | 655 | 11 | 0 | 8 |
| AC1032 (2018) | 66 | 66 | 396 | 388 | 8 | 0 | 1 |

Three differences between AutoCAD's export and the table are measured and applied:
from R2004 the export does not list LAYOUT or ACDBPLACEHOLDER (newer releases hold them
as native types; both are listed in an R2000 export), and an R2000 export writes
`AcDbDataTable`'s record as `ACDBDATATABLE` where the table, and every later export,
say `DATATABLE`. The 67 relations are the instance count (91): the export counts the
objects it holds, among them those AutoCAD creates when it loads the file (more
DICTIONARYVARs, CELLSTYLEMAPs) or leaves out, so the DWG's own count differs by a few
on those classes. The export also holds 11 classes the table lacks (7 CELLSTYLEMAP and
4 DATATABLE, on the larger samples) that AutoCAD registers on load; they are listed and
not written. The probe also converts every drawing to every release from R2000: 3,934
CLASS records over 252 conversions, group 91 on each exactly when the target is R2004
or later. A drawing before R2004 has no count in its table, so a newer target gets the
count taken from the object stream; that count equals the table's own on 473 of the 544
classes of the R2004 and later drawings (the other 71 differ, as the export's counts
differ from the table's), and no corpus pair shows AutoCAD's own count for an R2000
drawing saved as a newer DXF, so that case is owed against an export. Not measured: an
R14 target.

## 7ap. Pre-R2007 text is in the code page the header names (2026-10-27, #370)

Before R2007 every `TV` is a run of single bytes in the drawing's code page, and
§3.2.5 says where the page is named: "Bytes at 0x13 and 0x14 are a raw short
indicating the value of the code page for this drawing file." Every reader of
such a string (the symbol-table entry names, TEXT, ATTRIB, ATTDEF, MTEXT,
TOLERANCE, IMAGEDEF, BLOCK, every non-entity object through
`objects::modern::read_tv`, the class table, the header variables and the
metadata sections) read the bytes as UTF-8 and kept U+FFFD for what is not, so a
byte above 0x7F that is not valid UTF-8 on its own (`0xB0`, `0xE9`, a Cyrillic
letter) was lost on the way in.

**What the specification gives, and what it does not.** The short and nothing
else: v5.4.1 prints no table of its values (the word "codepage" occurs in §3.2.5,
in the header dumps, in §22's template string and in the XRECORD and extended
entity data string layouts, where each string carries a code page of its own).
The values are therefore
learned as the project learns everything the specification omits (CLEANROOM.md,
2026-10-27): from AutoCAD's own output. `examples/probe_code_page.rs` pairs the
header short of each pre-R2007 drawing with the `$DWGCODEPAGE` of AutoCAD's DXF
export of it:

| Header short | Export's `$DWGCODEPAGE` | Drawings paired | Table row |
|---:|---|---:|---|
| 29 | `ANSI_1251` | 14 (seven nextgis saves each in AC1015 and AC1018) | `CodePage::Ansi1251`, Windows-1251 |
| 30 | `ANSI_1252` | 2 (`sample_AC1015.dwg`, `sample_AC1018.dwg`) | `CodePage::Ansi1252`, Windows-1252 |
| 29, 30 | none: AutoCAD's exports of the AC1014 drawings are R12 DXF or absent, and carry no `$DWGCODEPAGE` | 7 and 1 AC1014 drawings | the same two rows apply to them; no export confirms the short on R14 |

Every other short is `CodePage::Unknown(n)` and its text is read as before; the
probe fails when an export names a page the table has no row for, which is how a
row is added: in the pull request of the drawing and the export that name it.
Windows-1250, 1253 to 1258, 874, 932, 936, 949, 950 and the DOS and ISO pages
all decode through `encoding_rs` the day their numbers are measured; none is
named here because no pinned drawing has a number to name them by.

**One path.** `CodePage::decode` (`src/code_page.rs`) is the only place a
pre-R2007 byte string becomes a `String`, through `encoding_rs`' WHATWG tables
(`decode_without_bom_handling`: a leading `EF BB BF` is text here, not a byte
order mark). The records reach it through the page they were stamped with:
`ObjectWalker::with_code_page` puts `DwgFile::code_page()` on every
`RawObject::code_page`, `decode_from_raw` and `decode_from_raw_with_class_map`
put it in force for the decode with `with_code_page`, and the shared `TV` readers
read it back. The page is carried that way and not as a parameter because about
forty object decoders take only `(payload, body_start, inline_end, version)`;
a decoder called on a bare cursor sees `CodePage::default()` and reads UTF-8 as
before. `DwgFile` does the same around the header variables, the class table and
`AcDb:SummaryInfo`, `AcDb:AppInfo` and `AcDb:FileDepList`. A header variable
that leaves ASCII under a page the table does not name stays `Value::RawText`;
under a named one it is `Value::Text`. The writer's `$DWGCODEPAGE` takes its
name from the same table (`CodePage::dxf_name`).

**What the corpus holds.** No pre-R2007 string with a byte above 0x7F:

| Release | Drawings | Strings decoded | With a byte above 0x7F |
|---|---:|---:|---:|
| AC1014 (R14) | 8 | 3,418 | 0 |
| AC1015 (2000) | 8 | 2,846 | 0 |
| AC1018 (2004) | 8 | 2,812 | 0 |

The degree sign the twins show is not a counter-example: the MTEXT of
`sample_AC1015.dwg` that AutoCAD's export writes as `94\U+00B0` holds those nine
ASCII characters in the DWG too (`\U+00B0` is MTEXT's own escape for a character
outside the page, and the dimension text `\U+220545,6` is the same), so the old
and the new decode both return them and agree with the export. No decoded,
skipped or errored count and no oracle row moved (below).

**A patched copy.** With no such string to read, the path is proved on real
drawings altered in memory (nothing is written). For each of the 16 uncompressed
drawings (the eight AC1014 and the eight AC1015) and each kind of record that
holds a string, the first string of six bytes or more that sits once in the file
has its first four characters overwritten, at the bit they sit at, with
`E9 B0 C0 FC`, and the record is read back under the drawing's own header short,
under 29, under 30 and under 31, a number the table lacks:

| Header short | Reads as | Before the code page |
|---:|---|---|
| 29 | `й°Аь` + the rest (Windows-1251's own chart) | U+FFFD ×4 |
| 30 | `é°Àü` + the rest (Windows-1252's own chart) | U+FFFD ×4 |
| 31 | the bytes, as before, and counted as unknown | U+FFFD ×4 |

153 strings in 24 kinds of record, 612 of 612 reads as the table says:

| Kind of record | Strings | Reads right | `dwg-to-dxf` TABLES holds it |
|---|---:|---:|---:|
| AppId | 16 | 64 of 64 | 16 of 16 |
| DimStyle | 16 | 64 of 64 | 8 of 8 |
| Layer | 1 | 4 of 4 | 1 of 1 |
| Ltype | 16 | 64 of 64 | 16 of 16 |
| Style | 2 | 8 of 8 | 2 of 2 |
| Vport | 8 | 32 of 32 | 8 of 8 |
| BlockHeader | 1 | 4 of 4 | - |
| Block | 8 | 32 of 32 | - |
| Text, Attrib, Attdef | 2, 2, 2 | 8 of 8 each | - |
| MText, Tolerance | 2, 2 | 8 of 8 each | - |
| Dictionary, ACDBDICTIONARYWDFLT, DICTIONARYVAR | 16, 16, 1 | 64, 64, 4 of the same | - |
| LAYOUT, SCALE, IMAGEDEF, MULTILEADER | 2 each | 8 of 8 each | - |
| ACDBDETAILVIEWSTYLE, ACDBSECTIONVIEWSTYLE | 16 each | 64 of 64 each | - |
| HATCH, Hatch | 1 each | 4 of 4 each | - |

The 51 table-entry names reach `dwg-to-dxf` as `\U+XXXX` escapes of the
characters their page gives (`\U+00E9\U+00B0\U+00C0\U+00FC` under 30,
`\U+0439\U+00B0\U+0410\U+044C` under 29), where they were `\U+FFFD` before. The
corpus's own twins do not show how AutoCAD exports an in-page character: the
`\U+XXXX` form is the writer's choice (`src/dxf_header.rs`), measured against
nothing but the escapes the twins hold as literal text. The first run of this
probe named one reader that takes a `RawObject` without the dispatcher:
`tables::dimstyle::decode_with_vars`, which DXF TABLES calls for DIMSTYLE (0 of 8
before, 8 of 8 now); it puts the record's page in force itself. No other reader
of the crate takes a record that way. The `from_utf8_lossy` calls left in
`src/` are `CodePage`'s own fallback, the ASCII section names of the R2004
section map and the DXF reader's.

**Counts.** On `main` at `5851283` with this change merged: `measure.py check`
matches all 52 rows (15,527 records decoded, 547 skipped, 0 errored), so no
baseline is regenerated; the DXF oracle's regenerated table is identical to
`tools/oracle/baseline.tsv` (105,690 values equal, 11,142 default, 283 different,
6,722 DWG-only); `probe_header_vars`, `probe_dxf_header`, `probe_dxf_tables`,
`probe_dxf_entities`, `probe_entity_colour`, `probe_text` and `probe_mtext` read
0 differences; `cargo test --release --all-features` passes (921 tests). Text
content changes no field width, and none moved.

**Owed.** The writers still emit strings as UTF-8 (`file_writer.rs` names,
the element encoders): the inverse of this decode, a page-aware encoder, is not
in this change. XRECORD strings (§20.4.104: an `RC` code page byte after the
length) and extended entity data strings (§28: a 2-byte code page short) carry a
code page of their own beside each string; the crate does not decode them
(XRECORD keeps its bytes, the extended data chain is skipped).
`dxf_tables::dash_text` (#358) takes `CodePage::decode` with the record's page
(`ltype_groups` is handed `RawObject::code_page`); the corpus's complex
linetypes have ASCII text, so the probe above patches an LTYPE's name and no
data reads that text area through a non-ASCII byte. The diagnostic `read_tv`
copies in `examples/trace_*` and the field-list probes still decode as UTF-8;
they print strings and compare none.

## 7aq. ACDBASSOCPERSSUBENTMANAGER and ACDBPERSSUBENTMANAGER, derived from AutoCAD's own output (2026-10-27, #152)

ODA spec v5.4.1 prints no layout for either class, and no other lane had touched them: they
were the last two skipped types with no decoder and no open PR. `src/objects/pers_subent.rs`
reads them from the layout below, derived by the CLEANROOM.md method of 2026-10-26: the data
bits of every record in the pinned corpus, read against the object of the same handle in
AutoCAD's DXF export. The corpus holds one record of each class per release, 14 in all, and
all 14 close on the data-stream boundary. Every record is in ACadSharp's `sample_base`
(AutoCAD 2025, saved as R14, R2000, R2004, R2007, R2010, R2013 and R2018) at the same handles,
0xD62 and 0xD63, on every release; no nextgis drawing holds either class, and the R2000 to R2018
exports hold both objects (the R14 save has no twin). One drawing therefore supplies all 14.

| Release | Records | Close | Data bits (ASSOC, PERS) | Values compared | Differ |
|---|---:|---:|---|---:|---:|
| R14 | 2 | 2 | 49, 64 (boundary from the `RL` in the common object data) | none, no DXF twin | |
| R2000 | 2 | 2 | 49, 64 | 19 | 0 |
| R2004 | 2 | 2 | 49, 64 | 19 | 0 |
| R2007 | 2 | 2 | 49, 64 | 19 | 0 |
| R2010 | 2 | 2 | 49, 64 | 19 | 0 |
| R2013 | 2 | 2 | 107, 28 | 22 | 0 |
| R2018 | 2 | 2 | 107, 28 | 22 | 0 |
| Total | 14 | 14 | | 120 | 0 |

### The field lists

```text
ACDBPERSSUBENTMANAGER
  BL version, array, array, array
ACDBASSOCPERSSUBENTMANAGER
  BL version, array ids, one array per id, one more array at version 2, B flag
array = BL count, then count x BL
```

| Variant | Records | Groups 90 of the DXF object, in order | Group 290 |
|---|---|---|---|
| ASSOC, version 1 (R14 to R2010) | 5 | 1 3 0 2 4 0 0 0 | 0 |
| ASSOC, version 2 (R2013, R2018) | 2 | 2 3 0 2 4 0 2 3 4 2 3 4 0 | 0 |
| PERS (R14 to R2010) | 5 | 2 0 2 4 0 2 3 4 | none |
| PERS (R2013, R2018) | 2 | 2 0 2 0 0 0 | none |

How it was found, so that it can be checked. Each record's bits tile exactly as `BL`s: the
version, the counts and the values are 10-bit forms (code `01`, then a byte) for 1, 2, 3 and 4
and 2-bit zero codes for 0, and the ASSOC ends in one more bit. Those numbers are the groups
90 of the DXF object in file order, count for count (8, 13, 8 and 6), and the last bit is
group 290. `probe_field_list` reproduces it with `8*BL,B`, `13*BL,B`, `8*BL` and `6*BL` on
handles 3426 and 3427 of every `sample_AC10xx.dwg` from R2000. The grouping into arrays
is the reading, among those that parse the same bits, that keeps the most repeat counts in the
stream: in the ASSOC the ids array `[0 2 4]` is followed by three arrays (all empty at version
1; `[]`, `[3 4]`, `[3 4]` at version 2), then at version 2 one more empty one; in the PERS the
arrays are `[]`, `[4 0]`, `[3 4]` on R14 to R2010 and `[]`, `[0 0]`, `[]` on R2013 and R2018.
A count is followed by exactly that many values in all 14 records.

### Values compared with the DXF (`probe_pers_subent`, `probe_assoc_pers_subent`)

| Object | Group | Compared | Differ |
|---|---|---:|---:|
| ACDBASSOCPERSSUBENTMANAGER | 90 (every value, and their number) | 64 | 0 |
| ACDBASSOCPERSSUBENTMANAGER | 290 | 6 | 0 |
| ACDBPERSSUBENTMANAGER | 90 (every value, and their number) | 50 | 0 |

The DXF value oracle scores both (`examples/dxf_oracle/scorers.rs`): 64 and 44 values equal
the DXF on six releases, 0 different, and the 12 groups it leaves are the owner and the
reactor (330). The corpus baseline moves by 2 decoded and 2 skipped on each of the seven
`sample_AC10xx.dwg` drawings (14 in all) and no other drawing changes.

```sh
python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
cargo run --release --example probe_assoc_pers_subent -- ../dwg-corpus
cargo run --release --example probe_pers_subent -- ../dwg-corpus
cargo run --release --features dxf-reader --example dxf_oracle -- ../dwg-corpus \
    --baseline tools/oracle/baseline.tsv
```

### Unverified by value

The numbers are verified; their grouping and meaning are not, and a field that one drawing
shows stays provisional (#348).

- Where each array ends. The DXF writes every number as group 90, so no group shows it.
  Readings that parse the same bits: a fixed number of arrays after the ids (3, then 4 at
  version 2) in place of one per id; the version-2 tail as a lone `BL` 0 instead of an empty
  array; and the leading `BL` 3 of the ASSOC (and `BL` 0 of the PERS) as a lone scalar with
  the arrays starting after it. All 14 records hold the ids array at three ids and the PERS at
  an empty first array, so none separates them.
- What any value means. The ids `0 2 4` and the array items `3 4`, `4 0`, `0 0` are small
  integers with no role the twin names.
- Whether the version-2 array is gated by the version word or by the release: the word is 1 on
  R14 to R2010 and 2 on R2013 and R2018 in all seven ASSOC records, so the two coincide. The
  PERS carries 2 everywhere, with its content differing by release all the same.
- The `BL` width: every value is under 256 and every zero uses the 2-bit code, so `BS` is
  bit-identical on all 14 records; group 90 is a 32-bit integer, and the 32-bit form is never
  exercised.
- The flag is pinned in width (a `BS` or `BL` zero would take two bits and the record has
  one) but is 0 in every record.
- A version word other than 2 (PERS) and 1 or 2 (ASSOC) is refused with
  `Error::Unsupported`, never read on a guess.
- Handles. The DXF writes no handle-valued group for either object beyond the owner and the
  one reactor (both 0xC, `330`). Read by hand from the bits after the data on all 14 records,
  the handle stream holds those two (on R2000 also a null extension-dictionary handle) and
  padding; no probe reads it, and none is decoded.

The controlled drawings should vary what these records hold (a solid with subentities, a
dynamic block, a second associative object), so that the arrays carry something other than
the three ids and the counts of two.

## 7ar. dwg-to-dxf: the BLOCKS section (2026-10-26, #126)

`src/dxf_blocks.rs` writes one block per `BLOCK_RECORD` of the table, in its order: the
`BLOCK` (its handle from the block record's handle stream, its layer, name, flags, base
point, description and xref path), the entities of the block that
`dxf_entities::build` collects (model space and the active paper space are in ENTITIES, so
those two blocks are empty here, as in AutoCAD's own export) and the `ENDBLK`. Any kind
nothing writes yet is counted in a `999 skipped` comment. The tables are built a second time
knowing which entities and blocks are written, so the header's `$DIMBLK` family, a
DIMSTYLE's arrowhead blocks and a BLOCK_RECORD's `{BLKREFS` name only what is in the output.
`examples/probe_dxf_entities.rs --section=BLOCKS` pairs every record of the section with the
export's of the same handle; the Corpus job runs it.

| Kind | Written | Paired | Groups written | Agree | Disagree | Extra | Groups omitted |
|---|---:|---:|---:|---:|---:|---:|---:|
| ARC | 12 | 12 | 118 | 118 | 0 | 0 | 0 |
| ATTDEF | 29 | 29 | 324 | 324 | 0 | 0 | 20 |
| BLOCK | 270 | 264 | 2274 | 2268 | 0 | 0 | 0 |
| CIRCLE | 30 | 30 | 150 | 150 | 0 | 0 | 72 |
| ENDBLK | 270 | 264 | 420 | 420 | 0 | 0 | 0 |
| INSERT | 48 | 36 | 378 | 378 | 0 | 0 | 24 |
| LINE | 432 | 432 | 4472 | 4472 | 0 | 0 | 0 |
| LWPOLYLINE | 12 | 12 | 90 | 90 | 0 | 0 | 0 |
| MTEXT | 147 | 142 | 1990 | 1990 | 0 | 0 | 122 |
| POINT | 204 | 204 | 1190 | 1190 | 0 | 0 | 0 |
| SOLID | 78 | 78 | 1177 | 1177 | 0 | 0 | 0 |
| All | 1532 | 1503 | 12583 | 12577 | 0 | 0 | 238 |

Every group written has the export's code and value, in its order, over the 42 drawings of
the corpus that have an AutoCAD export of the same release: 1,503 of 1,532 records pair, 12,577
of 12,583 groups agree. The six that do not are the BLOCK name of a dynamic block, below. The
29 that do not pair are the records of one block per drawing the export registers again.

- **A block the export registers again.** `*U25`, an anonymous block of the ACadSharp saves,
  is in the export under a new block record, new `BLOCK` and `ENDBLK` and new entity handles
  (the tables probe pairs the record by name). The DWG's `BLOCK`, `ENDBLK` and entities of it
  (one MTEXT and two INSERTs per drawing) have no counterpart by handle, so the probe pairs the
  two `BLOCK` records by name and sets the block's records on both sides aside, counted with
  their reason.
- **The dynamic block of R2000 to R2007.** `my-dynamic-block` is in the DWG under that name
  and in the export as `*U17`: AutoCAD loads a dynamic block of those releases as an anonymous
  block. The probe names it a relation (six BLOCK records, name and description groups).
- **Group 4.** A `BLOCK` carries its block record's description as group 4 when it has one.
- **A block left out, its entities and its INSERTs.** A block whose `BLOCK` or `ENDBLK` cannot
  be written (its layer or linetype is not in the tables, its base point is not finite) is not
  in BLOCKS. The entities inside it are left out with it (counted `(in a block that is not
  written)`), so the handles the second table build is told are written are all in the output
  and no BLOCK_RECORD's `{BLKREFS` lists an INSERT that is not; an INSERT of the block, with its
  ATTRIBs, is left out too (counted `INSERT (its block is not written)`). The probe converts
  every drawing to every release the writer writes it to and checks each INSERT's block (group
  2) against the `BLOCK`s of the output (510 INSERTs in 264 conversions, 0 unresolved) and
  each BLKREFS handle against the output's records (366 handles, 0 not written). No corpus
  drawing has a block the writer must leave out, so the probe also builds the entities of each
  drawing once per layer with that layer out of the tables, which takes the blocks on it out
  (160 builds, 283 blocks left out), builds the tables from what is left and reads the two
  sections' writers back: 39,429 handles told to the tables as written, 0 not in the output,
  1,124 BLKREFS handles in those tables, 0 not in the output, 1,580 INSERTs written, 0
  naming a block that is not written. With the entity filter taken out, 589 of the 40,018
  handles told are not in the output and the probe exits 1; no BLKREFS entry dangles in the
  corpus even then, so that symptom is guarded by the check and not measured. A negative run
  (`PROBE_DXF_ENTITIES_DROP_BLOCK=1`) takes one written block and one written entity out of
  the output the two first checks read: 72 INSERTs and 42 BLKREFS handles unresolved, exit 1.
- **Not written yet.** 24 VIEWPORTs, one multi-line ATTDEF (R2018), and 38 MTEXTs that carry a background fill (90, 63, 45,
  441: the DWG stores the colour as an RGB word and AutoCAD derives its nearest index, 63,
  which is not decoded) or an embedded object (R2018); the probe counts them by kind.

## 7as. XRECORD values and extended entity data, in the code page each string names (2026-10-27, #375)

Two places hold pre-R2007 text that the crate never turned into a string. An XRECORD's
body stayed opaque bytes (`XRecord::data`), and the extended entity data chain
(§28, the `BS` size, application handle and payload that follows every object's handle)
was read to be skipped (`common_entity::skip_extended_data`). `src/extended_data.rs` reads
both as values for R13 to R2004: `extended_data::read` for the chain of a walked record and
`extended_data::xrecord_values` for an XRECORD, each string decoded by
`CodePage::decode` in the page the string itself names. R2007 and later (UTF-16) are refused.

**Layouts, measured.** On every pre-R2007 drawing of the pinned corpus (24 drawings, 2,370
XRECORDs and 1,263 records with extended data), the specification's two string layouts
turn out to be one. §20.4.104 says "the string length (RC), the dwgcodepage (RC), and then
the string" and §28 "1st byte of value is the length N; this is followed by a 2-byte short
indicating the codepage": in the data both are the same three bytes before the characters,
an `RS` length and an `RC` code page (a length below 256, a zero, the page). No string of
the corpus is 256 bytes long (the longest is 250), so the data cannot say whether the
length is an `RC` followed by a short or an `RS` followed by a byte; the reader takes the
`RS` form, which is also the one that reads the XRECORD strings of every record to its
last byte. Two more things the specification leaves open:

| Value | XRECORD (§20.4.104) | Extended data (§28) |
|---|---|---|
| handle | 8 bytes, least significant first (330 to 369) | 8 bytes, most significant first, "read as hex" (codes 3 and 5), the layer of code 1003 resolving to the layer name the export writes |
| the other widths | by the DXF group code's range: 16-bit for 60 to 79, 170 to 179, 270 to 279, 370 to 379 and 1070; 32-bit for 90 to 99, 420 to 429, 440 to 449 and 1071; one byte for 280 to 299; a double for 40 to 59, 140 to 149 and 460 to 469; three doubles for 10 to 39; a binary chunk with a length byte for 310 to 319 and 1004 | the item widths §28 lists |

The XRECORD ranges are those of Autodesk's public DXF Reference (recalled, CLEANROOM.md),
taken for a range only where a code of the range occurs in the corpus; every XRECORD body
then closes exactly on its length (2,370 of 2,370). A code outside them (for instance 105,
110 to 139, 160 to 169, 210 to 239, 320 to 329, 380 to 419, 999 and 1005 to 1013) is
`Error::Unsupported`, never read at a guessed width, and `XRecord::data` is there to read as
bytes.

**Every string names the header's page.** 78,242 strings (76,926 in XRECORDs, 1,316 in
extended data) carry a page byte, and every one equals the header's short: 29 or 30, in R14
(27,153 strings), R2000 (25,751) and R2004 (25,338) alike. The decoder reads each string in
the page it names (a number the table lacks reads as before), so the two agree here and a
file whose strings named another page than its header would be read by its strings.

**Against AutoCAD's exports.** `examples/probe_code_page.rs` sets each decoded record beside
the export's record of the same handle, group by group (R2000 and R2004; R14 has no export):

| Release | XRECORDs compared | Equal | Save-time | Extended data records compared | Equal | Tail only | Export only |
|---|---:|---:|---:|---:|---:|---:|---:|
| AC1015 (2000) | 227 | 225 | 2 | 410 | 410 | 2 | 1 |
| AC1018 (2004) | 227 | 225 | 2 | 347 | 347 | 0 | 1 |

1,211 of 1,211 compared records agree. The probe lets a value differ only where the save
itself makes it anew, and only in two named records (`SAVE_TIME_XRECORDS`), under the same
group code in the export, with every other value of the record and the count of groups still
compared: in `#306` a handle the save gave another value (0x1020 in the DWG, 0x1487 in the
export, the oracle's save-time rule) and in `#D09` a GUID and the path of the file the save
wrote (`name.dwg` against the export's `name_ascii.dxf`). The "export only" record is the
VPORT table header `#8`, which the DXF writer gives `1001 ACAD`, `1000 DbSaveVer`, `1071`
and the DWG holds no extended data for; any other application data the export holds for a
record the reader returns nothing for fails the probe. Applications are paired by identity:
each application of the export (the value of its group `1001`) with the decoded
application whose handle resolves, through the drawing's APPID entries, to the same
name, and then item by item, so a wrong application handle fails. The 2 "tail only"
records are the two MTEXT (`#9DA` and `#CDC` of `sample_AC1015`) whose `ACAD` application
data begins with an `RTTcAl` block (a string `RTTcAl`, a short, a long and
`$$NoName$$`) that the R2000 export leaves out; its other items equal ours, and that
exact block, on an `ACAD` application of an MTEXT record, is the only prefix the probe lets
the export lack. A decoded application the
export omits altogether is counted and not compared (45 on the pinned corpus: the export
drops several render applications' data).

**A patched copy.** As in §7ap, strings rewritten in memory: the XRECORD string and the
extended data string of the first record that holds one of six bytes or more sitting once in
the file, their first four characters overwritten with `E9 B0 C0 FC` and their page byte (the
eight bits before the characters) set to 29, 30 and 31, and read back through the new
readers: XRECORD values 24 strings, 96 of 96 reads (`é°Àü`, `й°Аь`, and under 31 the bytes as
before); extended data 3 strings, 12 of 12 (R14, R2000 and, since the DWG writer assembles
the patched R2004 drawings, R2004; the string's own page byte is patched there and the
header keeps the drawing's page).

**R14, and the assumption that the shorts mean there what they mean later.** Nothing in
the corpus names an R14 page: AutoCAD's exports of the AC1014 drawings are R12 DXF, which
carries no `$DWGCODEPAGE`. What is measured is weaker, and recorded as such. The strings of
the R14 drawings (27,153 in XRECORDs and extended data) name 29 and 30, the same two numbers
their headers carry; each of the seven nextgis R14 saves has an R2000 save of the same stem
whose short is 29 and whose export names `ANSI_1251`, and the ACadSharp R14 and R2000 saves
both carry 30. That is one project saved to two releases, not a statement by AutoCAD that
the numbers keep their meaning; the rows apply to R14 on that footing and no export confirms
it.

**Not covered.** R2007 and later extended data and XRECORD strings (UTF-16); the unmeasured
XRECORD ranges above; the length field's width for a string of 256 bytes or more; an
extended data application the export omits; and the entities' own strings that sit in a
record's extended data on an R13 drawing (the corpus holds none).

## 7at. The writers write the drawing's code page (2026-10-27, #375)

#374 reads pre-R2007 text in the page the header names; the writers still did the
opposite of what they read. `dwg-to-dxf` wrote every character above ASCII of a
pre-R2007 DXF as a `\U+XXXX` escape, whatever the drawing's page (a `String` could
hold nothing else), the class table of the DWG writer wrote its strings as UTF-8
(`classes::write_tv`), and `DwgFile::to_bytes` assembled the R2004-family header with a
code page of 0, so a rewritten file read its own text as an unnamed page. Now:

- `CodePage::encode_char` and `CodePage::encode` are the inverse of `decode`: a character
  the page holds is its byte (two on a double-byte page), any other is the escape
  `\U+XXXX`, four uppercase hexadecimal digits of each UTF-16 unit, and text that is
  already an escape (MTEXT's `\U+00B0`) passes as it is. Under an unnamed page no
  character is known to be in the page, so the result is ASCII with escapes.
- `dxf_convert::convert_dwg_to_dxf_bytes` (and `convert_file_to_dxf_bytes`, and the
  `dwg-to-dxf` binary) write R12 to R2004 in the drawing's page: each character the page
  holds as its byte, every other as an escape. The writer's text is still a `String`, so
  the page bytes travel through it as the chars of the same code points and the function
  turns them back into bytes; `convert_dwg_to_dxf` keeps returning escapes only. From
  R2007 the output is UTF-8 as before.
- `classes::write_tv` encodes through the page in force, and
  `file_writer::assemble_dwg_bytes_with_page` writes the page into the header (the short at
  0x13); `DwgFile::to_bytes` passes the drawing's own, so the string bytes it copies read
  as the text they were.

**What AutoCAD does.** Two things are measured; the rest is owed.

- *An in-page character is its byte in a pre-R2007 DXF.* The corpus's one raw high byte
  in a pre-R2007 export is `0xD8` in a TEXT of `sample_AC1009_ascii.dxf` (R12): AutoCAD
  wrote the character the page holds as that byte. None of the R2000 and R2004 exports
  holds one (their only non-ASCII is escape text copied from the DWG), so for those
  targets the choice rests on the DXF Reference's statement that DXF before R2007 is in
  `$DWGCODEPAGE` (public documentation, recalled; `dxf_reader` reads on the same
  statement), not on an export.
- *A character the page lacks is `\U+XXXX`.* The corpus holds one: the diameter sign
  U+2205, which the R2007 and later saves of `sample_AC10xx.dwg` hold as the character and
  the R2000 and R2004 saves hold as the ASCII escape `\U+2205`, in an MTEXT and in an
  XRECORD string. `CodePage::encode` of each R2007 and later text, under the R2000 file's
  page, gives the bytes of the R2000 text of the same handle: 59 of 59 texts in each of
  `sample_AC1021`, `sample_AC1024`, `sample_AC1027` and `sample_AC1032` (one holding the
  diameter sign, written `\U+220545,6` as AutoCAD wrote it; five more are MTEXT that AutoCAD
  formatted differently in the two releases, a line break or a stacked fraction, and are
  not compared). That is MTEXT and one XRECORD. No TEXT, ATTRIB, ATTDEF or table name of
  the corpus holds a character outside its page, so the escape for those strings is the
  same one by extension, as it was before this change, and stays unmeasured.

**Evidence.** `examples/probe_code_page.rs`, on the 24 pre-R2007 drawings of the pinned
corpus, with the strings rewritten in memory as in §7ap:

| What | Result |
|---|---|
| `encode` is `decode`'s inverse on every byte above 0x7F of each named page | 128 of 128 for `ANSI_1251` and for `ANSI_1252` |
| The escapes: U+2205 and U+0439 under 1252, U+00E9 under 1251 and under an unnamed page, MTEXT's own `94\U+00B0` | each as above |
| 266 strings in 27 kinds of record (R14, R2000 and R2004, XRECORD values and extended data included), read under the drawing's own page, 29, 30 and 31; the R2004 drawings re-assembled by the DWG writer with that short as the header's page | 1,064 of 1,064 reads as the code page charts say |
| The 87 table-entry names of those drawings written by `dwg-to-dxf` (R2000 target, bytes) | 348 of 348 checks: under 29 and 30 the patched bytes `E9 B0 C0 FC` and the rest of the name verbatim, under 31 the `\U+FFFD` escapes of what was read |
| `DwgFile::to_bytes` on the R2004 family | 23 of 23 rewritten drawings keep the header's page, and the 8 pre-R2007 ones decode to the same entities (5 more are refused by the experimental writer for their section map, as before) |
| R2007 and later texts against the R2000 save, as above | 4 of 4 drawings, 59 of 59 each |

Until the OBJECTS section was written no text of the corpus left ASCII in a pre-R2007 DXF, so
`convert_dwg_to_dxf_bytes` wrote exactly the bytes `convert_dwg_to_dxf` wrote: 104 of 104
conversions of the corpus's 52 drawings (R2000 and R2004 targets), identical. The `ACAD_LAYOUT`
dictionary of the GBK drawings now writes their layout name as a key (#126), the first
character above ASCII in a written DXF of the corpus: `convert_dwg_to_dxf` spells it
`\U+5E03\U+5C40`, `convert_dwg_to_dxf_bytes` writes the page's bytes `B2 BC BE D6`, the
bytes AutoCAD's own export holds for that key, so 10 of the 122 conversions now differ
(112 of 122 identical). The check is therefore that the bytes read back under the
drawing's page to the `String` writer's text once every character above ASCII is spelled as
its escape again, and that at least one conversion writes page bytes. The first run of the
older, identical-bytes check read 0 of 104 and named a real defect: four comments the writer itself emits (`999` groups in
`dxf_sections.rs`: the ACI colour note, the opaque proxy note and the custom-class
pass-through note) held an em dash or a `≈`, which a pre-R2007 DXF reads as bytes of the
code page, not UTF-8 (and which the bytes conversion cannot map to one byte). They are ASCII
now, and a character above U+00FF that still reaches the bytes conversion is written as a
`\U+XXXX` escape, not as `?`. `probe_dxf_header`, `probe_dxf_tables`,
`probe_dxf_entities` and `probe_dxf_classes`, which call `convert_dwg_to_dxf`, read 0
differences.

**Owed.** A TEXT, ATTRIB or name outside its page (above); an in-page character in an
R2000 or R2004 export (above); MTEXT in an R2000 or R2004 DXF target, which the ENTITIES
writer does not write yet, so no AutoCAD export pairs with the writer's own escape end to
end; characters above the BMP, two escapes each as before and never measured; and a
strings encoder for the entities, which the DWG writer does not write at all (its only
authored strings are the class table's).

## 7au. Code page rows 28 to 41, from drawings that name them (2026-10-27, #375)

#374 named two header shorts (29, 30) because the pinned corpus held no others. The
number at 0x13 of the header (§3.2.5) is AutoCAD's own enumeration of code pages, not a
Windows code page number, and the shorts 28 to 41 run in the order of the Windows ANSI
pages with gaps: 28 is 1250, 29 is 1251, 30 is 1252, but 38 is 932, 39 is 936, 40 is 949
and 41 is 950. This change adds the eight rows below, each backed by a drawing, and pins
the public drawings that can back two of them:

| Short | `$DWGCODEPAGE` | Decodes through | Backed by |
|---:|---|---|---|
| 28 | `ANSI_1250` | Windows-1250 | a DXF twin of the same drawing names it (local) |
| 32 | `ANSI_1253` | Windows-1253 | 10 of 14 Greek drawings decode to Greek words (local) |
| 33 | `ANSI_1254` | Windows-1254 | a DXF twin of the same drawing names it (local) |
| 36 | `ANSI_1257` | Windows-1257 | the DXF of another save of the same drawing names it (local) |
| 38 | `ANSI_932` | Shift_JIS (code page 932) | the Japanese layout name decodes in 4 of 4 pinned AC1014 drawings |
| 39 | `ANSI_936` | GBK (code page 936) | the Chinese layout name decodes in 5 of 5 pinned drawings (AC1015, AC1018) |
| 40 | `ANSI_949` | EUC-KR (code page 949) | the Korean layout name decodes in 4 of 4 distinct drawings (local) |
| 41 | `ANSI_950` | Big5 (code page 950) | a DXF twin names it and 8 of 8 distinct non-ASCII strings equal the export's (local) |

Not named, and why: 0 (one AC1018 pairs with `ANSI_0`), 1 and 23 (one drawing each, no
twin), 45 (an AC1012 whose twin says 936), 31 (never seen), 34 (one AC1015 file of 43 MB
with no text check, and one AC1032 twin of unknown producer that says `ANSI_1255`), 35 and
37 (AC1032 files only, twins that say `ANSI_1256` and `ANSI_874`) and the DOS and ISO pages
(never seen). They stay `CodePage::Unknown` and their text is read as before.

**The pinned drawings.** `tools/corpus/pins.tsv` gains nine, all fetched by sha256 and never
committed (CLEANROOM.md, 2026-10-27): four AC1014 drawings of `kurema/BookViewerApp3` (MIT,
short 38), four AC1018 drawings of `Seeed-Studio/OSHW-Jetson-Series` (Apache-2.0, short 39,
AutoCAD build `I.18.0.0`) and one AC1015 drawing of `huifer/planar_algorithm` (Apache-2.0,
short 39). Three `icon.dwg` of the first repository are left out because the corpus keys
files by name, and the fourth candidate of the second kind, `Xinyuan-LilyGO/T-Dongle-S3`
(MIT), because its `AcDb:AppInfo` names Teigha 19.7 as the writer, not AutoCAD. The fetcher
now percent-encodes the path of a file, which the first of these (spaces) and the last
(Chinese) need. None of the nine has a DXF twin, so the number is proved by text:
AutoCAD names its first layout in the language of the installation, `レイアウト1`, `布局1`
and `배치1`, and `examples/probe_code_page.rs` decodes every string of every pre-R2007
drawing and fails when a drawing of a checked short holds none of the known names:

| Short | Drawings | Decode to the known layout name |
|---:|---:|---:|
| 38 | 4 (all AC1014) | 4 |
| 39 | 5 (4 AC1018, 1 AC1015) | 5 |

In every one the name is a Dictionary entry and the name of the drawing's LAYOUT object
(a class-numbered object, 502, in the R14 and R2000 files). The text is raw
double-byte data: `レイアウト1` is `83 8C 83 43 83 41 83 45 83 67 31`, five two-byte
characters whose trail bytes (`43`, `41`, `45`) are ASCII letters, so a double-byte string is
not recognisable as non-ASCII by its trail bytes, only by its lead bytes.

**Local evidence.** The other rows rest on drawings from repositories that state no licence, a
custom one, or open-data records whose licence the publisher leaves unstated; they were
downloaded for this measurement, are not pinned, and are listed here by sha256 so anyone can
fetch the same bytes (each is a file of its repository's default branch on 2026-10-27; no
commit was recorded). `probe_code_page` run over a directory of them reads:

| Short | What the probe printed | Drawings (sha256) |
|---:|---|---|
| 28 | header 28, export `ANSI_1250` (AC1015; the export's `VERSIONGUID` is in the DWG) | `c6837f5bdd4870cbaadd6c3b64db647edcd7695eb604f84381c772935501ffc5` |
| 33 | header 33, export `ANSI_1254` (AC1015; both GUIDs of the export are in the DWG) | `3b4a60301ed32c830668bf09dd34ab8077080b94ec315bd36a9c81c9e17bf2d9` |
| 36 | header 36, export `ANSI_1257` on 3 AC1018 drawings | `4679c1c2d2fb76afe213dc01a33a57e90dce2ad599c7a7207a0d295a5e488d39`, `d757100f62bdbbe2ccb7cda9761f72536413381b715db87513b6f3efd04e4bfd`, `0ebc64a2b4bd20dde88d9f3f9f390a4cffdc05892865d908b29bbb9a0be707fb` |
| 41 | header 41, export `ANSI_950` on 2 AC1018 drawings; 8 of 8 distinct non-ASCII strings occur in the export | `11912c97aa9fa6d8c3ee46f55e34a9a6da2868105a919535ba0cf4d9a4540b07`, `8b5ccd79d7ef8304794f6049e9a92e72649513d492b157108e384afe9fc1bf9d` |
| 40 | 4 distinct AC1015 drawings (one is uploaded twice) decode to the Korean layout name | `4cb4bda6f3e8ba8f52155300546f70d202526301344b029e868d21172895e16c`, `715a85bc28f9b19fe06b0a98307cc5ff6a30a63100d751fb915ae0a8a6e19d5a`, `1f644de3f83a397dca237b174d121cb3485a7a47f80891856a6c339e7003cd31`, `12f609df87b0d5f1003e5f866281c5dba5bc506e9e83ec7743886813bfa8f44e` |
| 30 (R14) | header 30, export `ANSI_1252` on an AC1014 drawing and its AC1014 export | `bf77cf41e8eddfa51bfd9cca9b622accea822e67de0ab355078a9b189eb59373` |
| 32 | 14 distinct drawings (3 AC1014, 1 AC1015, 10 AC1018) of five Greek municipalities' open data; 10 decode to one of five Greek words, the other 4 hold none of them | the 14 sha256 values below |

The Greek drawings (`data.gov.gr` resources, catalogue licence CC BY 4.0 by the harvested
record, "not specified" by the publisher's own): `41e1314447765107143eec7b188a026d8ea94c9d33fe062e25c89e0632cd7d2f`,
`3a81107ae70ac709d6ec77e60ab83fe6b0db4f2e4281ce94377483b68621d6f4`,
`e770851c3f27699af207c8aed240c622e8fbcabc1acc6cca353e2e4bcc9933b0`,
`c476ca65c000d9c21483711d9791f92daaff42e427409825daac8bbcad942d1a`,
`a49e0eb948f88da5e9b704c7a3fb64ad4e87bd90fc33e7f9920f47275eb42821`,
`adb56428dd6e5ee69c72b20f5265c30ee5e4936bd915eaf81f59adb8e2c726e5`,
`3f163f4f60a62c71c6290eb77c5f788c84653b9acb9094714e4d1d13c931a942`,
`9c13e8fd092591d0e0969f5a5e6adcb5c9c70f5082af5a33396be3fc9ef113e2`,
`7f36b422d594090db9acdbd8847875d361f8a4f17d3c521ed0302f4b5d13c7a4`,
`1e6d884cdda4c896b614aebab5838d1a607104e01e64c161934ab0043aaf6844`,
`31cbfea23edcd07a07f1c4858083567aaad21dda089ffa910f8dc0cec708b5f2`,
`d2cacf61be0c3a0c8266faec297c422e407dee76b527b767a9b8bb764f746a05`,
`0808a2317a4d84304405eec9d0bfea2af62adcecd20ce264dfb4c94df143ba6c`,
`c6c01d061e25c0177f068dbbb320c9e678925ad1a462e969c455fe2b7ea0bfcd`. The words decode as accented, mixed-case Greek
(streets, offices, names) in the AC1014 drawings too, and the text of the MTEXT font tags
carries `c161`, the Windows Greek charset number, beside it.

Two caveats the rows carry. Rows 28 and 33 rest on the twin alone: neither export has a
byte above 0x7F, so no text separates 1250 from 1252 or 1254 from 1252 there; they are
the Windows pages the shorts' order gives and the exports name. Row 36's three DXF files
are another save of each drawing, not the export of the file read (the DWG holds an ATTRIB
at handle 0x87 where the DXF has a LINE, and the only non-ASCII text, a degree sign, is
the same in 1252 and 1257), so the probe's per-record comparison reports 4 strings
differing there and its set comparison 0 of 4: the row is the weakest of the eight. The
research that found these drawings also counted DXF twins of AC1024 and later drawings
that name 1250, 1253, 1254, 1257, 949 and 950 for the same shorts (the header short is
read from every release), 39 of them for 949 alone; those files are not kept or pinned
and are not part of what this probe reproduces (it pairs the short of every release with
its export, which on the pinned corpus gives 29 and 30 on 35 and 7 drawings, 0 differing).

**How out-of-page text is held.** The strings of the 14 Greek and the Chinese drawings
carry `\U+XXXX` escapes (text: `\U+00E1` and `\U+00DF` in a Greek-page drawing, `\U+2205` in a
GBK one), and the pinned corpus holds 9 in the three ACadSharp saves (short 30): a
character outside the page is written as the escape of its Unicode code point, in ASCII,
by the application that saved the drawing, on R14 as on R2004, and the string keeps it as
it is, the escape being the text's own (a page byte `E1` is `α` under 1253; the escape
`\U+00E1` is `á`). The multibyte interchange form
`\M+` appears in none of the strings of the 52 pinned drawings or the local ones.
`CodePage::encode` (#382) writes the same escapes, and returns bytes only for a
character that decodes back to itself, which keeps Shift_JIS's extra mappings (U+00A5 to
0x5C) out.

**Counts.** `measure.py` over the 61 pinned drawings (the Corpus job's `corpus-measurement` artifact on
the first head of this change): the 52 existing rows are unchanged and nine are added,
and `tools/corpus/baseline.tsv` carries them. The nine decode 106,189 records and skip 90
(the custom classes CELLSTYLEMAP and MATERIAL, and two render-settings classes in the
R14 files), and **error on 9**: every one a
BLOCK_HEADER of the four Seeed AC1018 drawings (5 in `A205_TOP.dwg`, 1 in
`A206_BOTTOM.dwg`, 3 in `A206_TOP.dwg`), the corpus's only drawings of AutoCAD build
`I.18.0.0`. Those records are the 1,028-byte "descriptions" the probe lists as non-ASCII strings
(`#101`, `#151`, `#335E` of `A205_TOP`, `#2AC6` of `A206_TOP`): the decoder reads a
string from a place that is not text and its field list then misses the record's boundary.
The error is the ratchet doing its job on drawings the corpus did not hold before, and
the rows pin it.

**Owed.** A double-byte round trip over real strings (`decode` then `encode` returns the
bytes read) waits for #382, which adds `encode`; the DXF writers' handling of a double-byte
string is that function's. Rows for 34, 35 and 37 wait for a pre-R2007 drawing. In
`A205_TOP.dwg` and `A206_TOP.dwg` four BLOCK_HEADER records (`#101`, `#151`, `#335E`,
`#2AC6`) are among the 9 BLOCK_HEADER records that error (Counts above); their
"description" is bytes of something else, so the layout of the R2004 BLOCK_HEADER tail
after the insert-count run is wrong for these blocks. No layer of this change touches it.

## 7av. BLOCK_HEADER: the insert-count run has no length cap (2026-10-27, #386)

§20.4.52 puts, after the base point and the xref path, "Insert Count: a sequence of zero
or more non-zero RC's, followed by a terminating 0 RC. The total number of these indicates
how many insert handles will be present", then the description, the preview size and the
preview bytes. The decoder read at most 257 of those RCs (`MAX_INSERT_COUNT`, "no realistic
block carries more") and went on to the description from wherever it stood, so a block with
more inserts than that misread everything after the run and the record errored, each in a
different way: a reserved `BL` bit pattern, the bit cursor running out, a preview size of
269,488,144 bytes (`0x10101010`, a `BL` read out of the run's bytes), or fields that closed 6,592 and
7,216 bits past the boundary. The run now ends at its zero, and runs out of record only by
the cursor's own bounds, in both decode paths (R2004 and earlier inline, R2007 and later
from the data stream). `examples/probe_block_header_inserts.rs` is the evidence.

It decodes every BLOCK_HEADER of the corpus and compares the run with the record's handle
stream, which lists the block control, the reactors, the extension dictionary, a NULL, the
owned entities (R2004 and later), the ENDBLK, one handle per insert and the layout (on R2000
the first and last entity of a block that is not an xref). The record agrees when the handles
after the extension dictionary number exactly that sum: a second count of the same run that
reads none of its bytes. All 2,953 records of the 49 pinned drawings from R2000 to R2018 decode
and agree, and the Corpus job runs it:

| Release | Records | Decoded | Handle stream agrees | Run over 256 | Longest run |
|---|---:|---:|---:|---:|---:|
| R2000 | 51 | 51 | 51 | 0 | 10 |
| R2004 | 2715 | 2715 | 2715 | 9 | 29895 |
| R2007 | 48 | 48 | 48 | 0 | 10 |
| R2010 | 48 | 48 | 48 | 0 | 10 |
| R2013 | 48 | 48 | 48 | 0 | 10 |
| R2018 | 43 | 43 | 43 | 0 | 10 |

The nine records that errored before are the whole of the runs over 256, in four drawings of
Seeed-Studio/OSHW-Jetson-Series (Apache-2.0) saved as R2004 by AutoCAD I.18.0.0; no other
block of the corpus has a run that long (every byte of the run of `0x101` is `0x01`):

| Drawing | Handle | Block | Insert handles | Owned objects | Before |
|---|---|---|---:|---:|---|
| A205_TOP | 0x101 | `SMOOTH_0$1` | 383 | 1 | reserved `BL` bit pattern |
| A205_TOP | 0x151 | `SYM_TOP_3` | 426 | 8 | reserved `BL` bit pattern |
| A205_TOP | 0x24CE | `STK_TOP_15` | 260 | 9 | bit cursor exhausted |
| A205_TOP | 0x335E | `DRIL_HOLE_0` | 462 | 2 | fields end 6,592 bits past the boundary |
| A205_TOP | 0x3501 | `SMOOTH_0$0254` | 14079 | 1 | preview of 269,488,144 bytes |
| A206_TOP | 0x20CB | `STK_TOP_72` | 260 | 9 | bit cursor exhausted |
| A206_TOP | 0x2875 | `SMOOTH_0$0254` | 29895 | 1 | preview of 269,488,144 bytes |
| A206_TOP | 0x2AC6 | `DRIL_HOLE_0` | 384 | 2 | fields end 7,216 bits past the boundary |
| A206_BOTTOM | 0x7A9 | `SMOOTH_0$0254` | 8493 | 1 | preview of 269,488,144 bytes |

All nine now close on their data-stream boundary and agree with their handle stream, so the
corpus baseline (`tools/corpus/baseline.tsv`) moves for those three drawings and no other:
A205_TOP 35,002 decoded and 5 errored to 35,007 and 0, A206_TOP 50,626 and 3 to 50,629 and 0,
A206_BOTTOM 13,367 and 1 to 13,368 and 0, and the ratchet check passes on the regenerated
rows. No drawing of the corpus has an AutoCAD DXF export of these four, so the values of
these records are not compared with an export: the boundary and the handle stream are the
evidence (the name, flags and base point precede the run, so the fix does not touch them).
Not corpus-measured: a run over 256 in an R2007 or later drawing (the longest
there is 10; the same change applies to that path), and an R13 or R14 block (the field does
not exist there).

## 7aw. dwg-to-dxf: the OBJECTS section (2026-10-26, #126)

`src/dxf_objects.rs` writes the objects of the drawing that are neither entities nor
table records: DICTIONARY and ACDBDICTIONARYWDFLT (each entry as its key and handle,
`350` for a soft-owner dictionary and `360` for a hard-owner one, with the default
entry's `340`), DICTIONARYVAR, XRECORD, SCALE and ACDBPLACEHOLDER. The handle stream
of an object is the one `tables::handles::refs` reads for a table record: owner,
reactors, extension dictionary, then the object's own handles (a dictionary's item
handles in key order). Records of a class named only in `AcDb:Classes`
(DICTIONARYVAR, SCALE, ACDBDICTIONARYWDFLT) decode through the class map. An
XRECORD's data is typed by the group code's range, as in DXF (`XRecord::items`): `RS`
code, then a string, a real, three reals, a short or long integer, a byte, a handle of
eight bytes or a chunk of binary data. Up to R2004 the data is read by
`extended_data::xrecord_items` (§7as), which decodes each string in the code page it
names and reads only the ranges the corpus measures, so an XRECORD with another code, or
a string outside ASCII in a page the crate does not name, is counted `(not written)`; from R2007 a string is an `RS` length and UTF-16 units and
`items` reads it here. An object is planned only when its record can be written, so a
handle given to the tables, the entities or the dictionaries is always emitted. A dictionary entry whose object is not written
(a MATERIAL, a VISUALSTYLE, a LAYOUT: typed writers come after this one) is left out,
and the probe states why.

### The order of the section

AutoCAD's export walks the objects level by level from several roots: the named
object dictionary first, then every object not owned by an object (an extension
dictionary of a table, a table record, a block marker or an entity), in the order of
its owner in TABLES, BLOCKS and ENTITIES, each root's dictionary entries in key order
followed by its extension dictionary. A level is finished before the next starts, so
a root's children come after all the roots. An object the walk does not reach follows
in file order. The owner places are the table records in table order, the entities by
their block and place in it (an attribute or a vertex by its parent's place), and a
class-numbered entity (an ACAD_TABLE, an IMAGE) by its class's item id 0x1F2 against
0x1F3 for an object. The probe checks the order of every paired record against the
export's, and it holds on all 42 drawings. From the root dictionary alone the walk
reproduces the whole order of the export for 32 of the 35 single-entity drawings
(the other three have an extension dictionary of a table or an entity); the roots
after it account for those and for the larger samples. No corpus dictionary has both
entries and an extension dictionary, so their relative order is not measured.

### Against AutoCAD's export

`examples/probe_dxf_objects.rs` pairs every record written with the export's record
and compares owner, groups (reals to AutoCAD's 16 decimals), dictionary entries (key,
code and handle), the `{ACAD_REACTORS` and `{ACAD_XDICTIONARY` brackets and the order.
The Corpus job runs it.

| Kind | Written | Paired | Groups | Agree | Relation | Disagree | Extra | Groups omitted |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ACDBDICTIONARYWDFLT | 42 | 42 | 294 | 294 | 0 | 0 | 0 | 0 |
| ACDBPLACEHOLDER | 42 | 42 | 84 | 84 | 0 | 0 | 0 | 0 |
| DICTIONARY | 930 | 905 | 7514 | 5809 | 0 | 0 | 0 | 1733 |
| DICTIONARYVAR | 365 | 365 | 1825 | 1825 | 0 | 0 | 0 | 0 |
| SCALE | 826 | 826 | 6608 | 6608 | 0 | 0 | 0 | 0 |
| XRECORD | 296 | 223 | 2815 | 2741 | 84 | 0 | 0 | 0 |

The export registers again objects it creates on load under handles of its own (at or
above the DWG's `$HANDSEED`), extension dictionaries and what they hold: 72 DICTIONARYs,
107 DICTIONARYVARs and 97 XRECORDs among the records written, paired by owner or by the key
under which a paired dictionary holds them. 84 group values are relations the probe names:
the editing time and save date of `DWGPROPS` (40, 42), the saved file's identity of
`ACAD_LAST_SAVED_VERSION_INFO` (300, 302, 303), a handle of an object the export registers
again, the styles AutoCAD lists again in an `ACDB_RECOMPOSE_DATA` XRECORD (330; the probe
compares every 330 after the record's owner, so an XRECORD's handles and a LAYOUT's block
record are checked), and the tail of a round-trip XRECORD that AutoCAD consumes when it
loads the file. Extended data on a dictionary (1000 to 1071) is not decoded (28 groups).
2,223 dictionary entries are written and 1,705 left out: 1,652 name objects the output does
not define, 53 objects the export made itself and 48 objects it leaves out. 10 of the 42
drawings have no export of their release and are skipped.

### Ownership is closed over what the output holds

An object is written only when its owner is null, a table record, an entity or block
marker the output holds, or another object that is written: `Plan::close` drops the rest
(and, in turn, what they own) before any handle is told to a table record or an entity, and
the entities forget the dropped objects as reactors and extension dictionaries. The objects
dropped hang off entities the entity writer does not write yet (HATCH, DIMENSION,
VIEWPORT, MLINE and the others it counts as skipped) and off objects not written yet, and
come back by themselves as those kinds are written. Every record of the 42 drawings'
OBJECTS sections is checked for the handles it names, and each structural class must
resolve to a record of the same output:

| Reference | Written before | To no record before | Written now | To no record now |
|---|---:|---:|---:|---:|
| owner (330) | 5944 | 1287 | 2501 | 0 |
| reactor (330) | 4452 | 0 | 2296 | 0 |
| extension dictionary (360) | 6 | 0 | 6 | 0 |
| dictionary entry (350, 360) | 4452 | 0 | 2296 | 0 |
| dictionary default (340) | 42 | 0 | 42 | 0 |
| XRECORD data handle (an application's, not held to it) | 454 | 120 | 374 | 76 |

The records left out are counted in the probe by kind with the reason "its owner is not
written" (1,111 XRECORDs and 260 DICTIONARYs among them); an export record of a supported
kind that is missing without that reason, or whose owner the output does hold, is still a
failure. A synthetic check writes an XRECORD whose group 102 strings and application
group 100 hold a newline and a character outside ASCII: after `AcDbXrecord` a group 100 or
102 is application data, now escaped for the release like every other string (`^J` for the
newline, `\U+00E9` for the accented letter on R2004), where before the newline split the
record and the DXF no longer read (`malformed DXF at line 21`). No corpus drawing has such a string, so that check measures
the writer and not a drawing. A second synthetic check writes an XRECORD whose application
data holds `102 {ACAD_REACTORS` and a 330 to a handle no record has: the probe reads
brackets only up to `100 AcDbXrecord`, as `DxfRecord::top_level` does, so that 330 counts
as an application's handle and not as a dangling reactor, and a control XRECORD with a
real reactor to an absent handle still counts as dangling.

## 7ax. The double-byte round trip, on the pinned drawings (2026-10-27, #375)

#382 wrote `CodePage::encode` and proved it on the two single-byte pages the corpus then
held (128 of 128 bytes of 1251 and of 1252); #384 named Shift_JIS and GBK and pinned
nine drawings that hold their text. This section is the proof that the inverse holds on
real double-byte strings, not on a chart: `examples/probe_code_page.rs` decodes every
string of every pre-R2007 drawing with `CodePage::decode` and encodes the text back, and
fails when the bytes differ from the bytes read.

| Header short | Strings above ASCII, decoded | Encode back to their bytes | Hold U+FFFD | Differ |
|---:|---:|---:|---:|---:|
| 38 (`ANSI_932`, Shift_JIS) | 21 | 21 | 0 | 0 |
| 39 (`ANSI_936`, GBK) | 16 | 12 | 4 | 0 |

The 21 Shift_JIS strings are the names of the two Japanese layouts (`83 8C 83 43 83 41 83 45
83 67 31` and `...32`, five two-byte characters and a digit), each as a dictionary entry and as
the name of a LAYOUT object, in the four AC1014 drawings, and five more dictionary entries
of `3d.dwg`; the 12 GBK strings are the two layout names (`B2 BC BE D6 31` and `...32`, two
two-byte characters and a digit) in the same two places: the four AC1018 drawings hold the
first, the AC1015 one both. Every
string that decodes without an invalid sequence encodes to the bytes it came from,
including those whose trail bytes are ASCII letters (`43`, `41`, `45`). The 4 that hold
U+FFFD are the four 1,028-byte non-text "descriptions" of the BLOCK_HEADER records that
error (#386, ARCHITECTURE.md 7au): bytes that are not text, so some of their pairs are not
GBK, and an invalid sequence is not text to write back; the probe counts them and does
not encode them. The check lives in the probe, so every row a later change adds is held
to it.

`CodePage::encode_char` returns bytes only when they decode back to the character (#382),
which is what keeps this round trip exact on the WHATWG encoders' extra mappings, none of
which the pinned strings use.


## 7ay. AcDb:Classes as AutoCAD writes it, and a writer that matches it (2026-10-27, #45)

`write_class_map` was "not a byte-exact AutoCAD emitter" (#45): it round-tripped through
`ClassMap::parse` and no further. `examples/probe_class_roundtrip.rs` reads each
drawing's `AcDb:Classes` as stored (decompressed on R2004 and later), parses it and
writes it back for the drawing's own release, both under the code page its header names,
and compares the bytes; where they differ it names the first differing byte and the part
of the section it falls in. A drawing it cannot open, read or parse is counted as a
failure, and `--strict` fails on any difference, any failure and a run with no match.

The Corpus job runs it with `--strict` on every pinned drawing (step "Class table written
back byte for byte on every pinned drawing", log `class-roundtrip-probe.log` in the
`corpus-measurement` artifact); that run is the record. The figures below were measured
locally with `python3 -I tools/corpus/fetch.py --dest ../dwg-corpus` and
`cargo run --release --example probe_class_roundtrip -- ../dwg-corpus --strict`, before
builds moved to the runners: the old writer matched none of the 52 drawings pinned then
(the size word at byte 16 already differed on all of them), the writer in this change
matched all 52, and 61 of 61 over the whole local corpus cache (the 52 and nine more from
three other public sets, which `tools/corpus/pins.tsv` pins since).

| Release | Drawings, then pinned | Match, old writer | Match, now |
|---|---:|---:|---:|
| R14 (AC1014) | 8 | 0 | 8 |
| R2000 (AC1015) | 8 | 0 | 8 |
| R2004 (AC1018) | 8 | 0 | 8 |
| R2007 (AC1021) | 8 | 0 | 8 |
| R2010 (AC1024) | 8 | 0 | 8 |
| R2013 (AC1027) | 8 | 0 | 8 |
| R2018 (AC1032) | 4 | 0 | 4 |

The layout is §10.1 (R13 to R15) and §10.2 (R18 and later) of ODA v5.4.1, with the fields
the spec leaves open taken from the bytes:

```text
SN 16 bytes                       start sentinel
RL                                size of the class data area, in bytes
(R2010+) RL                       0
(R2007+) RL                       size of the class data in bits, plus 32
class data, a bit stream (byte 20 on R14 to R2004, 24 on R2007, 28 on R2010+):
  (R2004+) BL max class number, B true
  records; before R2007 each with its three TVs inline; from R2007 the
  non-string fields of all records, then the 3 x N strings, then (the
  string-stream trailer) an RS with the string block's size in bits, preceded
  by an RS of its high bits when the size is 0x8000 or more (the low word then
  has bit 15 set), then a B true
  padding to the byte boundary, the padding bits set
RS CRC-8, seed 0xC0C1, over every byte from the size word to here
SN 16 bytes                       end sentinel 72 5E 3B 47 3B 56 07 3A 3F 23 0B A0 18 30 49 75
(R2004+) 8 bytes                  zero
```

What each departure from the old writer rests on:

| Rule | Measured on |
|---|---|
| R13 to R15 have no `BL` maximum and no `B` before the first record, and no trailing zeros | `line_R14.dwg`: section 839 bytes, size 801, CRC at byte 821, end sentinel at 823; `line_2000.dwg`: 742 bytes, size 704, CRC at 724, end sentinel at 726. All 16 R14 and R2000 pinned drawings match |
| Pre-R2007 `TV`s carry a terminating NUL that the `BS` length counts; R2007+ strings have none | all 24 R14, R2000 and R2004 pinned drawings match with the NUL written, all 28 R2007 to R2018 drawings without one (`read_tv` drops a trailing NUL, so the parsed table never shows it) |
| On R2007 and later the byte size is the stream plus 4 (the bits `RL`) | `line_2010.dwg`: size 1090 = 4 + 1086, section 1140 bytes; `line_2013.dwg`: 985 = 4 + 981, 1035 bytes; `sample_AC1032.dwg`: 6246 = 4 + 6242, 6296 bytes. On R2004 it is the stream alone: `line_2004.dwg` 646, section 692 bytes |
| The bit size is the stream's bits plus 32, the stream ending after the trailer | `line_2010.dwg`: 8714 = 8682 + 32; `line_2013.dwg`: 7875 = 7843 + 32; `sample_AC1032.dwg`: 49962 = 49930 + 32 |
| The CRC covers every byte from the size word to the end of the data, so on R2007+ it includes the bit-size words (and on R2010+ the zero `RL`), and never the start sentinel | R14, R2000, R2004, R2010, R2013 and R2018 files, checked with an independent Python port, then all 52 pinned drawings through the Rust writer. The 2026-10-25 note on #45 had R2010 and R2013 matching none of the coverages tried |
| The padding bits that round the class data up to a byte are ones | the writer matched only after they were set; it wrote them clear first |
| R2004+ sections end with eight zero bytes after the end sentinel | every R2004 to R2018 drawing; §10.2 says "the ODA writes 0 bytes", AutoCAD writes eight |
| A string block of 0x8000 bits or more takes two `RS` words, the high word first | `sample_AC1032.dwg`: 45,804 bits = high word 1, then 0xB2EC |

What the section holds that a `ClassDef` does not, the two unknown `BL`s of an R2004+
record and the `B` after the maximum class number, is written as `0` and `true`; every
one of the 52 sections matches with those values, so no drawing carries anything else.
Unmeasured: R13 (AC1012), for which the corpus holds no AutoCAD-written file, so its
section is written by the R14 rule on the strength of §10.1 alone. The canonical
fixtures on R2004 and later have no `AcDb:Classes` section, and the R2000 fixture's
empty one is framed by `build_fixtures` itself, so none of them exercises this writer.



## 7az. dwg-to-dxf: the LAYOUT, GROUP and MLINESTYLE objects (2026-10-27, #126)

`dxf_objects` writes three more kinds, each from the decoder that already closed on
the corpus. A **LAYOUT** is its plot settings (`AcDbPlotSettings`: page setup, printer,
paper size, plot view name, the six margins and sizes, origin, window, scale,
flags, units, rotation, type, style sheet, scale type and factor, and from R2004 the
shade plot triple) then `AcDbLayout` (name, flags, tab order, limits, insertion base,
extents, elevation, UCS, orthographic type) and its handles. The export's order
puts `70` of the plot settings after `143` and the shade plot triple after `147`;
the handle stream lists, from R2004 the plot view, from R2007 the visual style, then
the block record (`330`), the last active viewport (`331`) and the base and named
UCS (`346`, `345`), of which the null ones are not written, and so is any that names a
record the output does not define. The plot view name (`6`) is the DWG's own up to
R2000; from R2004 the DWG stores a handle instead, and the name written is that of the VIEW
record it names. A **GROUP** is its description (`300`), the unnamed and selectable flags
and one `340` per member the output holds (an MLINE or VIEWPORT member, which the entity
writer does not write yet, is left out, counted by the probe). An
**MLINESTYLE** is its name, flags (the DWG's bits are not DXF's: bits 1, 2, 32, 64,
512 and 1024 swap in pairs), description, fill colour, angles in degrees and one
offset, colour and linetype per line; a linetype is `BYLAYER` or `BYBLOCK` for the
two sentinel indexes, and any other index, or any R2018 record (which names the
linetype by handle), is not written.

| Kind | Written | Paired | Groups | Agree | Relation | Disagree | Extra | Groups omitted |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| LAYOUT | 132 | 132 | 8182 | 8182 | 0 | 0 | 0 | 126 |
| GROUP | 12 | 12 | 144 | 144 | 0 | 0 | 0 | 0 |
| MLINESTYLE | 40 | 40 | 640 | 640 | 0 | 0 | 0 | 0 |

The 126 groups a LAYOUT omits are extended data (1000 to 1070, the `ACAD_PSEXT`
application data, 108) and the last active viewport (`331`, 18): it names a VIEWPORT
entity, which is not written yet, so the group is left out and the probe names the reason.
The probe compares every `330` after the record's owner, so a LAYOUT's block record is
checked. The two R2018 MLINESTYLEs of the corpus are the records not
written. With the layouts written, a BLOCK_RECORD's `340` and the dictionary entries
of `ACAD_LAYOUT` are written too (132 more entries), and the tables probe still
agrees. Not corpus-measured: a non-null base or named UCS (`345`, `346`), a layout that
plots a named view (none of the 132 has plot type 3 or a non-empty group `6` in the
export, so the plot view handle's resolution to the VIEW record's name is by the rule
and not by a drawing), a visual style handle (`333`), an MLINESTYLE whose linetype is
neither sentinel, and R2018 MLINESTYLE. A LAYOUT whose plot settings, limits, elevation or
any of its six 3D points (insertion point, extents, UCS origin and axes) holds a NaN or an
infinity, which `read_bd` returns as read, is not written: the DXF reader rejects either.
No pinned layout holds one (all 132 are written), so a synthetic check of the predicate
(`layout_is_finite`, the one the planner uses) refuses all 63 cases, a NaN, an infinity and a
negative infinity in each of the 18 coordinates of the six points and in one real of each other kind.

## 7ba. dwg-to-dxf: the VISUALSTYLE objects (2026-10-27, #66)

`dxf_objects` writes the 24 built-in VISUALSTYLEs of every drawing from the fields
`acad_visual_style` decodes (the layout the module derives from bytes, since §20.4 has no
prescription for the object), under the groups AutoCAD's own DXF export gives them, which are the
mapping the value oracle (#279) already scores. The export has three layouts, by the release the
drawing was saved by, and the writer follows the DWG's own:

- **R2000 to R2007.** `100 AcDbVisualStyle`, `2` the name, `70` the style, then one group per
  property in the order 71, 72, 73, 90, 40, 41, 63, 74, 91, 64, 65, 75, 175, 42, 92, 66, 43, 76, 77,
  78, 67, 79, 170, 171, 290, 93, 44, 173, and the internal-use flag `291` last. A true colour is its
  RGB under 421 to 425 (63 plus 358: 63 to 421, 66 to 424, 67 to 425) and, before R2004, a colour is
  the bare index.
- **R2010.** The head `70`, `177` (format version), `291`, then the same properties in the same
  order, each followed by its `176` flag.
- **R2013 and R2018.** The head, then `70` as the count of the property list (58), then every
  property as a typed group with its `176` flag: `90` an integer, `40` a real, `290` a boolean, `62`
  and `420` a colour, `1` a text. The first 28 are the named ones, the other 30 the decoder's
  `extended` list, whose names are not determined and which are written positionally.

`examples/probe_dxf_objects.rs` pairs every record with the export's of the same handle, group for
group and in the export's order. Over the 42 drawings of the corpus that have an export of their
release (R2000 to R2018):

| Kind | Written | Paired | Groups | Agree | Relation | Disagree | Extra | Groups omitted |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| VISUALSTYLE | 1008 | 1008 | 62208 | 61944 | 264 | 0 | 0 | 5468 |

1,008 is 24 styles in each of 42 drawings, all paired. The styles are named in the
`ACAD_VISUALSTYLE` dictionaries too (their entries were left out while the object was not
written). These counts are the VISUALSTYLE row of the Corpus job's probe log in CI run
40600119327, on the head that landed (#413, a195ddf), with the ownership closure of §7aw in
place; the reasons below are that log's own.

What is not written, each counted by the probe with its reason:

- **A true colour's nearest index** (62 to 67, 1,364 groups, each with the colour's RGB beside
  it). The export derives it from the RGB; the DWG stores 0 in the colour's index word, and the
  derivation is not known from the corpus (the export gives 7 for white, 8 for both `0x808080`
  and `0x787878`, 18 for black and 5 for blue), so it is left out rather than guessed.
- **Three constants of the R2000 to R2007 export**: `62` (5), `174` (0) and `45` (0.0), the same in
  every style of those releases (576 each). The DWG has no field that varies with them (the
  decoder holds the unnamed `edge_unknown_byte` and, on R2007, `display_unknown_short`, both 0), so
  they are not written.
- **Extended data** (1000, 1001, 1070) on 792 records, not decoded for objects.

Two values differ from the export and are named relations, not agreement: the format version
`177` is 3 in the export of an R2013 or R2018 drawing where the DWG stores 2 (240 records), and
`Realistic`'s lighting quality (`72`) is 3 in the export where an R2000 to R2007 DWG stores 2 (24
records, the same style `acad_visual_style` documents as clamped when saved down). A VISUALSTYLE
is written in the layout of the DWG's release whatever release the output targets, as the
LAYOUT is; the probe measures each drawing against the export of its own release only.


## 7bb. MATERIAL, derived from AutoCAD's own output (2026-10-26, #49, #152)

ODA spec v5.4.1 lists MATERIAL in §20.3 and prints no layout for it (§20.4
has no entry), and the 65 records measured in §7d fixed only the budgets a
field list had to fill. The layout in `src/objects/acad_material.rs` is
derived by the method of CLEANROOM.md (2026-10-26): the data bits of every
MATERIAL record in the pinned public corpus, read against the `AcDbMaterial`
AutoCAD exported to DXF for the same handle. All 212 records close, on all
seven releases.

| Release | Records | Close | Values compared | Differ |
|---|---:|---:|---:|---:|
| R14 | 32 | 32 | none, no DXF twin | |
| R2000 | 32 | 32 | 2,509 | 0 |
| R2004 | 32 | 32 | 2,509 | 0 |
| R2007 | 32 | 32 | 2,528 | 0 |
| R2010 | 32 | 32 | 2,528 | 0 |
| R2013 | 32 | 32 | 2,528 | 0 |
| R2018 | 20 | 20 | 456 | 0 |
| Total | 212 | 212 | 13,058 | 0 |

The records are the three defaults (`ByLayer`, `ByBlock`, `Global`) of 7
nextgis drawings saved by AutoCAD 2016 (R14 to R2013, 21 per release) and
`sample_base`'s 11 per release saved by AutoCAD 2025 (R14 to R2018): the three
defaults and eight library materials (`Beech`, `Bamboo`, `Aluminum Cast`,
`Exposed Aggregate`, `1.5in Square - Medium Blue`, `Asphalt - Wet`, `Ashlar -
Patterned`, `Blue`). The R2018 band also holds the three defaults of
`empty.dwg`, `export_sample.dwg` and `geoloc.dwg` (9 records, three with a
twin).

### The field list

```text
TV name, TV description
colour group x3 (ambient, diffuse, specular):  RC method, BD factor, BL colour if method is 1
map block x6:  BD blend, RC x3, BD x16 matrix, RC source, TV file if source is 1
```

in the order ambient, diffuse, diffuse map, specular, specular map, `BD` gloss,
reflection map, `BD` opacity, opacity map, bump map, `BD` refraction index,
refraction map; and from R2007 only, `BD`, a 2-bit zero, `BD`, `BS`, `BS`, a
2-bit zero. R14, R2000 and R2004 have no tail. Before R2007 the `TV`s are
inline (the name and description first, each file name right after its
source), from R2007 they are in the string stream in the same order.

How it was found, so that it can be checked:

- **Anchors.** Each DXF value was searched in the record's bits as a full `BD`
  or `BL`. `Blue`'s colour `0xC24C4CFA` sits at bits 12, 56 and 166, the
  first two behind the same ten bits (`RC 1`, `BD 1.0`) and a `BL` code; the
  bump blend 0.515 at 464, the refraction index 1.5 at 594. The gaps between
  anchors are whole blocks of 66 bits: `BD 1.0` (2), three `RC` of 2, 1 and 1
  (24), an identity matrix (32) and an `RC` (8).
- **Why 516 and 1,284.** The same release, R2013, holds a default MATERIAL of
  1,284 bits (nextgis, AutoCAD 2016) and of 516 (`sample_base`, AutoCAD
  2025). AutoCAD 2016 writes each of the six matrices with its diagonal at
  1/48 (two 66-bit `BD` against two 2-bit ones), 6 x 2 x 64 = 768 bits, and
  the difference is 768.
- **The source flag.** The five map sources equal DXF groups 72, 77, 171,
  175 and 179 wherever the DXF writes them (always 0, in 111 to 153 records
  each),
  and the DWG holds 1 in all 183 cases where the DXF omits the group. The
  refraction source is 0 in every record and has no DXF group.
- **The block order.** The DXF writes a matrix group only when the matrix is
  not the identity (320 of 320 omissions). `Global` in `sample_base` writes
  groups 43, 49, 142 and 144, and its non-identity blocks are the first,
  third, fourth and fifth; the library materials write 43 and 144, and their
  blocks one and five.
- **The strings.** From R2010 a `TV` costs no data-stream bit, so the boundary
  cannot see the `source` conditional; the decoder requires the string stream
  to be exhausted instead (2 plus one entry per map with source 1, on all
  116 R2007+ records), and the inline releases confirm the placement: R2004's
  `ByLayer` is 582 bits, which is R2007's 516 less its 20-bit tail plus 86
  inline string bits (name 74, description 2, five empty file names of 2
  each after the five sources of 1).
- **The tail.** Group 94, the bit set of maps with a file, is 17 (diffuse and
  bump) on the library materials, 1 on `Aluminum Cast`, 0 on `Blue`, 63 on
  the 2025 defaults and 127 on the 2016 ones. It is the second `BS` of
  the tail in 47 compared records.

### Values compared with the DXF (`probe_material`)

| Field | Group | Compared | Differ |
|---|---|---:|---:|
| name | 1 | 174 | 0 |
| description | 2 | 48 | 0 |
| diffuse file, bump file | 3, 8 | 36, 30 | 0 |
| ambient, diffuse, specular method | 70, 71, 76 | 48 each | 0 |
| ambient, diffuse, specular colour | 90, 91, 92 | 48 each | 0 |
| map sources | 72, 77, 171, 175, 179 | 111, 153, 153, 153, 117 | 0 |
| matrices (16 values each) | 43, 47, 49, 142, 144, 147 | 2,368, 1,680, 1,792, 1,792, 2,272, 1,680 | 0 |
| gloss, opacity | 44, 140 | 48, 6 | 0 |
| bump blend, refraction index | 143, 145 | 42, 36 | 0 |
| tail | 148, 468, 93, 94 | 8, 20, 4, 47 | 0 |

```sh
python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
cargo run --release --example probe_material -- ../dwg-corpus
```

### Unverified by value

Their width is claimed, nothing more, and they are named `unknown_*` or
documented so in the module. Every value is the one shown; the controlled
drawings should vary each.

- The three colour factors: `BD 1.0` in every record, written by no DXF group.
- The blend of the diffuse, specular, reflection, opacity and refraction maps
  (1.0 everywhere; only the bump blend is checked, against group 143).
- The three `RC` after every blend: 2, 1, 1 in all 1,272 blocks. No DXF
  group shows them, and the split of the 24 bits into three bytes is itself
  unverified.
- The two 2-bit tail slots (always the zero code) and the refraction map's
  source (always 0). A tail slot that is not the zero code, a colour method
  or a map source other than 0 and 1, and a group 93 or 94 wider than the
  `RC` form are refused with `Error::Unsupported`, never read on a guess.
- Which of ambient, diffuse and specular owns which of groups 70/71/76 and
  90/91/92: the three carry one value in every record.
- Which of the specular, reflection, opacity and refraction blocks owns
  which of groups 47, 49, 142 and 147, where the DXF writes the same
  matrix for several (the nextgis defaults write six equal ones).
- Every map's file name but the diffuse and the bump, which no record
  gives a non-empty name.

The R2018 DXF writer in `dxf_sections.rs` still emits a MATERIAL as its name
only; surfacing these fields there, and a MATERIAL scorer in the DXF value
oracle, are separate changes.

## 7bc. TABLESTYLE from R2010, and CELLSTYLEMAP (2026-10-26, #151)

§20.4.101.2 prints TABLESTYLE's R2010 layout and §20.4.102 CELLSTYLEMAP,
both as sequences of cell styles (§20.4.101.4, with the content format of
§20.4.101.3 inside them). One reader serves both
(`src/objects/table_cell_style.rs`): a `BL` type, a `BS` data flag, and when
the flag is 1 the override and merge flags, a background colour, the layout
flags, the content format, the margins and the borders, each border an edge
flag and, when that is not 0, its overrides, type, colour, line weight,
invisibility and double-line spacing. §7d's note on TABLESTYLE called its
R2010 record a 6,844-bit envelope whose token sequence
no record could pin; with the spec's field list the record closes on all 20
R2010 to R2018 records, and so does CELLSTYLEMAP on all 32 of R14 to R2007.

Three readings are not in the spec's table and come from the bits:

| Where | What the spec prints | What the bytes say |
|---|---|---|
| §20.4.101.2, p234 | the cell styles "(for data, title, header in this order)" | title, header, data: the names read `_TITLE`, `_HEADER`, `_DATA` with ids 1, 2, 3, in `arc_2010.dwg` handle `0x65` and every other record |
| §20.4.101.2, p234 | each repeated cell style is the §20.4.101.4 fields, then `BL` id, `BL` class and `TV` name | one more `BL` before each, equal to the id after it on every record (1, 2, 3); without it the first named style lands 10 bits early |
| §20.4.101.2, p234 | `RC`, `TV`, `BL`, `BL`, `H`, then the "Table" cell style | the two `BL`s are 0 and 101 on every record, 101 being the first custom cell style id (§20.4.101.3) |

The `TC` of R2000 follows the until-R2007 TABLESTYLE section: every colour of a cell style
is the 44-bit `BS`, `BL`, `RC` form on every release. A cell style's border
count is 0 to 6: `sample_AC1032.dwg`'s "Table" style has none, which is why
that record is 5820 bits against R2013's 6844.

The DXF holds the cell styles in the drawing's one CELLSTYLEMAP object, group
by group in the order the spec prints the fields, bracketed by marker strings
(`TABLEFORMAT_BEGIN`, `CONTENTFORMAT_BEGIN`, `GRIDFORMAT_BEGIN`, ...).
AutoCAD writes that object afresh on every save, so the DWG's CELLSTYLEMAP
and the DXF's never share a handle (the oracle lists it among its save-time
objects, `examples/dxf_oracle/main.rs`); each DWG record is paired with the
only CELLSTYLEMAP in its twin. From R2010 the DWG holds no
CELLSTYLEMAP, and the same twin object is compared with the cell styles of the
TABLESTYLE record, whose "Table" style (id 4) the DXF holds nowhere.

```sh
cargo run --release --example probe_tablestyle -- ../dwg-corpus
cargo run --release --example probe_cellstylemap -- ../dwg-corpus
```

| Release | TABLESTYLE close | Values compared | CELLSTYLEMAP close | Values compared | Differ |
|---|---|---|---|---|---|
| R14 | 8 / 8 | no twin | 8 / 8 | no twin | |
| R2000 | 8 / 8 | 608 | 8 / 8 | 2360 | 0 |
| R2004 | 8 / 8 | 608 | 8 / 8 | 2360 | 0 |
| R2007 | 8 / 8 | 680 | 8 / 8 | 2360 | 0 |
| R2010 | 8 / 8 | 2368 | none in the DWG | | 0 |
| R2013 | 8 / 8 | 2368 | none in the DWG | | 0 |
| R2018 | 4 / 4 | 592 | none in the DWG | | 0 |

## 7bd. TABLEGEOMETRY (2026-10-26, #151)

§20.4.103 prints the laid-out geometry of a table, an object AutoCAD 2008
added beside TABLECONTENT and that "does not need to be present": `BL` row
count, `BL` column count, `BL` their product, then per cell a `BL` flag, two
`BD` (width and height with gap), a handle, a `BL` content count and per
content two `3BD` distances, four `BD` and a last unknown. The handle is in the
handle stream. The record had no decoder, so all 14 of the corpus's
(two tables on each of R14 to R2018, the ACadSharp sample set) were skipped.

The list closes as printed on all 14, on each release's data-stream
boundary. The last value is typed `BD` in the spec and `BL` in the DXF (group
95); a `BD` of 0 and a `BL` of 0 are the same two bits, `10`, and every record
holds 0, so the width cannot choose between them. It is read as `BL`, and a
non-zero value would break the boundary.

Against AutoCAD's DXF export (R2000 to R2018) all values equal the groups
that carry them, 3462 in all: the counts (90, 91, 92) and per cell the flag (93),
the two sizes (40, 41), the content count (94) and per content the distances
(10 to 31), the sizes (43 to 46) and the unknown (95). The first table's objects
have the same handles in both files; the second table's TABLEGEOMETRY and
TABLECONTENT were created while saving and have other handles in the DXF, so the probe pairs a DWG record that finds no DXF record of its handle
with the next unpaired DXF record of the type, in file order. R14 has no twin.

```sh
cargo run --release --example probe_tablegeometry -- ../dwg-corpus
```

| Release | Records | Close | Values compared | Differ |
|---|---|---|---|---|
| R14 | 2 | 2 | no twin | |
| R2000 | 2 | 2 | 577 | 0 |
| R2004 | 2 | 2 | 577 | 0 |
| R2007 | 2 | 2 | 577 | 0 |
| R2010 | 2 | 2 | 577 | 0 |
| R2013 | 2 | 2 | 577 | 0 |
| R2018 | 2 | 2 | 577 | 0 |

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
