//! WebAssembly bindings for `dwg-rs` (V-01 build pipeline + V-02 JS API).
//!
//! # Honest scope
//!
//! This crate is pre-alpha scaffolding for the browser-side viewer
//! Phase 13. It wraps the top-level [`dwg::DwgFile`] API in a
//! wasm-bindgen surface that JavaScript can consume.
//!
//! The public JS API is intentionally small:
//!   - `DwgFile.open(bytes: Uint8Array)` — parse from an uploaded file
//!   - `DwgFile.version()` — detected DWG version as a string
//!   - `DwgFile.sections()` — array of `{ name, size, offset }` records
//!   - `DwgFile.section_map_status()` — `"Full" | "Fallback" | "Deferred"`
//!
//! Entity iteration + typed export (SVG / glTF) will land in V-03/V-04;
//! this first cut proves the wasm-pack build pipeline works end-to-end.
//!
//! # Build
//!
//! ```text
//! cd wasm
//! wasm-pack build --target web --release
//! # output: pkg/dwg_wasm_bg.wasm + pkg/dwg_wasm.js
//! ```
//!
//! # Safety
//!
//! The parent `dwg` crate is `#![forbid(unsafe_code)]`; this wasm
//! wrapper adds only the wasm-bindgen-generated `extern "C"` shims.
//! All panics abort (per `panic = "abort"` in Cargo.toml) — the
//! parent crate returns `Result<T, Error>` on every malformed input,
//! so panic paths are unreachable on well-formed inputs.

#![forbid(unsafe_code)]

pub mod measure;

use serde::Serialize;
use wasm_bindgen::prelude::*;

/// JS-side view of one DWG section.
#[derive(Serialize)]
struct SectionView {
    name: String,
    size: u64,
    offset: u64,
    kind: String,
}

/// A loaded DWG file. Owns the decoded header + section list.
#[wasm_bindgen]
pub struct DwgFile {
    inner: dwg::DwgFile,
}

#[wasm_bindgen]
impl DwgFile {
    /// Parse a DWG file from an uploaded byte buffer (typically
    /// `new Uint8Array(await file.arrayBuffer())` in the browser).
    ///
    /// Throws a JS `Error` with the parse failure message if the
    /// bytes aren't a recognizable DWG file.
    #[wasm_bindgen(js_name = "open")]
    pub fn open(bytes: &[u8]) -> Result<DwgFile, JsValue> {
        let owned = bytes.to_vec();
        let inner = dwg::DwgFile::from_bytes(owned)
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        Ok(DwgFile { inner })
    }

    /// Detected DWG version as an ASCII string (e.g. "AC1032" for
    /// R2018). Available even when the section map fell back.
    #[wasm_bindgen(js_name = "versionMagic")]
    pub fn version_magic(&self) -> String {
        String::from_utf8_lossy(&self.inner.version().magic()).into_owned()
    }

    /// Human-readable version name (e.g. "R2018"). Convenience for
    /// UI labels.
    #[wasm_bindgen(js_name = "versionName")]
    pub fn version_name(&self) -> String {
        format!("{}", self.inner.version())
    }

    /// Return the section list as a JS array of `{ name, size,
    /// offset, kind }` objects.
    #[wasm_bindgen(js_name = "sections")]
    pub fn sections(&self) -> Result<JsValue, JsValue> {
        let views: Vec<SectionView> = self
            .inner
            .sections()
            .iter()
            .map(|s| SectionView {
                name: s.name.clone(),
                size: s.size,
                offset: s.offset,
                kind: format!("{:?}", s.kind),
            })
            .collect();
        serde_wasm_bindgen::to_value(&views).map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    /// Section-map status: "Full" | "Fallback" | "Deferred" —
    /// callers should treat Fallback + Deferred as advisory.
    #[wasm_bindgen(js_name = "sectionMapStatus")]
    pub fn section_map_status(&self) -> String {
        match self.inner.section_map_status() {
            dwg::SectionMapStatus::Full => "Full".to_string(),
            dwg::SectionMapStatus::Fallback { .. } => "Fallback".to_string(),
            dwg::SectionMapStatus::Deferred { .. } => "Deferred".to_string(),
        }
    }
}

/// Returns the dwg-rs crate version string embedded at build time.
/// Useful for JS-side compat checks against a published viewer.
#[wasm_bindgen(js_name = "crateVersion")]
pub fn crate_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
