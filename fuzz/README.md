# dwg-rs fuzz targets

Five `cargo-fuzz` targets exercising every byte-level attack surface in
this crate:

| Target | What it fuzzes |
|--------|----------------|
| `lz77_decompress` | `lz77::decompress` against arbitrary compressed streams |
| `bitcursor_primitives` | Every `BitCursor::read_*` method against arbitrary bytes |
| `dwg_file_open` | Top-level `DwgFile::from_bytes` against arbitrary input |
| `section_map` | `handle_map::HandleMap::parse` against malformed sections |
| `object_walker` | `ObjectWalker::next` against arbitrary payloads, all versions |

## Install cargo-fuzz

```bash
cargo install cargo-fuzz
# Note: requires a nightly toolchain because cargo-fuzz uses sanitizer flags.
rustup install nightly
```

## Run a target

```bash
cd fuzz
cargo +nightly fuzz run lz77_decompress -- -max_total_time=600
cargo +nightly fuzz run bitcursor_primitives -- -max_total_time=600
cargo +nightly fuzz run dwg_file_open -- -max_total_time=600
cargo +nightly fuzz run section_map -- -max_total_time=600
cargo +nightly fuzz run object_walker -- -max_total_time=600
```

`-max_total_time=600` bounds each run at 10 minutes. For an overnight
sweep, drop the bound and leave it running.

## Reproducing a crash

Each crash is saved to `fuzz/artifacts/<target>/crash-<hash>`. To reproduce:

```bash
cargo +nightly fuzz run <target> fuzz/artifacts/<target>/crash-<hash>
```

## Invariant

**No input should ever panic.** Every failure path must return an
`Err(crate::error::Error)` variant. Fuzzing that discovers a panic is a
security-relevant bug and should be filed via the private security
advisory flow (see `SECURITY.md` in the repo root).

## Status

These targets are scaffolded and compile (see `Cargo.toml`), but have not
been run for extended sessions yet. A first overnight sweep is on the
0.1.0-beta.1 milestone list.
