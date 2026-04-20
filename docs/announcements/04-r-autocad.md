# r/AutoCAD / r/cad post draft (L-10)

**Suggested title:** Open-source Rust reader for DWG files (R13–R2018) — pre-alpha, measured status

---

**Body:**

I have been building dwg-rs, an open-source, Apache-2 reader for AutoCAD DWG files from R13 through R2018. Posting here because the longer-term goal is practical: drop a DWG into a browser tab, see it render, no AutoCAD install needed. A WASM viewer is on the roadmap (Phase 13).

Where things stand, without marketing gloss: pre-alpha. The container layer — recognizing DWG version, decompressing sections, reading metadata, listing every object — has landed and is exercised by the repository's unit + corpus tests against sample files from R14 through R2018.

Where it falls short: the per-entity decoders (the code that turns "object #42" into "a LINE from A to B") are alpha. The README publishes measured per-version numbers — about 86 % of entities decode on R2013, about 22 % on R2018, 25 % aggregate — so you can see the actual state instead of trusting a pitch. Raising that number is the next milestone.

You can already pull a DWG's embedded thumbnail, list the AutoCAD version that wrote the file, extract the metadata block, and walk the raw object table. The in-browser viewer needs the entity decoders to land first.

Why bother: the open-source DWG reader ecosystem is strong but split — LibreDWG (GPL-3, the most complete reader), ACadSharp (MIT, .NET). Nothing Apache-2.0 exists in Rust, which is what dwg-rs fills. Built from the ODA's public Open Design Specification; see CLEANROOM.md for the full source-provenance policy.

Repo: https://github.com/DrunkOnJava/dwg-rs

This is pre-alpha software. See [capability table](https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance) for measured decode rates.
