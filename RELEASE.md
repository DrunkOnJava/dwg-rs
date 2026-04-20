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

The release pipeline is automated behind a manual-approval gate
(Q-07 / #418). The human-driven portion is steps 1–4 below; steps
5–7 are performed by `.github/workflows/release.yml` and surface
in the Actions tab.

### Pre-flight checklist

Before tagging, confirm:

- [ ] `main` is green on `ci.yml` (fmt / clippy / test matrix / doc / deny / msrv / coverage-smoke / pii-guard / gitleaks).
- [ ] `main` is green on `perf.yml` and the run recorded a fresh `main` baseline.
- [ ] `docs-rs.yml` is green on the most recent PR merged into main.
- [ ] `cargo deny check` is clean (also run as part of `ci.yml`, but worth re-running locally with the current Cargo.lock).
- [ ] The `CHANGELOG.md` `[Unreleased]` section has been triaged — nothing in there is tagged "do not ship yet."

### 1. Update version metadata

- `Cargo.toml` — bump `version = "X.Y.Z"`.
- `CITATION.cff` — bump `version: X.Y.Z`, set `date-released: YYYY-MM-DD`.
- `CHANGELOG.md` — move every entry in `[Unreleased]` into a new
  `[X.Y.Z] - YYYY-MM-DD` section, and re-create an empty `[Unreleased]`
  heading. Keep the Keep-a-Changelog structure.

### 2. Local verification

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --release --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo deny check advisories bans licenses sources
cargo package --allow-dirty  # dry-run the publish tarball
```

### 3. Commit + tag

```bash
git add Cargo.toml CITATION.cff CHANGELOG.md
git commit -m "release: vX.Y.Z"
git tag -s "vX.Y.Z" -m "vX.Y.Z"
git push origin main --tags
```

Tags must match the SemVer regex enforced by `release.yml`:
`v[0-9]+.[0-9]+.[0-9]+` or `v[0-9]+.[0-9]+.[0-9]+-prerelease`.

### 4. Monitor the pipeline

The tag push triggers `release.yml` which runs, in order:

1. `verify` — `cargo publish --dry-run --all-features`.
2. `sbom` — CycloneDX JSON SBOM generation.
3. `binaries` — release builds for the 5 CLI tools
   (`dwg-info`, `dwg-corpus`, `dwg-dump`, `dwg-convert`,
   `dwg-to-dxf`) across 5 targets:
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
   - `x86_64-pc-windows-msvc`
4. `github-release` — creates a **draft** GitHub release with all
   archives + `bom.json` attached. This is where you review the
   auto-generated release notes, edit the description, and
   manually press "Publish release" once satisfied.
5. `publish-crates-io` — gated behind the `crates-io` environment.
   A repo maintainer must click "Approve" in the Actions UI. The
   job then runs `cargo publish --dry-run` one more time, and on
   success, `cargo publish` for real. **`cargo publish` is
   irrevocable**; the only remediation is `cargo yank`.

Prereleases (`-alpha`, `-beta`, `-rc`) skip `publish-crates-io`
entirely and produce a draft GitHub release marked `prerelease`.

### 5. Post-publish

- Wait for docs.rs to complete the build
  (`https://docs.rs/dwg/X.Y.Z`). docs.rs runs asynchronously; if
  the build fails, the CHANGELOG entry for this version should be
  amended in a follow-up commit noting the docs.rs issue.
- Announce in the CHANGELOG `[Unreleased]` for the next cycle if
  anything post-release surfaced.
- Publish the draft GitHub release from step 4.4 once the notes
  read well.

### What's still manual

- Tag signing (`git tag -s`) is recommended but not enforced in CI.
  A TODO in `release.yml` flags the eventual `gpg --verify` step.
- `CHANGELOG.md` generation is manual; `cliff.toml` is committed so
  contributors can run `git-cliff --tag vX.Y.Z --prepend CHANGELOG.md`
  as a starting point, but the final edit is hand-curated.
- Python wheels are not shipped yet. The `publish-pypi` job in
  `release.yml` is gated off with `if: false` until `python_stubs.rs`
  graduates from its placeholder (tracked alongside Phase 13).

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
