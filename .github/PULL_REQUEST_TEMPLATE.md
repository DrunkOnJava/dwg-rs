<!--
Thank you for the PR. Please read below before submitting.
-->

## Summary

<!-- One sentence on what this PR does. -->

## Spec reference

<!--
If this PR decodes a new format feature, cite the ODA spec section
(e.g. "spec §19.4.25 LWPOLYLINE"). If it fixes a bug, link the issue.
If it improves entity-decode coverage, include before/after numbers
from `cargo run --release --example coverage_report -- <path>`.
-->

## Test plan

<!--
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test --release` passes
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` passes
- [ ] New code covered by unit or proptest tests
- [ ] For decoder changes: coverage-report numbers attached
-->

## Clean-room declaration

By submitting this PR I certify that:

- **I have not consulted the Autodesk DWG SDK** source code or NDA-protected documentation in creating this contribution.
- **I have not consulted the Open Design Alliance's closed-source Teigha / ODA SDK** source code.
- **I have not consulted LibreDWG** (GPL-3) or any other copyleft-licensed DWG implementation in creating this contribution. This is a critical legal requirement: `dwg-rs` is Apache-2.0 and must remain free of GPL-adjacent contamination.

My contribution is based exclusively on:
- The ODA's freely-redistributable *Open Design Specification for .dwg files* (v5.4.1);
- Public reverse-engineering notes, research papers, or blog posts where cited inline;
- Cross-verification against permissively-licensed implementations (e.g., ACadSharp / MIT) where noted in code comments — and such verification is limited to reading the algorithmic description, not copying code.

## Developer Certificate of Origin

By submitting this PR, I certify that the contribution complies with the
[Developer Certificate of Origin 1.1](https://developercertificate.org/).
Specifically, I affirm that the contribution is my own work (or rightfully
licensed to me) and that it can be distributed under the project's Apache-2.0
license.

I will sign off commits with `git commit -s` when asked, which adds a
`Signed-off-by` trailer to the commit message asserting the DCO.
