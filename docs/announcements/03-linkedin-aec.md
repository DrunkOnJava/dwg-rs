# LinkedIn post draft (L-09)

**Audience:** AEC / CAD / BIM professionals, product engineering leads at commercial CAD-adjacent tools

---

**Body:**

Announcing dwg-rs, a permissively-licensed (Apache-2.0), clean-room Rust foundation for reading AutoCAD DWG files — R13 through R2018 (AC1032).

For almost three decades the DWG format has been a moat. Autodesk never published a specification. The Open Design Alliance publishes a spec but its SDK requires paid membership. The one mature open-source reader, LibreDWG, is GPL-3 — which quietly disqualifies it from commercial codebases that can't take on copyleft contamination.

dwg-rs is built only from the ODA's freely-redistributable Open Design Specification v5.4.1 and first-party byte inspection of public sample files. No Autodesk SDK. No ODA SDK. No LibreDWG source consulted at any point. The clean-room policy is documented in CLEANROOM.md and every contribution carries a signed declaration.

What this means for downstream tools: a DWG container-layer reader you can ship in a closed-source product, a SaaS backend, or a permissively-licensed OSS pipeline without rewriting your license terms or budgeting for SDK membership.

Honest status: 0.1.0-alpha.1. The container layer (file identification, section map, LZ77 decompression, CRC + Reed-Solomon, metadata, handle map, class map, object-stream walker) is shipping and covered by 193 tests. Per-entity field decoders are alpha — the README publishes measured decode rates per DWG version so you can evaluate honestly whether it's ready for your use case today.

Sibling project: rvt-rs — same clean-room posture, same license, for Autodesk Revit (.rvt / .rfa).

Repo: https://github.com/DrunkOnJava/dwg-rs
