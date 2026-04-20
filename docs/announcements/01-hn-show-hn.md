# Show HN draft (L-07)

**Title:** Show HN: dwg-rs – Apache-2 Rust reader for AutoCAD DWG (R13–R2018)

---

**Body:**

dwg-rs is a clean-room, Apache-2.0 Rust foundation for Autodesk DWG files. It is pre-alpha: the container layer (file identification, section map, LZ77, Sec_Mask layer-1, CRCs, Reed-Solomon, metadata, handle map, class map, object-stream walker) is shipping and covered by 193 tests, but per-entity field decoders are alpha — measured aggregate decode rate is 25 % on the corpus, with R2013 topping out around 86 % and R2018 at 22 %. The README publishes those numbers with a per-version breakdown.

Why another DWG reader: Autodesk never published a spec, the ODA's SDK requires paid membership, and LibreDWG is GPL-3. dwg-rs exists so Rust projects can read DWG container structure without taking on either cost. Built only from the ODA's public Open Design Specification v5.4.1 and first-party byte inspection of public sample files. No Autodesk SDK, no ODA SDK, no LibreDWG source consulted at any point — the policy is written down in CLEANROOM.md.

What works today: [capability matrix](https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance). Entity-decoder correctness is the 0.2.0 milestone.

Feedback welcome — especially on the bit-cursor abstraction and the R14/R2000 object-stream layout gap.

Repo: https://github.com/DrunkOnJava/dwg-rs
