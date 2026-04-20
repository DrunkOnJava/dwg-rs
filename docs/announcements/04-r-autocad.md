# r/AutoCAD / r/cad post draft (L-10)

**Suggested title:** Open-source Rust reader for DWG files (R13–R2018) — honest status

---

**Body:**

I've been building dwg-rs, an open-source, Apache-2 reader for AutoCAD DWG files from R13 through R2018. Posting here because the end goal is practical: drop a DWG into a browser tab, see it render, no AutoCAD install needed. A WASM viewer is on the roadmap (Phase 13).

Where things stand, without marketing gloss: pre-alpha. The container layer — recognizing DWG version, decompressing sections, reading metadata, listing every object — works and is covered by 193 tests across sample files from R14 through R2018.

Where it falls short: the per-entity decoders (the code that turns "object #42" into "a LINE from A to B") are still landing. The README publishes measured per-version numbers — about 86 % of entities decode on R2013, about 22 % on R2018 — so you can see the real state instead of trusting a pitch. Raising that number is the next milestone.

You can already pull a DWG's embedded thumbnail, list the AutoCAD version that wrote the file, extract the metadata block, and walk the raw object table. The in-browser viewer needs the entity decoders to land first.

Why bother: no permissively-licensed DWG reader exists today — the choices are paid-membership SDKs or GPL-3. dwg-rs is built clean-room from the public Open Design Specification, so there's a real path to a viewer anyone can embed.

Repo: https://github.com/DrunkOnJava/dwg-rs
