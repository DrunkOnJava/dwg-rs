//! Stubs for the eventual Python bindings — kept in-tree so the API
//! surface tracker (`Phase 2 API discipline`) can reference functions
//! that will exist once `PyO3` glue is wired up. Nothing in this
//! module ships compiled Python today.
//!
//! When the real bindings land they will live in a sibling crate
//! (`dwg-py`) wrapping these signatures one-to-one. Until then, this
//! module exists for documentation parity with the Rust API and to
//! anchor doctests / link checks in `CONTRIBUTING.md`.
//!
//! # Roadmap
//!
//! See `CONTRIBUTING.md` for the Python bindings roadmap. The first
//! cut will expose `DwgFile::open`, `summary`, `decoded_entities`,
//! and a JSON view of [`crate::api::Diagnostics`] via [`diagnostics`].

#![allow(dead_code)]

/// When Python bindings ship via PyO3, this function returns the
/// current [`crate::api::Diagnostics`] as a JSON string. Stub for API
/// parity tracking — see `CONTRIBUTING.md` for the Python bindings
/// roadmap.
///
/// The Rust-side caller will assemble a `Diagnostics` value as part
/// of a `Decoded<T>` and the binding layer will serialize it (likely
/// via `serde_json`) so Python callers get a stable dict shape:
///
/// ```text
/// {
///   "warnings": [{"code": "...", "message": "...", "bit_offset": 1234}, ...],
///   "skipped_records": 0,
///   "failed_streams": 0,
///   "partial_fields": 0
/// }
/// ```
///
/// Today the function returns an empty JSON object so signature
/// consumers can reference it without conditionally compiling.
pub fn diagnostics() -> String {
    String::from("{}")
}
