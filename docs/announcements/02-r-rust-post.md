# r/rust post draft (L-08)

**Suggested title:** dwg-rs — Apache-2.0 Rust reader for AutoCAD DWG (R13–R2018), pre-alpha, clean-room, zero unsafe

---

**Body:**

Just published dwg-rs, a clean-room Rust foundation for Autodesk DWG files. Honest upfront: 0.1.0-alpha.1, container layer ships, per-entity decoders are alpha. Aggregate decode rate is 25 % across the public `nextgis/dwg_samples` corpus; per-version breakdown is in the README.

Why this exists: Autodesk never published a DWG spec. The ODA SDK is paid-membership. LibreDWG is GPL-3. dwg-rs is built only from the ODA's freely-redistributable Open Design Specification v5.4.1 and first-party byte inspection of public sample files. `CLEANROOM.md` documents the allowed / forbidden source list and every PR ships with a clean-room declaration.

Technical posture:

- `#![forbid(unsafe_code)]` crate-wide. Reed-Solomon, LZ77, GF(256), bit-cursor, bit-writer — all safe Rust.
- Edition 2024, MSRV 1.85.
- CLI dependencies (`clap`, `anyhow`, `serde`) feature-gated behind `cli`. Pure library consumers pull only `thiserror` + `byteorder`.
- Fuzz harness under `fuzz/` (cargo-fuzz; corpora are still stabilizing).
- Criterion bench at `benches/lz77.rs`.
- 193 tests today: 156 unit + 5 corpus + 9 proptest + 22 sample + 1 doctest, zero failures. clippy `-D warnings` clean. `cargo deny` clean.
- Defensive caps — 256 MiB LZ77 output, 1 M dictionary entries, 16 MB XRECORDs, 1 M spline control points. Threat model in `THREAT_MODEL.md`.

What works: version identification, section map, LZ77 decompression (with the spec-errata offset reading called out), Sec_Mask layer-1 across every R2004-family version, CRC-8/32, Reed-Solomon(255,239) decoder, metadata parsers, handle map, class map, object-stream walker. 27 per-entity decoders exist but real-file coverage is the 0.2.0 milestone.

Would love feedback on the bit-cursor abstraction in `src/bitcursor.rs` — it's load-bearing for every decoder and I'd like a second set of eyes on the MC / MS / BLL / 3BD primitives before I call the API stable.

Repo: https://github.com/DrunkOnJava/dwg-rs
