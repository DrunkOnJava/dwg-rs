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
| `read_3b`       | 1-3 bits (early-stop on 0)                       | `{0, 2, 6, 7}`           |
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
   [count][handle₁][offset₁][handle₂ delta][offset₂ delta]...
            └─────────┐                 ┌────┘
                      ▼                 ▼
                 handle values     signed MC deltas from previous
```

`HandleMap::parse` walks this index, applies the deltas to recover
absolute handles and offsets, and returns a sorted list. The walker
then seeks to each offset, reads the object's size + type code + body.

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
