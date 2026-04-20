# dwg-wasm sample fixtures

Tiny synthetic DWG files bundled with `dwg-wasm` so a browser viewer
can load a real file without the user having to supply one. Served by
the V-20 `DwgFile.openSample(name)` JS entry point (see
`wasm/src/sample_loader.rs`).

## Provenance

Generated at build time by `tools/synthetic-dwg` (see
`tools/synthetic-dwg/src/main.rs`). That binary emits a minimal valid
R14 (AC1014) file — magic + common prefix + five 9-byte locator
records + a fake LINE entity payload. The file round-trips through
`dwg::DwgFile::open` but is NOT guaranteed to open in AutoCAD; these
are test fixtures, not AutoCAD-authored drawings.

To regenerate:

```bash
cd tools/synthetic-dwg
cargo build --release
./target/release/synthetic-dwg ../../wasm/examples/samples/line_r14.dwg --read-back
```

## Files

| Name | Version | Size | Entity content |
|------|---------|------|----------------|
| `line_r14.dwg` | R14 (AC1014) | 107 B | Single LINE (0,0)→(100,100) |
| `empty_r14.dwg` | R14 (AC1014) | 107 B | Empty drawing |
| `square_r14.dwg` | R14 (AC1014) | 107 B | Four-LINE square outline |
| `triangle_r14.dwg` | R14 (AC1014) | 107 B | Three-LINE triangle |

NOTE: the current `synthetic-dwg` generator emits the same skeleton
file for each invocation — the name-to-content mapping above is the
**intended** future shape once the generator grows per-shape entity
output. All four files are byte-identical today. The four names still
exercise the sample-loader's `fetch` path + URL routing, which is the
purpose of V-20.

## License

These fixtures are generated output — not copied from Autodesk or any
other vendor's drawings. They ship under the same Apache-2.0 license
as the rest of `dwg-rs`.
