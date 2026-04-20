# Benchmarks

> **Status: scaffold.** No numbers are published here yet. The
> Criterion harness lives at
> [`benches/lz77.rs`](../../benches/lz77.rs) but only covers one
> pipeline stage. A full multi-stage bench suite lands in the
> `0.2.0` → `0.4.0` window — the placeholder table below fills in as
> each operation's bench ships and a matched LibreDWG baseline run is
> captured under the same conditions.

## What gets measured

Four operations, measured per-file and summarized per size bucket.

| Operation               | Definition                                                                                                                                      |
|-------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------|
| `open`                  | Time from `DwgFile::open(path)` returning to the caller — file-open header parse, section map parse, section info parse. No payload decompress. |
| `decompress`            | Time to decompress every section payload (LZ77 + Sec_Mask layer-1). Dominated by `AcDb:AcDbObjects` on most files.                              |
| `decode_all_objects`    | Time for `all_objects()` to return every `RawObject`. Object-stream walker only; no per-entity decode.                                          |
| `svg_export`            | Time for the full `DwgFile → SVG` pipeline. Activates when L9 SVG export ships.                                                                 |

Each op is measured on cold and warm FS cache paths; the table
reports warm-cache. Cold-cache numbers live in the raw Criterion
output in `target/criterion/`.

## Methodology

- **Harness.** [Criterion.rs](https://github.com/bheisler/criterion.rs)
  under `benches/*.rs`, invoked via `cargo bench`.
- **Machine class.** Numbers are reported with the host CPU model,
  total RAM, and kernel version captured by a small shim in the
  harness — no single-number "it's X ms" claim without hardware
  context.
- **Warmup.** Criterion default (3 s warmup, 5 s measurement
  window, 100 samples).
- **Input selection.** Five size buckets chosen from the measured
  corpus plus a synthetic large file. A bucket is used in a run
  only if ≥ 3 files fall inside it — otherwise that row reports
  "insufficient sample" rather than a single-point number.
- **No network.** Benchmarks are fully local; no cloud runners, no
  hosted CI timings used for the public table (hosted CI is fine
  for regression gates, not for published latency numbers).

## Baseline comparison posture

The planned baseline is [LibreDWG](https://www.gnu.org/software/libredwg/)
— the only other open-source DWG reader with broad version
coverage. LibreDWG is GPL-3 licensed and **was not consulted at any
point** during the build of `dwg-rs`; the comparison is strictly
external-behavior benchmarking (same input files, same operations),
not an implementation cross-reference. The clean-room posture
documented in [`CLEANROOM.md`](../../CLEANROOM.md) is unchanged by
the benchmark track.

We are not benchmarking against the ODA SDK. Its license terms make
publishing head-to-head numbers legally awkward, and the project
posture is deliberately that ODA SDK source is never consulted or
invoked in this repository's CI.

## Placeholder table

Numbers marked `TBD` fill in as the corresponding bench ships. Ratio
is `LibreDWG / dwg-rs` — higher is better for `dwg-rs`.

### `open`

| Input size bucket | dwg-rs (ms) | LibreDWG (ms) | Ratio |
|-------------------|-------------|---------------|-------|
| tiny   (≤  10 KB) | TBD         | TBD           | TBD   |
| small  (≤ 100 KB) | TBD         | TBD           | TBD   |
| medium (≤   1 MB) | TBD         | TBD           | TBD   |
| large  (≤  10 MB) | TBD         | TBD           | TBD   |
| huge   (≥  50 MB) | TBD         | TBD           | TBD   |

### `decompress`

| Input size bucket | dwg-rs (ms) | LibreDWG (ms) | Ratio |
|-------------------|-------------|---------------|-------|
| tiny   (≤  10 KB) | TBD         | TBD           | TBD   |
| small  (≤ 100 KB) | TBD         | TBD           | TBD   |
| medium (≤   1 MB) | TBD         | TBD           | TBD   |
| large  (≤  10 MB) | TBD         | TBD           | TBD   |
| huge   (≥  50 MB) | TBD         | TBD           | TBD   |

### `decode_all_objects`

| Input size bucket | dwg-rs (ms) | LibreDWG (ms) | Ratio |
|-------------------|-------------|---------------|-------|
| tiny   (≤  10 KB) | TBD         | TBD           | TBD   |
| small  (≤ 100 KB) | TBD         | TBD           | TBD   |
| medium (≤   1 MB) | TBD         | TBD           | TBD   |
| large  (≤  10 MB) | TBD         | TBD           | TBD   |
| huge   (≥  50 MB) | TBD         | TBD           | TBD   |

### `svg_export`

| Input size bucket | dwg-rs (ms) | LibreDWG (ms) | Ratio |
|-------------------|-------------|---------------|-------|
| tiny   (≤  10 KB) | TBD         | TBD           | TBD   |
| small  (≤ 100 KB) | TBD         | TBD           | TBD   |
| medium (≤   1 MB) | TBD         | TBD           | TBD   |
| large  (≤  10 MB) | TBD         | TBD           | TBD   |
| huge   (≥  50 MB) | TBD         | TBD           | TBD   |

## Running the benches yourself

The harness that exists today:

```bash
cargo bench --bench lz77
```

As additional benches ship, they appear under `benches/` with the
operation name. Criterion emits HTML reports to
`target/criterion/report/index.html`.

## What "TBD" really means

A row fills in only when all three preconditions hold:

1. The `dwg-rs` side of the operation is shipped and correct on the
   full size bucket (not just the tiny files).
2. A matched LibreDWG run on the exact same input files is captured
   on the exact same machine in the same benchmark window.
3. At least three files in the bucket contribute data points.

If any precondition fails the cell stays `TBD`. Publishing a
half-measured number would be worse than publishing none.
