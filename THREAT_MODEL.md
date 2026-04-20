# Threat model

`SECURITY.md` documents **how** to report security issues. This
document documents **what** `dwg-rs` is defending against and how,
so that reporters, auditors, and prospective commercial adopters
can evaluate the posture in detail.

This is a living document. Every bounded data structure enumerated
below corresponds to a `const MAX_*` in source; when new bounds are
added, they should be reflected here in the same commit.

## Attack surface

`dwg-rs` is a parser. The only trust boundary the crate crosses is
the byte stream handed to `DwgFile::open` or `DwgFile::from_bytes`.
Every byte of that input is **untrusted** — it could come from a
download, a user upload, an attachment, or a maliciously crafted
fixture.

No network I/O. No shell-out. No filesystem writes by default (the
library itself opens files for reading only; the CLI binaries under
`src/bin/` may write to stdout / to user-specified paths but do not
write to arbitrary locations).

## Threat classes

### 1. Memory safety

**Mitigation:** the entire library is safe Rust —
`#![deny(unsafe_code)]` is set at the crate root in `src/lib.rs`.
No `unsafe` blocks, no raw-pointer dereferencing, no transmutes.
All integer arithmetic that could realistically overflow uses
either explicit `checked_*` / `saturating_*` / `wrapping_*` or
`i64::unsigned_abs`-style idioms documented inline.

Out of scope: memory-safety issues in `std`, in transitive
dependencies (audited via `cargo-deny`), or in compiler codegen.
These are reported upstream, not here.

### 2. Panic-on-malformed-input

**Mitigation:** parsing functions return `Result<T, Error>`. The
only remaining panics are:

- `debug_assert!` assertions that fire only in debug builds.
- `BitWriter::write_3b(v)` with an invalid `v` — this is a
  writer-side programmer-error path, not input-driven; callers use
  `try_write_3b` when `v` is not statically validated.
- Intentional `unreachable!` branches in matches where the bit
  pattern is exhaustively covered (e.g., a 2-bit field where all
  four patterns are handled).

Reports of panics from input under `DwgFile::open` / `from_bytes`
are security issues — please file via Security Advisories.

### 3. Denial of service via unbounded allocation

**Mitigation:** every parser that reads a caller-supplied count or
length has a cap. The current registry:

| Location | Cap | Rationale |
|----------|-----|-----------|
| `lz77::decompress` output | `DecompressLimits::max_output_bytes` (default 256 MiB) | Decompression-bomb defense |
| `lz77::decompress` back-reference length | `DecompressLimits::max_backref_len` (default 1 MiB) | Pathological copy length |
| `lz77::decompress` initial capacity | `min(expected_size, max_output_bytes)` | Reject huge up-front allocation from lying `expected_size` |
| `handle_map::HandleMap::parse` | `MAX_HANDLE_ENTRIES = 1_000_000` | Handle table fan-out |
| `common_entity::parse_xdata` | `MAX_XDATA_ITERATIONS = 256` | Bounded XDATA loop |
| `entities::dispatch::DispatchSummary::errors` | `MAX_RETAINED_ERRORS = 1_000` | Don't accumulate unbounded error strings from a file full of broken objects |
| `classes::ClassMap::parse` | `MAX_CLASS_ENTRIES = 4_096` | Class-map entry count |
| `entities::lwpolyline::decode` points | `10_000_000` | Current (too-generous, tracked as work) |
| `entities::image::decode`, `leader::decode`, `spline::decode` | `1_000_000` | Current (too-generous, tracked as work) |

Planned tightening in upcoming releases:

- Replace the scattered constants with a single configurable
  `ParseLimits` struct.
- Derive upper bounds from `cursor.remaining_bits()` before
  allocating — a count that could not fit in the remaining payload
  is rejected without trying to read it.
- Lower entity-specific caps to realistic defaults (e.g., 1M
  points for LWPOLYLINE, 100K for less-common collections).

### 4. Decompression bombs

**Mitigation:** covered by class (3) above. The `DecompressLimits`
struct is exposed publicly so callers can pick a profile that fits
their context:

- **`Default`** — 256 MiB output / 1 MiB back-reference. Fits real
  DWG section sizes.
- **`permissive()`** — 4 GiB output / 64 MiB back-reference. For
  test harnesses that need to pass large synthetic fixtures.
- **Custom** — any caller-specified values.

The plain `lz77::decompress` function uses the default profile;
callers wanting tighter limits use `decompress_with_limits`.

### 5. Integer overflow / under-flow

**Mitigation:** `BitWriter::write_mc` uses `i64::unsigned_abs` to
handle `i64::MIN` without the two's-complement negation overflow
that previously shipped. Other paths use `.saturating_add` and
`.checked_sub` explicitly where an attacker-controlled value is
combined with internal state. The `BitWriter::position_bits` method
and the `ObjectWalker` offset arithmetic both use explicit branches
rather than `-1`-style arithmetic.

Rust's default overflow-check semantics (panic in debug, wrap in
release) apply to the rest. A release-mode wrap that produces a
semantically wrong decoded value is a correctness bug, not a
security issue, unless it leads to unbounded allocation or an
out-of-bounds read.

### 6. Silent mis-decode

`dwg-rs` draws a line between **incorrect geometry extracted from
a valid file** (a correctness bug) and **undefined behavior from an
invalid file** (a security issue). Reports in the former category
are tracked as work items; reports in the latter category are
security issues.

Specifically: an `LZ77OutputLimitExceeded` error, an
`Lz77BackrefTooLong` error, an `Unsupported` error, or any other
typed `Error` variant is NOT a security issue — the parser
correctly refused to accept the input.

## Fuzz coverage

Under `fuzz/` there are scaffolded `cargo-fuzz` targets for:

- `lz77_decompress` — the decompression-bomb surface
- `bitcursor_primitives` — every bit-level primitive in
  `BitCursor`
- `dwg_file_open` — whole-file parse path, from magic through
  container + section map
- `section_map` — the R2004+ section map layer specifically
- `object_walker` — the handle-driven object stream walker

Fuzz runs have not yet been logged for an extended duration. The
`0.5.0` milestone (see `ROADMAP.md`) requires at least 1 hour of
stable run time per target before declaring hardened. Contributors
running fuzz sessions are encouraged to commit corpus seeds back
into `fuzz/corpus/<target>/`.

## Out of scope

The following are **not** considered security issues by this
project:

- **Cryptographic confidentiality** — DWG's R2004+ XOR magic
  sequence and R2007 Sec_Mask are obfuscation, not encryption.
  `dwg-rs` makes no claim of confidentiality and does not
  implement any confidentiality primitive.
- **Timing side channels** in the LZ77 decoder or the object-
  stream walker. This is a parser for a public file format; the
  content being decoded is not secret.
- **Memory-safety bugs in transitive dependencies** — audited via
  `cargo-deny` (see `deny.toml`) but reported upstream.
- **Incorrect geometry decoding from a valid file** — tracked as
  a correctness bug, not a vulnerability.

## How to report

See `SECURITY.md`. Use GitHub Security Advisories at
<https://github.com/DrunkOnJava/dwg-rs/security/advisories/new>.
