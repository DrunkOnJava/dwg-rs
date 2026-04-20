# r/rust post draft (L-08)

**Suggested title:** dwg-rs — Apache-2.0 Rust reader for AutoCAD DWG (R13–R2018), pre-alpha, zero unsafe

---

**Body:**

Just published dwg-rs, an Apache-2.0 Rust reader for Autodesk DWG files. Honest upfront: 0.1.0-alpha.1. Container layer has landed; per-entity decoders are alpha. Aggregate decode rate across the public `nextgis/dwg_samples` corpus + one AC1032 sample is 25 %, ranging from 22 % on R2018 to 86 % on R2013; the full per-version breakdown is in the README.

Why this exists: mature open-source DWG readers do exist — LibreDWG (GPL-3) and ACadSharp (MIT, C#) — but nothing Apache-2.0 in Rust. dwg-rs fills that niche. It is built from the ODA's freely-redistributable Open Design Specification v5.4.1 and first-party byte inspection of public sample files. `CLEANROOM.md` documents the source-provenance policy; every PR ships with a declaration.

Technical posture:

- `#![forbid(unsafe_code)]` crate-wide. Reed-Solomon, LZ77, GF(256), bit-cursor, bit-writer — safe Rust throughout.
- Edition 2024, MSRV 1.85.
- CLI dependencies (`clap`, `anyhow`, `serde`) feature-gated behind `cli`. Pure library consumers pull only `thiserror` + `byteorder`.
- Fuzz harness under `fuzz/` (cargo-fuzz; corpora are still stabilizing).
- Criterion bench at `benches/lz77.rs`.
- Test split: 156 unit + 5 corpus + 9 proptest + 22 sample + 1 doctest. clippy `-D warnings` clean. `cargo deny` clean.
- Defensive caps — 256 MiB LZ77 output, 1 M dictionary entries, 16 MB XRECORDs, 1 M spline control points. Threat model in `THREAT_MODEL.md`.

What currently parses: version identification, section map, LZ77 decompression (with the spec-errata offset reading called out), Sec_Mask layer-1 across every R2004-family version, CRC-8/32, Reed-Solomon(255,239) decoder, metadata parsers, handle map, class map, object-stream walker. Per-entity decoders exist for 27 types but real-file coverage is the 0.2.0 milestone.

Would love feedback on the bit-cursor abstraction in `src/bitcursor.rs` — it is load-bearing for every decoder and I would like a second set of eyes on the MC / MS / BLL / 3BD primitives before I call the API stable.

Repo: https://github.com/DrunkOnJava/dwg-rs

This is pre-alpha software. See [capability table](https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance) for measured decode rates.
