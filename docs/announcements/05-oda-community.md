# Community post: Open Design Alliance forum

**Title:** dwg-rs — a clean-room Apache-2.0 Rust DWG reader built on the Open Design Specification v5.4.1

**Audience:** Open Design Alliance community forum / mailing list.

**Status:** Draft. Tone is respectful and complementary, not competitive with ODA's commercial SDK.

---

Hello ODA community,

I want to introduce a new open-source project and, more importantly, say thank you. For roughly 28 years the DWG format has been closed territory, and the single most useful public reference on it has been the ODA's freely-redistributable *Open Design Specification for .dwg files* (currently v5.4.1). That PDF is the foundation this project is built on. Without it, there is no credible clean-room path into DWG for anyone outside the Autodesk or ODA membership umbrella, and publishing it the way ODA does is a real contribution to the ecosystem.

The project is called `dwg-rs`. It is an Apache-2.0, clean-room Rust reader for the DWG container format, targeting R13 through AC1032. It is pre-alpha — the container layer (file identification, LZ77 decompression, Section Page Map, Sec_Mask layer-1, CRC, Reed-Solomon, metadata parsers, handle and class maps, raw object stream walker) has landed and been exercised against a 19-file public corpus; per-entity field decoders are alpha, with real-file decode rates currently in the 22 to 86 percent range depending on version. The README states this honestly.

I want to be explicit about what `dwg-rs` does **not** use: no ODA Drawings SDK / Teigha source, no decompiled binaries, no internal ODA documentation. Only the public specification PDF, publicly-redistributable sample files, academic papers, and independent byte-level inspection. The full sourcing policy is in `CLEANROOM.md` in the repository.

`dwg-rs` is not a substitute for the ODA SDK. It is a complement, aimed specifically at the Apache-2-only segment — projects that cannot take the SDK license but still want a permissively-licensed path toward interoperability for the subset of DWG features already covered. Feedback from ODA members is welcome.

Repository: https://github.com/DrunkOnJava/dwg-rs

Thank you again for the specification work.

Griffin Long

This is pre-alpha software. See [capability table](https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance) for measured decode rates.
