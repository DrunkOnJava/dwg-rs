# dwg-wasm

WebAssembly bindings for [`dwg-rs`](https://github.com/DrunkOnJava/dwg-rs).

Pre-alpha. First cut is the build-pipeline scaffolding (V-01) plus a
minimal JS-visible `DwgFile` API (V-02). The full browser viewer
(pan/zoom, entity rendering, selection, export buttons) is tracked
as V-03 through V-24 on the public roadmap.

## Build

```bash
cd wasm
wasm-pack build --target web --release
# output: pkg/dwg_wasm_bg.wasm + pkg/dwg_wasm.js
```

Alternative targets: `--target bundler` (webpack/rollup), `--target
nodejs` (Node.js + fs), `--target no-modules` (classic script tag).

## JS API

```javascript
import init, { DwgFile, crateVersion } from './pkg/dwg_wasm.js';

await init();

console.log('dwg-rs', crateVersion());

// Load a user-uploaded file.
const input = document.querySelector('input[type=file]');
input.addEventListener('change', async () => {
  const file = input.files[0];
  const bytes = new Uint8Array(await file.arrayBuffer());

  try {
    const f = DwgFile.open(bytes);
    console.log('version:', f.versionName(), '(', f.versionMagic(), ')');
    console.log('section-map:', f.sectionMapStatus());
    for (const s of f.sections()) {
      console.log(`  ${s.name}  ${s.size} bytes @ 0x${s.offset.toString(16)}`);
    }
  } catch (e) {
    console.error('DWG parse failed:', e);
  }
});
```

## Scope and non-goals

Shipped in V-01/V-02:

- File open from `Uint8Array`
- Version detection
- Section-list enumeration
- Section-map status (Full / Fallback / Deferred)

NOT yet in this release:

- Entity decoding exposed to JS (V-02a follow-up)
- Layer / block iteration
- SVG / glTF / DXF export via the browser
- pan / zoom / selection (V-05..V-23)
- Progressive streaming for large drawings (V-17)

The crate parent's [compatibility
matrix](../docs/landing/compatibility.md) applies here unchanged —
every version the reader supports, the wasm bindings support. Every
version that errors out native also errors out in-browser.

## Safety posture

Shares the parent crate's `#![forbid(unsafe_code)]` policy. All
panics abort (wasm is compiled with `panic = "abort"` to shrink the
.wasm payload); the parent crate returns `Result<T, Error>` on every
malformed input, so unreachable-panic paths don't fire on legitimate
input.
