# LinkedIn post draft (L-09)

**Audience:** AEC / CAD / BIM professionals, product engineering leads at commercial CAD-adjacent tools

---

**Body:**

Announcing dwg-rs, a permissively-licensed (Apache-2.0), early-preview Rust reader for AutoCAD DWG files — R13 through R2018 (AC1032).

The open-source DWG ecosystem has two strong options: LibreDWG (GPL-3, the most complete reader today) and ACadSharp (MIT, C#, mature .NET). For Rust stacks that can't take GPL-3 and don't have a C# runtime, those options don't fit. dwg-rs fills that specific niche — an Apache-2.0 Rust crate for DWG container reading.

It is built from the ODA's freely-redistributable Open Design Specification v5.4.1 and first-party byte inspection of public sample files. Executable code from the Autodesk SDK, the ODA SDK, and GPL-licensed readers was not imported; the source-provenance policy is documented in CLEANROOM.md and every contribution carries a declaration.

What this means for downstream tools: a DWG container-layer reader you can evaluate for a closed-source product, a SaaS backend, or a permissively-licensed OSS pipeline — provided the current partial coverage fits your use case.

Honest status: 0.1.0-alpha.1, pre-alpha. The container layer (file identification, section map, LZ77 decompression, CRC + Reed-Solomon, metadata, handle map, class map, object-stream walker) has landed and carries unit + corpus coverage. Per-entity field decoders are alpha — the README publishes measured decode rates per DWG version (22 % on R2018, 86 % on R2013, 25 % aggregate across the `nextgis/dwg_samples` corpus + one AC1032 sample). Entity-decoder correctness is the 0.2.0 milestone.

Sibling project: rvt-rs — same source-provenance policy, same license, for Autodesk Revit (.rvt / .rfa).

Repo: https://github.com/DrunkOnJava/dwg-rs

This is pre-alpha software. See [capability table](https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance) for measured decode rates.
