# Community post: Open Design Alliance forum

**Title:** dwg-rs — an Apache-2.0 Rust DWG reader built on the Open Design Specification v5.4.1

**Audience:** Open Design Alliance community forum / mailing list.

**Status:** Draft. Tone is respectful and complementary, not competitive with ODA's commercial SDK.

---

Hello ODA community,

I want to introduce a new open-source project and, more importantly, say thank you. The ODA's freely-redistributable *Open Design Specification for .dwg files* (currently v5.4.1) is the single most useful public reference on DWG, and it is the foundation this project is built on. Publishing that specification the way the ODA does is a substantial contribution to the ecosystem.

The project is called `dwg-rs`. It is an Apache-2.0 Rust reader for the DWG container format, targeting R13 through AC1032. It is pre-alpha — the container layer (file identification, LZ77 decompression, Section Page Map, Sec_Mask layer-1, CRC, Reed-Solomon, metadata parsers, handle and class maps, raw object stream walker) has landed and been exercised against a 19-file public corpus; per-entity field decoders are alpha, with real-file decode rates currently in the 22 to 86 percent range depending on version. The README states this honestly.

I want to be explicit about provenance: executable code from the ODA Drawings SDK / Teigha was not consulted or imported, no decompiled binaries or internal ODA documentation was used. Only the public specification PDF, publicly-redistributable sample files, academic papers, and independent byte-level inspection. One scoped exception is documented in the repository: algorithm-description comments (not executable code) in the MIT-licensed ACadSharp were consulted to resolve one LZ77 offset-encoding ambiguity. The full source-provenance policy is in `CLEANROOM.md`.

`dwg-rs` is not a substitute for the ODA SDK. It is a complement, aimed specifically at the Apache-2-only segment — projects that cannot take the SDK license but still want a permissively-licensed path toward interoperability for the subset of DWG features already covered. Feedback from ODA members is welcome.

Repository: https://github.com/DrunkOnJava/dwg-rs

Thank you again for the specification work.

Griffin Long

This is pre-alpha software. See [capability table](https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance) for measured decode rates.
