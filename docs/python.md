# Python bindings — current state

**Status:** Not yet released. Target version **0.2.0**.

The Python bindings are a placeholder today. The entire surface lives
in [`src/python_stubs.rs`](../src/python_stubs.rs) and compiles to
**41 lines**: a single `diagnostics()` stub that returns the string
`"{}"` so the API-parity tracker can reference a symbol that resolves.
There is no PyO3 crate, no published wheel, no `pip install dwg`, and
no way to call dwg-rs from Python today.

If you need DWG access from Python right now, shell out to the Rust
CLI (`dwg-info`, `dwg-dump`) and parse its output. The CLIs are
documented in the repository's [`README.md`](../README.md#cli-tools).

## Planned API (0.2.0 target)

The 0.2.0 bindings will land in a sibling crate named `dwg-py` that
wraps the Rust surface one-to-one via PyO3. Expected shape:

```python
import dwg

# Open a file. Accepts a filesystem path.
f = dwg.DwgFile.open("drawing.dwg")

# Basic metadata.
print(f.version)                 # "AC1032"
print(f.section_map_status)      # "Full" | "Fallback" | "Deferred"

# Section list — analogous to Rust's DwgFile::sections().
for s in f.sections():
    print(s.name, s.kind, s.offset, s.size, s.compressed, s.encrypted)

# Decode diagnostics (after reading a section).
buf = f.read_section("AcDb:Header")
diag = f.sections()[0].diagnostics()
print(diag.decompressed_bytes, diag.compression_ratio)

# Full object walk — returns a list of RawObject dicts.
for obj in f.all_objects():
    print(hex(obj["type_code"]), obj["kind"], hex(obj["handle"]))
```

Failure modes will raise a single `dwg.DwgError` with a short code
(`"not_dwg"`, `"unsupported_version"`, `"truncated"`, `"crc_mismatch"`,
`"lz77_truncated"`, `"walker_limit_exceeded"`, ...) plus a message
string matching the Rust `Error::{variant}` Display output.

## Safety knobs

The [`OpenLimits`](../src/limits.rs) profile will be exposed at
`DwgFile.open()` via three kwargs, matching the SEC-10 target:

| Kwarg | Rust field | Default (safe) | Purpose |
|-------|------------|---------------:|---------|
| `max_file_bytes` | `OpenLimits::max_file_bytes` | 1 GiB | Refuse oversize input before buffering bytes. |
| `max_section_bytes` | `OpenLimits::max_section_bytes` | 256 MiB | Per-section cap applied after LZ77 decompression. |
| `max_output_bytes` | `DecompressLimits::max_output_bytes` | 256 MiB | LZ77 decompression-bomb ceiling. |

Example:

```python
# Tighter profile for an upload pipeline:
f = dwg.DwgFile.open(
    path,
    max_file_bytes=100 * 1024 * 1024,   # 100 MiB
    max_section_bytes=16 * 1024 * 1024, # 16 MiB
    max_output_bytes=16 * 1024 * 1024,  # 16 MiB
)
```

Presets (`"safe"`, `"paranoid"`, `"permissive"`) from
[`OpenLimits`](../src/limits.rs) will also be exposed as a single
keyword:

```python
f = dwg.DwgFile.open(path, limits="paranoid")
```

## Tracking

- The shape of the bindings is frozen at the Rust API level once a
  symbol has a doctest in [`CONTRIBUTING.md`](../CONTRIBUTING.md).
- The actual PyO3 wiring, wheel building, and CI/release workflow are
  tracked as a **0.2.0 milestone** in
  [`ROADMAP.md`](../ROADMAP.md).

## Why is this a placeholder?

The container layer is pre-alpha and the per-entity decoders hover in
the 22–86 % real-file coverage band (see the
[capability matrix](../README.md#capability-matrix-at-a-glance) for
the measured numbers). Shipping Python bindings before the Rust
surface stabilizes would mean the bindings churn every time a decoder
lands or an error variant changes. The responsible thing is to keep
the surface honest in Rust first and expose it to Python once 0.2.0
ships.

If you want to help make the bindings real, open an issue tagged
`python-bindings` before the 0.2.0 branch cuts — we are looking for
PyO3 experience for the wheel-packaging and error-translation layer.

---

This document reflects the code in `src/python_stubs.rs` as of
0.1.0-alpha.1. When the bindings ship it will be replaced with
binding-level documentation (module reference, classes, exceptions,
changelog).
