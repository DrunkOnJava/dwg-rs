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
├── r2007.rs            — R2007-specific two-layer Sec_Mask (partial)
├── reed_solomon.rs     — (255,239) FEC over GF(256), defensive recovery
│
├── lz77.rs             — Decompressor (the main read-path hot loop)
├── lz77_encode.rs      — Encoder (literal-only, correctness-first)
│
├── header.rs           — 0x80-byte file header + R2004+ encrypted header
├── section.rs          — Section name enum + kind classification
├── section_map.rs      — Page Map + Section Info parser
├── section_writer.rs   — Page emitter (inverse of section_map)
│
├── handle_map.rs       — AcDb:Handles (object-stream index)
├── classes.rs          — AcDb:Classes (custom-type dispatch table)
├── header_vars.rs      — AcDb:Header variable table (raw bit-stream)
├── metadata.rs         — SummaryInfo + AppInfo + Preview + FileDepList
│
├── object.rs           — Object-stream walker + RawObject type
├── object_type.rs      — 80+ built-in type codes + Custom(N) fallback
├── common_entity.rs    — Shared §19.4.1 preamble for every entity
│
├── entities/           — Per-entity decoders (27 types)
├── tables/             — Symbol-table entry decoders (9 tables)
├── objects/            — Control / dictionary / xrecord decoders
│
├── reader.rs           — DwgFile — the primary public entry point
├── file_writer.rs      — Scaffolded inverse of reader (Stages 1/5 shipped)
│
├── error.rs            — Error enum (thiserror)
├── version.rs          — Version enum + AC-magic mapping
│
└── bin/                — 4 CLI tools (dwg-info, dwg-corpus, dwg-dump, dwg-convert)
```

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
is how the R2007+ VIEW ambient-colour form, the VPORT flag block, the
DIMSTYLE `AcDs`-binary-data byte and TEXT's `RD` height were each
found.

### Other findings that fell out of the same work

| Finding | Evidence |
|---|---|
| Shared table-entry fields read `B 64-flag`, `B xdep`, `BS xrefindex+1` — not `B, BS, B` | Only that order keeps the `BS` on a 2-bit `10` code across every STYLE record in `sample_AC1032.dwg`; the alternative yields xref index 83 for `Standard` |
| VIEW / VPORT / LAYER / DIMSTYLE colours use a full `CMC`: `BS` index + `BL` true colour + `RC` byte, unconditionally | VIEW `view_custom` ambient reads index 0 with no flags yet is followed by `BL = 0xC2333333`; LAYER `0` reads `0xC3000007` |
| `BL` top byte selects the colour form: `0xC3` = ACI index in the low byte, `0xC2` = literal RGB | `Layer_color_80` reads ACI 80; `Layer_true_color` reads `0xC2…` |
| The R2013+ "has AcDs binary data" bit is followed by an `RC` | The two DIMSTYLE records in `sample_AC1032.dwg` with the bit set need exactly 8 more prefix bits than the four with it clear |
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
B    has_ds_binary_data      -- R2013+ (an RC follows when set)
```

Measured three independent ways: DICTIONARY's `BL numitems` decodes to
the number of strings its string stream actually holds only with the
`BL` consumed; ACDB_PLACEHOLDER, whose body is provably empty, closes
exactly; and the nine one-field `*_CONTROL` records close on their
`BL numentries`. Note that `tables/modern.rs` deliberately does *not*
read this `BL` and compensates with a different flag order — the two
readings sum to the same total for an APPID record, so both satisfy the
boundary check, and which one assigns the right values to the right
fields is not determinable from the 16 bits an APPID record spends
there.

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
- **The R2013+ `has_ds_binary_data` bit carries 16 further bits.** The
  spec gives only the `B`. Four corpus records set it — the four
  LAYOUTs of `sample_AC1032.dwg`, out of 331 non-entity records across
  the R2013 and R2018 files — and on all four the field list closes
  only with 16 bits consumed after the flag, the first byte `0x2C` on
  every one. `objects::modern` previously consumed a single `RC` there,
  an unvalidated guess no record had ever reached. See that module's
  docs for the reading this crate takes and the one it cannot rule out.

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
keeps its `RC` for now: that path also omits the `BL num_reactors`
`objects/modern.rs` reads, so its bit accounting has a second unresolved
variable and is out of scope here. Three record types needing three
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
| VIEWPORT | 6 (R2018) | −819 | the decoder reads 266 of 1125 bits by design | reported |
| MESH | 2 (R2018) | −2 | two trailing bits, `10` on both records | reported |
| LEADER | 1 (R2018) | −12 | field list stops before the text-box block | reported |

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
| MESH's trailing `10` | `entities/mesh.rs` | `BS` 0, `BL` 0, `BD` 0.0 and "two spare bits" all encode identically | a MESH whose trailing value is non-zero |

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
- **`#![deny(unsafe_code)]`** in `lib.rs` — all code is safe Rust.
  Reed-Solomon + LZ77 + GF(256) all implemented without `unsafe`.

## 10. Test strategy

Four layers:

1. **Unit tests** (156) — co-located with each module; typically
   one test per public function plus edge-case coverage.
2. **Property tests** (9) — `tests/proptest_roundtrip.rs` uses
   `proptest` for randomized round-trip of every bit-level
   primitive. Each property runs 256 cases by default.
3. **Corpus integration** (5) — `tests/corpus_roundtrip.rs`
   verifies invariants across all 19 sample DWG files (opens,
   section enumeration, metadata accessors never panic).
4. **Per-sample assertions** (22) — `tests/samples.rs` asserts
   specific values for specific sample files (version detection,
   section counts, etc.).

`cargo test --release` must print `test result: ok` for every
block before a PR is mergeable.

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
