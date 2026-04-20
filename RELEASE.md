# Release process

This document describes how `dwg-rs` versions its releases and the
process to cut a new one.

## SemVer commitment

`dwg-rs` follows Semantic Versioning 2.0.0 with one wrinkle: while
the version is `0.x.y`, every `0.y` bump is treated as a
breaking-change release. `0.x` releases are always free to break
API between minor versions — this is explicit in the README's
"Pre-alpha status" section.

Once we reach `1.0.0`, standard SemVer applies: MAJOR for breaking
API changes, MINOR for additive changes, PATCH for bug fixes.

### What counts as breaking

- Removing / renaming a `pub` item
- Changing the signature of a `pub fn`
- Adding a required field to a `pub struct` not marked
  `#[non_exhaustive]`
- Changing the memory layout of a `#[repr(C)]` type
- Changing the MSRV (except to enable a soundness fix)

### What does NOT count as breaking

- Adding new `pub` items
- Adding variants to `#[non_exhaustive]` enums
- Internal optimisations that preserve observable behavior
- Changing the exact text of an error message
- Changing bundled fuzz corpus content

### MSRV (Minimum Supported Rust Version)

Currently **1.85**. Bumping the MSRV is a MINOR-version change
while we're in `0.y` and a MAJOR-version change once we reach
`1.0.0`. MSRV bumps are announced in the CHANGELOG entry for the
release.

## Release cadence

There is no fixed cadence. A release is cut when:

1. Meaningful user-facing progress has accumulated (a batch of
   per-entity decoders, a new export format, a write-path stage).
2. The change log for the window has no known regression flagged
   as "do not ship yet."
3. CI is green on the tip of `main`.
4. `cargo deny check` is clean.

## Cutting a release

```bash
# 1. Update the version.
#    - Cargo.toml `version = "X.Y.Z"`
#    - CITATION.cff `version: X.Y.Z` + `date-released: YYYY-MM-DD`
#    - CHANGELOG.md: move [Unreleased] items into a new
#      [X.Y.Z] - YYYY-MM-DD section; create a fresh [Unreleased].
#
# 2. Verify the tree.
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --release --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo deny check advisories bans licenses sources
cargo package --allow-dirty  # dry-run the publish tarball

# 3. Commit + tag.
git add Cargo.toml CITATION.cff CHANGELOG.md
git commit -m "release: vX.Y.Z"
git tag -s "vX.Y.Z" -m "vX.Y.Z"
git push origin main --tags

# 4. Publish.
cargo publish
```

Steps 3 and 4 are currently manual. The GitHub release workflow
(tracked as task #152) reacts to the pushed tag and builds binary
artifacts (macOS / Linux / Windows) automatically; the crates.io
publish step is `cargo publish` run locally until a PyPI/crates.io
publish workflow lands (Q-07 / #418 / #111).

## Yanking a release

If a shipped release is found to have a soundness bug (UB, panic
on a known-valid file, security issue):

```bash
cargo yank --vers X.Y.Z
```

followed by a MINOR/PATCH release that fixes the issue. Yanks
don't remove the crate from the registry; they prevent new
dependency resolution from picking it up. Document the reason in
the CHANGELOG's new release entry.

## Backport policy

While `0.x`, there is no backport policy — fixes land on `main`
and ship in the next release. Once `1.0.0` is out, the last MAJOR
release will receive security backports for 6 months; feature
backports are out of scope.

## Release artifacts

Each release produces:

- crates.io package: `cargo add dwg@X.Y.Z`
- GitHub release page with binary archives for the 4 CLI binaries
  (dwg-info / dwg-corpus / dwg-dump / dwg-convert) on the 3
  CI-supported platforms (macOS / Linux / Windows).
- Rustdoc published at `https://docs.rs/dwg/X.Y.Z`.

## Deprecation policy

Deprecations use `#[deprecated(since = "X.Y.Z", note = "…")]`.
Deprecated items stay in the codebase for at least one MAJOR
release cycle before removal, so callers have a full version to
migrate. Deprecation notes must include the recommended
replacement.
