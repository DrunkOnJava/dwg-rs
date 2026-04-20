# LinkedIn post draft (L-09)

**Audience:** AEC / CAD / BIM professionals, product engineering leads at commercial CAD-adjacent tools

---

**Body:**

Announcing dwg-rs, a permissively-licensed (Apache-2.0), early-preview Rust foundation for reading AutoCAD DWG files — R13 through R2018 (AC1032).

For almost three decades the DWG format has been closed territory. Autodesk never published a specification. The Open Design Alliance publishes a spec but its SDK requires paid membership. The long-standing open-source reader, LibreDWG, is GPL-3 — which disqualifies it from commercial codebases that cannot absorb copyleft.

dwg-rs is built only from the ODA's freely-redistributable Open Design Specification v5.4.1 and first-party byte inspection of public sample files. No Autodesk SDK. No ODA SDK. No LibreDWG source consulted at any point. The clean-room policy is documented in CLEANROOM.md and every contribution carries a signed declaration.

What this means for downstream tools: a DWG container-layer reader you can evaluate for a closed-source product, a SaaS backend, or a permissively-licensed OSS pipeline — provided the current partial coverage fits your use case — without taking on a copyleft license or SDK membership.

Honest status: 0.1.0-alpha.1, pre-alpha. The container layer (file identification, section map, LZ77 decompression, CRC + Reed-Solomon, metadata, handle map, class map, object-stream walker) has landed and carries unit + corpus coverage. Per-entity field decoders are alpha — the README publishes measured decode rates per DWG version (22 % on R2018, 86 % on R2013, 25 % aggregate across the `nextgis/dwg_samples` corpus + one AC1032 sample) so you can evaluate whether today's coverage fits your use case. Entity-decoder correctness is the 0.2.0 milestone.

Sibling project: rvt-rs — same clean-room posture, same license, for Autodesk Revit (.rvt / .rfa).

Repo: https://github.com/DrunkOnJava/dwg-rs

This is pre-alpha software. See [capability table](https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance) for measured decode rates.
