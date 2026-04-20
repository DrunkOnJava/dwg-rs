# Compatibility Matrix

> **Status: pre-alpha.** The matrix below reflects what currently
> parses and/or decodes end-to-end against measured corpora, not what
> the ODA spec describes. The source of truth for coverage deltas is
> the version table in
> [`src/lib.rs`](../../src/lib.rs) and the empirical decode-rate
> table in [`README.md`](../../README.md) — this page merges the two
> into a single per-version view.

Legend:

| Symbol   | Meaning                                                                                      |
|----------|----------------------------------------------------------------------------------------------|
| `ok`     | Works end-to-end against the measured corpus for this version.                               |
| `partial`| Works for a documented subset; gaps tracked in the referenced issue.                         |
| `pending`| Not implemented. Calling the relevant API returns `None` or `Error::Unsupported`.            |
| `n/a`    | Not applicable to this version (e.g. R14 has no Sec_Mask, so that column is `n/a` not gap).  |

## Per-version matrix

| Magic    | Release                 | Year   | Container parse | Metadata parse | Object walker | Per-entity decoder | Geometry export | Write   |
|----------|-------------------------|--------|------------------|----------------|---------------|--------------------|-----------------|---------|
| `AC1014` | R14                     | 1997   | ok               | ok             | pending       | pending            | pending         | pending |
| `AC1015` | R2000 / 2000i / 2002    | 1999   | ok               | ok             | pending       | pending            | pending         | pending |
| `AC1018` | R2004 / 2005 / 2006     | 2003   | ok               | ok             | ok            | partial (0% real)  | pending         | pending |
| `AC1021` | R2007 / 2008 / 2009     | 2006   | partial          | pending        | pending       | pending            | pending         | pending |
| `AC1024` | R2010 / 2011 / 2012     | 2009   | ok               | ok             | ok            | partial (43% real) | pending         | pending |
| `AC1027` | R2013 / 2014-2017       | 2012   | ok               | ok             | ok            | partial (86% real) | pending         | pending |
| `AC1032` | R2018 / 2019-2025+      | 2017   | ok               | ok             | ok            | partial (22% real) | pending         | pending |
| `AC10??` | R32 / future            | future | n/a              | n/a            | n/a           | n/a                | n/a             | n/a     |

Notes, column by column:

- **Container parse.** Identifier detection, file-open header, section page map, section info, LZ77 decompression, Sec_Mask layer-1. The only `partial` is R2007 because its Sec_Mask layer-2 bit-rotation is scaffolded but not finished; section payloads for R2007 currently error rather than decode.
- **Metadata parse.** `SummaryInfo`, `AppInfo`, `Preview`, `FileDepList`. Works across every version the container layer parses. Auto-detects UTF-16 for R21+ and carves a PNG thumbnail from R24+ preview streams.
- **Object walker.** `all_objects()` returns every `RawObject` with type code, handle, and raw payload bytes. Works on R2004 and newer (handle-map-driven walk). R14 / R2000 use a different object-stream layout that is not yet implemented (issue #104). R2007 is blocked by the Sec_Mask layer-2 gap above.
- **Per-entity decoder.** The 27 typed decoders in [`src/entities/`](../../src/entities/) are verified against hand-crafted synthetic bit streams (193 unit + proptest tests pass). Real-file decode rates are what the `(… % real)` numbers in the table report, measured by `examples/coverage_report.rs`. The aggregate real-file decode rate across all corpora is currently 25 %. Closing this gap is the 0.2.0 ship bar — see [`ROADMAP.md`](../../ROADMAP.md).
- **Geometry export.** SVG / PNG / PDF / glTF output. The `svg` module exists in [`src/svg.rs`](../../src/svg.rs) but the full export path is pending until per-entity decoders stabilize — rendering broken geometry would publish confidently-wrong pictures.
- **Write.** `file_writer.rs` is stage 1 of 5. LZ77 literal-only encoder works; Reed-Solomon encoder is tracked by issue #109; stages 2-5 (section encoding, buffer assembly, CRC splicing, file-level write) are the 0.4.0 milestone.

## What "partial" looks like in practice

Consider the R2013 row. Container parse is `ok`, metadata parse is `ok`, object walker is `ok` — that means you can open any R2013 file, read metadata, and enumerate every object by handle. Per-entity decoder is `partial (86% real)` — meaning of the seven "hot" entity types `coverage_report.rs` probes per file, six decode cleanly and one errors. The error is typically a common-entity preamble misalignment that the synthetic unit tests don't exercise because they feed the decoder a hand-aligned bit stream.

The honest consequence: today on R2013 you can trust `DwgFile::version()`, `file.summary_info()`, `file.all_objects()`, and most `entities::*` decoders when you dispatch by hand. You cannot yet trust the fully automated `file.decoded_entities()` pipeline to cover 100 % of entities on arbitrary drawings — even though on R2013 specifically it gets most of the way there.

## R2007 is deferred on purpose

R2007 is the only version where container parse is `partial` rather than `ok`. Its section-payload obfuscation adds a second bit-rotation layer on top of the R2004-family Sec_Mask. Implementing it without an authoritative reference has been a rabbit hole; this crate's posture is to ship R2007 support only when the layer-2 bookkeeping is correct end-to-end, rather than shipping something that half-works and silently returns wrong bytes. Issue-tracked under the `0.3.0` milestone.

## How to test against your own files

The fastest way to see what `dwg-rs` does on a file you care about is the bundled `coverage_report.rs` example:

```bash
cargo run --release --example coverage_report -- path/to/file.dwg
```

Point it at a single file or a directory. Output includes:

- Detected version magic
- Container parse result
- Section list (name, size, offset)
- Metadata availability flags
- Object count + per-type code distribution
- Per-entity decode attempt / success / error counts
- A per-entity-type error concentration summary

If the tool reports `Err(...)` on something you think should work, that is a useful bug report — file an issue tagged with the magic (e.g. `AC1032`) and attach the output. Small reproducers are always welcome; we ask that contributors confirm the file is redistributable before attaching it to a public issue.

## How this page stays honest

The matrix at the top is derived from two machine-checked sources:

1. The version table in [`src/lib.rs`](../../src/lib.rs) — a rustdoc comment that lives next to the code it describes.
2. The output of `cargo run --release --example coverage_report` over the measured corpus in CI.

When those two sources shift, this file shifts with them. If you see a mismatch between what the matrix says and what `coverage_report` shows on your machine, that is a bug in the docs — please open an issue.
