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
| `AC1014` | R14                     | 1997   | ok               | ok             | ok            | partial (48.6% real) | pending         | pending |
| `AC1015` | R2000 / 2000i / 2002    | 1999   | ok               | ok             | ok            | partial (84.4% real) | pending         | pending |
| `AC1018` | R2004 / 2005 / 2006     | 2003   | ok               | ok             | ok            | partial (97.5% real) | pending         | pending |
| `AC1021` | R2007 / 2008 / 2009     | 2006   | ok               | ok             | ok            | partial (82.8% real) | pending         | pending |
| `AC1024` | R2010 / 2011 / 2012     | 2009   | ok               | ok             | ok            | partial (97.8% real) | pending         | pending |
| `AC1027` | R2013 / 2014-2017       | 2012   | ok               | ok             | ok            | partial (97.0% real) | pending         | pending |
| `AC1032` | R2018 / 2019-2025+      | 2017   | ok               | ok             | ok            | partial (92.2% real) | pending         | pending |
| `AC10??` | R32 / future            | future | n/a              | n/a            | n/a           | n/a                | n/a             | n/a     |

Notes, column by column:

- **Container parse.** Three families, all `ok`: the R13-R15 flat section-locator list (§3.2.6), the R2004-family page map + section info with LZ77 and Sec_Mask layer-1 (§4), and the R2007 page map + section map with Reed-Solomon de-interleaving and the §5.10 LZ variant (§5.1-§5.4). Password-protected R2007 files are refused rather than mis-decoded.
- **Metadata parse.** `SummaryInfo`, `AppInfo`, `Preview`, `FileDepList`. Works across every version the container layer parses. Auto-detects UTF-16 for R21+ and carves a PNG thumbnail from R24+ preview streams.
- **Object walker.** `all_objects()` returns every `RawObject` with type code, handle, and raw payload bytes. Works on every release from R14 on. R13-R15 have no object *section*: their records sit loose in the file and the object map addresses them by absolute file offset, so `DwgFile::object_stream()` returns the whole file there. Every handle-map entry of every corpus file resolves to a record whose own handle field matches the map.
- **Per-entity decoder.** The typed decoders in [`src/entities/`](../../src/entities/) are verified against hand-crafted synthetic bit streams *and* required to end exactly on each real record's data-stream boundary. Real-file decode rates are what the `(… % real)` numbers in the table report, measured by `examples/coverage_report.rs`. The aggregate across all seven versions is currently 83.1 %, over 19 files — a number that is only comparable to itself once the corpus stops growing. Closing the remaining gap is the 0.2.0 ship bar — see [`ROADMAP.md`](../../ROADMAP.md).
- **Geometry export.** SVG / PNG / PDF / glTF output. The `svg` module exists in [`src/svg.rs`](../../src/svg.rs) but the full export path is pending until per-entity decoders stabilize — rendering broken geometry would publish confidently-wrong pictures.
- **Write.** `file_writer.rs` is stage 1 of 5. LZ77 literal-only encoder works; Reed-Solomon encoder is tracked by issue #109; stages 2-5 (section encoding, buffer assembly, CRC splicing, file-level write) are the 0.4.0 milestone.

## What "partial" looks like in practice

Consider the R2013 row. Container parse is `ok`, metadata parse is `ok`, object walker is `ok` — that means you can open any R2013 file, read metadata, and enumerate every object by handle. Per-entity decoder is `partial (15.2% real)` on the measured corpus. The remaining errors are typically version-specific table, text, insert, dimension, hatch, or flag-layout mismatches that synthetic unit tests don't exercise because they feed decoders hand-aligned bit streams.

The honest consequence: today on R2013 you can trust `DwgFile::version()`, `file.summary_info()`, `file.all_objects()`, and most `entities::*` decoders when you dispatch by hand. You cannot yet trust the fully automated `file.decoded_entities()` pipeline to cover 100 % of entities on arbitrary drawings — even though on R2013 specifically it gets most of the way there.

## R2007 was not what this page used to say it was

Earlier revisions of this page described R2007's container as the R2004 one plus "a second bit-rotation layer on top of the Sec_Mask". That was wrong, and it kept the version deferred for longer than it needed to be. §5.1-§5.4 of the ODA specification give R2007 a container that shares no *mechanism* with §4: a Reed-Solomon-encoded and separately-compressed file header at 0x80, bare data pages with no per-page header, a different LZ variant, and Reed-Solomon interleaving on the pages the section map marks. There is no Sec_Mask anywhere in it.

The verification is the spec's own constants. `AcDb:...` section names carry a tabulated hash code in §5.2, and all twelve that the table lists match on every corpus AC1021 file. The file header names seven "normally X" values and one field that must equal the file's own byte count; all eight come back correct. `examples/probe_r2007_container.rs` prints them with a pass/fail verdict each.

What is still out of scope for R2007: password-protected files (a section whose descriptor declares encryption is refused rather than decoded) and writing (the container is implemented read-only — a writer would need a Reed-Solomon encoder and a §5.10 compressor).

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
