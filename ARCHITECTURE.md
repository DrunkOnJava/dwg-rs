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
drawing object. Records are **not** sequential — there are padding
gaps between them. The authoritative enumeration comes from
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

The end of that trailer is **not** `payload_bits - handle_stream_bits`.
It is:

```
trailer_end = payload_bits - handle_stream_bits + mc_field_bits
```

where `mc_field_bits` is the width of the leading `MC` handle-stream-size
field (8 bits for a one-byte `MC`, 16 for two).

| Evidence | Value |
|---|---|
| Objects checked | 59 |
| Object types spanned | 11 (LAYER, LTYPE, STYLE, VIEW, VPORT, APPID, DIMSTYLE, BLOCK_HEADER, TEXT, ATTRIB, ATTDEF) |
| Files | `sample_AC1032.dwg` (R2018), `line_2013.dwg` (R2013), `arc_2010.dwg` (R2010) |
| Deviation from the rule | 0 in all 59 |

Reproduce with `cargo run --release --example probe_string_stream --
<file.dwg> <type-code>`; the probe prints `delta_vs_predicted` per
object. Two readings fit the evidence — either the record's `MS` byte
count excludes the `MC` field, or the recorded handle-stream size counts
the `MC` field itself — and this crate does not need to choose.

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
and stop — are deliberately **not** dispatched, along with MLINESTYLE,
whose field list this crate has not yet matched against real bytes.

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

R2004 and R2007 VISUALSTYLE records store the same styles *without* the
per-property flags and with a different property count; no sequence
over the same token set lands their 24 records on their `RL`
object-data-size boundary, so `objects::acad_visual_style` returns
`Error::Unsupported` for those bands rather than guessing, and the
dispatcher maps that to `Unhandled`.

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
reading could not produce. Where the two disagree (DICTIONARYVAR
104 / 72, MULTILEADER 15 / 10, LAYOUT 4 / 0, CELLSTYLEMAP 18 / 3) the
shortfall is on the *walk* side, i.e. objects the handle-map walker
does not reach — a separate gap, not a class-table one.

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
