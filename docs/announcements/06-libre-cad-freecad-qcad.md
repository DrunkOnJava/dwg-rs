# Community post: LibreCAD / FreeCAD / QCAD forums

**Title:** dwg-rs — permissive (Apache-2.0) Rust foundation for DWG read

**Audience:** LibreCAD, FreeCAD, and QCAD community forums. Minor variants per-forum; this draft is the common trunk.

**Status:** Draft. Framing is "another tool in the box alongside LibreDWG," not a replacement.

---

Hello,

I am sharing a new open-source project that may be useful to your ecosystem: `dwg-rs`, an Apache-2.0 Rust reader for AutoCAD DWG files. It is pre-alpha today — the container layer has landed, per-entity decoders are partial. The readme publishes the measured decode rates by version rather than marketing numbers.

**Why another DWG project?** LibreDWG is the longstanding open option and deserves credit — it has moved the ecosystem forward for years. It is licensed GPL-3, which disqualifies it from downstream projects that ship under Apache-2.0, MIT, BSD, MPL, or commercial licenses that cannot absorb copyleft. `dwg-rs` exists to fill exactly that gap: a DWG read path that can be pulled into permissively-licensed stacks without changing the host project's license posture, for the subset of DWG features already covered. The two projects serve different audiences and I view them as complementary.

**For LibreCAD:** the existing `libdxfrw` path handles DXF; a Rust-based DWG reader linked via an FFI shim could eventually give LibreCAD a DWG-read path that does not depend on the DXF side-channel and does not pull GPL-3 into the binary. I am happy to advise on the shim design if that is interesting.

**For FreeCAD:** the current DWG story leans on external converters. A Rust library called through a small Python binding (pyo3 is on the roadmap — see [`docs/python.md`](../python.md) for the planned surface and 0.2.0 target) could eventually give the Draft / Arch / TechDraw workbenches a direct DWG-read path, in-process, without shelling out.

**For QCAD:** integration is trickier because of the Qt / C++ surface, but a C ABI wrapper around `dwg-rs` is a tractable design and would support both the community and professional editions' licensing structures.

**Status honesty:** file identification, LZ77, section map, Sec_Mask layer-1, CRC, Reed-Solomon decode, metadata (SummaryInfo / AppInfo / Preview / FileDepList), and the raw object-stream walker have landed and are exercised across a 19-file public corpus. Per-entity decoders (LINE, MTEXT, CIRCLE, etc.) exist but fail on portions of real-world R2004-family files — closing that gap is the 0.2.0 milestone. If your use case is *read metadata + enumerate raw objects*, that path works today. If it is *render a drawing*, it does not yet.

Source-provenance policy: executable code from the Autodesk SDK, the ODA SDK, and GPL-licensed DWG readers was not imported. The build is based on the ODA *Open Design Specification for .dwg files* (v5.4.1) PDF, public sample files, and independent byte inspection; one scoped exception (algorithm-description comments in the MIT-licensed ACadSharp for one LZ77 offset ambiguity) is documented in `CLEANROOM.md`.

Integration conversation welcome. Repository: https://github.com/DrunkOnJava/dwg-rs

Griffin Long

This is pre-alpha software. See [capability table](https://github.com/DrunkOnJava/dwg-rs#capability-matrix-at-a-glance) for measured decode rates.
