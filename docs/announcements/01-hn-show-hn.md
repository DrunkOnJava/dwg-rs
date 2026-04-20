# Show HN draft (L-07)

**Title:** Show HN: dwg-rs – Apache-2 Rust reader for AutoCAD DWG (R13–R2018), pre-alpha

---

**Body:**

dwg-rs is an Apache-2.0 Rust reader for Autodesk DWG files. It is **pre-alpha**: the container layer (file identification, section map, LZ77, Sec_Mask layer-1, CRCs, Reed-Solomon, metadata, handle map, class map, object-stream walker) has landed and has unit + corpus coverage; per-entity field decoders are alpha — measured aggregate decode rate is 25 % on the public `nextgis/dwg_samples` corpus + one AC1032 sample, with R2013 around 86 % and R2018 at 22 %. The README publishes the per-version table so you can decide whether the current coverage fits your use case.

Why another DWG reader: LibreDWG (GPL-3) is more complete today and ACadSharp (MIT, C#) is a mature .NET option, but nothing Apache-2.0 exists in Rust. dwg-rs fills that niche. Built from the ODA's public Open Design Specification v5.4.1 and first-party byte inspection of public sample files. CLEANROOM.md documents the source-provenance policy (executable code from Autodesk SDK / ODA SDK / GPL-licensed readers was not imported; one scoped exception is documented for algorithm-description comments in the MIT-licensed ACadSharp).

Partial coverage — see the capability matrix for what ships today: https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance. Entity-decoder correctness is the 0.2.0 milestone.

Feedback welcome — especially on the bit-cursor abstraction and the R14 / R2000 object-stream layout gap.

Repo: https://github.com/DrunkOnJava/dwg-rs

This is pre-alpha software. See [capability table](https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance) for measured decode rates.
