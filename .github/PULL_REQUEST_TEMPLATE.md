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

## Source-provenance declaration

By submitting this PR I certify that my contribution does not incorporate executable code from any source whose license is incompatible with Apache-2.0. Specifically:

- **No Autodesk DWG SDK source** (RealDWG, ObjectARX, ObjectDBX) or NDA-protected Autodesk documentation was consulted or imported.
- **No Open Design Alliance Teigha / Drawings SDK source** was consulted or imported. The ODA's public *Open Design Specification for .dwg files* PDF is explicitly allowed (see `CLEANROOM.md`).
- **No GPL-licensed DWG implementation source** (LibreDWG, etc.) was consulted or imported. This matters because `dwg-rs` is Apache-2.0 and must remain compatible.

My contribution is based on:
- The ODA's freely-redistributable *Open Design Specification for .dwg files* (v5.4.1);
- Public reverse-engineering notes, research papers, or blog posts cited inline;
- Reading algorithm-description **comments** (not executable code) in permissively-licensed projects (MIT / Apache / BSD) to resolve spec ambiguities — disclose any such reading in this PR body so it can be recorded in `CLEANROOM.md`.

See `CLEANROOM.md` for the full allowed/forbidden source list and the honest scope of what "clean-room" means for this project.

## Developer Certificate of Origin

By submitting this PR, I certify that the contribution complies with the
[Developer Certificate of Origin 1.1](https://developercertificate.org/).
Specifically, I affirm that the contribution is my own work (or rightfully
licensed to me) and that it can be distributed under the project's Apache-2.0
license.

I will sign off commits with `git commit -s` when asked, which adds a
`Signed-off-by` trailer to the commit message asserting the DCO.
