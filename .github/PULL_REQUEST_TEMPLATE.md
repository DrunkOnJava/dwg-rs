<!--
Thank you for the PR. Please read below before submitting.
-->

## Summary

<!-- One sentence on what this PR does. -->

## Spec reference

<!--
If this PR decodes a new format feature, cite the ODA spec section
(e.g. "spec §19.4.25 LWPOLYLINE"). If it fixes a bug, link the issue.
-->

## Test plan

<!--
- [ ] `cargo test --release` passes
- [ ] `cargo test --release --test proptest_roundtrip` passes
- [ ] `cargo test --release --test corpus_roundtrip` passes (if samples available)
- [ ] New code covered by unit tests
- [ ] `cargo publish --dry-run` succeeds (for release PRs only)
-->

## Clean-room declaration

By submitting this PR, I confirm that I have **not** consulted:

- Autodesk's DWG SDK source or documentation under NDA.
- The Open Design Alliance's closed-source SDK (`Teigha`, etc.).
- LibreDWG or any other GPL-licensed DWG implementation.

My implementation is based exclusively on the ODA's freely-published
*Open Design Specification for .dwg files* (v5.4.1), public format
reverse-engineering notes, and (where noted in code comments)
cross-verification against permissively-licensed implementations
(ACadSharp / MIT).
