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
//! # Strict vs best-effort parity (API-12)
//!
//! The Rust side of the library exposes `summarize_strict` /
//! `summarize_lossy`, `read_object_strict` / `read_object_lossy`, and
//! `HeaderVars::parse_strict` / `parse_lossy` pairs. The Python
//! bindings mirror these one-to-one by appending `_strict` or
//! `_lossy` to each JSON-export method. Strict variants raise a
//! `DwgError` on the first decode failure; lossy variants always
//! return a `(value, diagnostics)` tuple where `value` may be partial
//! and `diagnostics` enumerates what was skipped.
//!
//! # Roadmap
//!
//! See `CONTRIBUTING.md` for the Python bindings roadmap. The first
//! cut will expose `DwgFile.open`, `summary`, `decoded_entities`, and
//! a JSON view of [`crate::api::Diagnostics`] via [`diagnostics`].

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

/// Python-binding stub for `DwgFile.summary_strict(path)`. When the
/// real bindings land, this returns the `Summary` JSON or raises
/// `DwgError` on the first decode failure.
///
/// Today returns an empty JSON object so signature consumers can
/// reference it. Panics are intentionally avoided — the stub is
/// inert.
pub fn summary_strict(_path: &str) -> String {
    String::from("{}")
}

/// Python-binding stub for `DwgFile.summary_lossy(path)`. When the
/// real bindings land, this returns a `(summary, diagnostics)` tuple
/// where `summary` may contain partial data and `diagnostics`
/// enumerates skipped sections.
///
/// Today returns the JSON shape `{"value": {}, "diagnostics": {}}`.
pub fn summary_lossy(_path: &str) -> String {
    String::from(r#"{"value":{},"diagnostics":{}}"#)
}

/// Python-binding stub for `DwgFile.read_object_strict(handle)`. Will
/// raise `DwgError` when the object fails to decode cleanly.
pub fn read_object_strict(_handle: u64) -> String {
    String::from("{}")
}

/// Python-binding stub for `DwgFile.read_object_lossy(handle)`. Will
/// return `(value, diagnostics)` — value may be the raw bytes when
/// typed decode fails.
pub fn read_object_lossy(_handle: u64) -> String {
    String::from(r#"{"value":{},"diagnostics":{}}"#)
}

/// Python-binding stub for `HeaderVars.parse_strict(path)`. Raises
/// `DwgError` on the first unknown variable or short-read.
pub fn header_vars_strict(_path: &str) -> String {
    String::from("{}")
}

/// Python-binding stub for `HeaderVars.parse_lossy(path)`. Always
/// returns `(vars, diagnostics)`. `vars` may omit entries that
/// failed to decode; `diagnostics.partial_fields` counts them.
pub fn header_vars_lossy(_path: &str) -> String {
    String::from(r#"{"value":{},"diagnostics":{}}"#)
}

/// Python-binding stub for `DwgFile.open_with_limits(path, limits)`.
/// The `limits` parameter on the Python side will be a dict keyed by
/// `max_file_bytes`, `max_section_bytes`, `max_output_bytes`,
/// `max_objects`, and `max_handles` (per SEC-10). Stub returns an
/// empty JSON object.
pub fn open_with_limits(_path: &str, _limits_json: &str) -> String {
    String::from("{}")
}

/// Python-binding stub for `DwgFile.decoded_entities_strict()`.
/// Iterates decoded entities; raises on the first typed-decode
/// failure.
pub fn decoded_entities_strict() -> String {
    String::from("[]")
}

/// Python-binding stub for `DwgFile.decoded_entities_lossy()`.
/// Returns a list of `(entity, diagnostics)` pairs where `entity`
/// may be the raw bytes when typed decode fails.
pub fn decoded_entities_lossy() -> String {
    String::from(r#"{"entities":[],"diagnostics":{}}"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stubs_return_valid_json() {
        // Every stub returns a string that serde_json (when
        // available via the cli feature) could parse. We don't
        // require serde_json here in lib-only mode, so just assert
        // the strings are non-empty.
        assert!(!diagnostics().is_empty());
        assert!(!summary_strict("/does/not/exist").is_empty());
        assert!(!summary_lossy("/does/not/exist").is_empty());
        assert!(!read_object_strict(0).is_empty());
        assert!(!read_object_lossy(0).is_empty());
        assert!(!header_vars_strict("/does/not/exist").is_empty());
        assert!(!header_vars_lossy("/does/not/exist").is_empty());
        assert!(!open_with_limits("/does/not/exist", "{}").is_empty());
        assert!(!decoded_entities_strict().is_empty());
        assert!(!decoded_entities_lossy().is_empty());
    }

    #[test]
    fn lossy_variants_emit_diagnostics_field() {
        assert!(summary_lossy("x").contains("\"diagnostics\""));
        assert!(read_object_lossy(0).contains("\"diagnostics\""));
        assert!(header_vars_lossy("x").contains("\"diagnostics\""));
        assert!(decoded_entities_lossy().contains("\"diagnostics\""));
    }

    #[test]
    fn strict_variants_omit_diagnostics_wrapper() {
        // Strict variants return the value directly — no
        // (value, diagnostics) tuple. Placeholder is "{}".
        assert_eq!(summary_strict("x"), "{}");
        assert_eq!(read_object_strict(0), "{}");
        assert_eq!(header_vars_strict("x"), "{}");
    }
}
