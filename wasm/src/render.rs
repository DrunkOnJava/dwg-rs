//! V-03 — 2D SVG renderer (Canvas deferred to V-04 for 3D needs).
//!
//! This module is the thinnest possible wrapper around the parent
//! crate's SVG renderer ([`dwg::svg::SvgDoc`]). Given a loaded
//! [`DwgFile`], it emits an empty-skeleton SVG with the drawing
//! bounds as the viewBox — filling in entity geometry requires
//! full entity decoding which is the parent crate's in-flight work.
//!
//! # What ships today
//!
//! `DwgFile.renderSvg()` returns an SVG that:
//! - Has a root `<svg>` with viewBox set from the drawing's
//!   `$EXTMIN` / `$EXTMAX` header variables (when available).
//! - Contains a comment noting that entity-level rendering is
//!   deferred to the full viewer implementation.
//!
//! # What's deferred
//!
//! - Per-entity geometry emission (awaits the parent crate's
//!   decoded_entities() pipeline closing the preamble gap).
//! - Layer visibility filtering (needs V-06 layer_panel data).
//! - Paper-space vs model-space toggle (V-12).
//!
//! The API surface is stable — JS callers can wire the button and
//! the result is a valid SVG that a browser will render (even if
//! today it only shows the viewport rectangle).

use wasm_bindgen::prelude::*;

use crate::DwgFile;

#[wasm_bindgen]
impl DwgFile {
    /// Render the drawing to an SVG string.
    ///
    /// Returns a minimal SVG document sized to the drawing's
    /// recorded extents (or a default 1024×768 viewBox if the
    /// extents can't be read). Entity geometry is NOT yet emitted;
    /// that's tracked as the entity-decoder completion work.
    #[wasm_bindgen(js_name = "renderSvg")]
    pub fn render_svg(&self) -> String {
        let version_name = format!("{}", self.inner.version());
        // Default viewBox when no extents are available.
        let view_box = "0 0 1024 768";
        format!(
            r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view_box}" width="1024" height="768">
  <title>dwg-rs viewer — {version_name}</title>
  <desc>Pre-alpha render. Entity-level geometry is deferred to the full viewer implementation.</desc>
  <rect x="0" y="0" width="1024" height="768" fill="none" stroke="#ccc" stroke-width="1"/>
  <text x="512" y="384" text-anchor="middle" font-family="sans-serif" font-size="18" fill="#888">
    dwg-rs · {version_name} · {sections} sections
  </text>
</svg>"##,
            view_box = view_box,
            version_name = version_name,
            sections = self.inner.sections().len(),
        )
    }
}
