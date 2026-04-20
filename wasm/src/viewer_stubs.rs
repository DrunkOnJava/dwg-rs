//! Viewer API stubs for the deferred V-* tasks.
//!
//! Each method here exists so JS-side wiring can be written against
//! a stable API surface today. The implementations return empty /
//! placeholder values; the real rendering happens when the parent
//! crate's entity pipeline + trailing-handle decoder close their
//! respective gaps (see `src/graph.rs` module docs for the
//! precise limitations).
//!
//! Shipped stubs:
//!   V-08 — hatch pattern rendering         (pattern_to_svg_fill)
//!   V-09 — text rendering with font fallback (text_to_svg_element)
//!   V-10 — dimension rendering             (dimension_to_svg_group)
//!   V-11 — block expansion on-the-fly      (expand_inserts)
//!   V-12 — model/paper space toggle        (setSpace, activeSpace)
//!   V-13 — print preview                   (printPreview)
//!   V-16 — WebWorker offloading            (WORKER_READY, send/receive docs)
//!   V-17 — progressive streaming           (openProgressive)
//!   V-21 — section-box clipping (3D)       (SectionBox, setSectionBox)
//!   V-23 — selection + property panel      (hitTest, entityProperties)

use wasm_bindgen::prelude::*;

use crate::{DwgFile, viewer::Viewer};

// -------- V-08 hatch --------

/// Placeholder: convert a DWG hatch pattern name (e.g. "ANSI31")
/// to an SVG pattern fill attribute. Today returns a simple
/// cross-hatch for the common named patterns; unrecognized names
/// fall back to solid gray.
#[wasm_bindgen(js_name = "hatchPatternToFill")]
pub fn hatch_pattern_to_fill(name: &str) -> String {
    match name {
        "SOLID" => "#888".to_string(),
        "ANSI31" | "HATCH" => "url(#dwg-hatch-ansi31)".to_string(),
        _ => "#ccc".to_string(),
    }
}

// -------- V-09 text --------

/// Convert a DWG text payload to an SVG `<text>` element with
/// SHX → Arial font fallback. Callers supply position + size;
/// this helper handles the style attributes only.
#[wasm_bindgen(js_name = "textToSvgElement")]
pub fn text_to_svg_element(text: &str, x: f64, y: f64, height: f64, font_family: &str) -> String {
    let family = if font_family.to_lowercase().ends_with(".shx") {
        "Arial, sans-serif".to_string()
    } else {
        format!("{font_family}, sans-serif")
    };
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<text x="{x}" y="{y}" font-family="{family}" font-size="{height}">{escaped}</text>"#
    )
}

// -------- V-10 dimension --------

/// Emit an SVG `<g>` wrapping a dimension line's extension lines,
/// dim line, arrows, and text label. Stub: today emits an empty
/// `<g>` with a data-attribute; real geometry lands when the
/// parent crate's dimension decoder feeds into
/// entity_geometry::dimension_to_paths and svg::push_dimension_linear.
#[wasm_bindgen(js_name = "dimensionToSvgGroup")]
pub fn dimension_to_svg_group(handle_hex: &str) -> String {
    format!(r#"<g data-dwg-handle="{handle_hex}" data-kind="dimension"></g>"#)
}

// -------- V-11 block expansion --------

#[wasm_bindgen]
impl DwgFile {
    /// Expand all INSERT entities into their constituent block
    /// bodies, applying per-instance transforms. Today returns 0
    /// (block expansion requires the trailing-handle decoder that
    /// the parent crate documents as in-progress).
    #[wasm_bindgen(js_name = "expandInserts")]
    pub fn expand_inserts(&self) -> u32 {
        0
    }
}

// -------- V-12 model / paper space toggle --------

#[wasm_bindgen]
impl Viewer {
    /// Select which space to render. `"model"` or a paper-space
    /// layout name like `"Layout1"`. Today a no-op; once entity
    /// iteration lands, this filters via
    /// graph::filter_by_paper_space_block.
    #[wasm_bindgen(js_name = "setSpace")]
    pub fn set_space(&self, _space: &str) {
        // Storage lives in Viewer once render uses it — today a no-op.
    }

    /// Currently-active space label. Defaults to `"model"`.
    #[wasm_bindgen(js_name = "activeSpace")]
    pub fn active_space(&self) -> String {
        "model".to_string()
    }
}

// -------- V-13 print preview --------

#[wasm_bindgen]
impl DwgFile {
    /// Render a paper-sized SVG preview of the named layout.
    /// Today returns the same skeleton SVG as `renderSvg()`.
    #[wasm_bindgen(js_name = "printPreview")]
    pub fn print_preview(&self, _layout_name: &str) -> String {
        self.render_svg()
    }
}

// -------- V-16 WebWorker --------

/// Signal to JS that the WASM is safe to load inside a dedicated
/// WebWorker — the `DwgFile` type is `Send + Sync`-equivalent
/// (no threads are shared; the worker owns its own instance).
///
/// JS usage:
/// ```js
/// const worker = new Worker('/pkg/dwg_wasm_worker.js', { type: 'module' });
/// worker.postMessage({ cmd: 'parse', bytes });
/// worker.addEventListener('message', ev => {
///   // { ok: true, version, sections } on success
/// });
/// ```
/// Function form (wasm_bindgen doesn't expose consts to JS).
#[wasm_bindgen(js_name = "workerReady")]
pub fn worker_ready() -> bool {
    true
}

// -------- V-17 progressive streaming --------

#[wasm_bindgen]
impl DwgFile {
    /// Progressive-open helper. Takes the full byte buffer today;
    /// the streaming variant (chunk-by-chunk parsing) is a
    /// follow-up that requires incremental-parser plumbing in the
    /// parent crate's section_map. Returns the same DwgFile as
    /// `open()` plus a `chunks_consumed` hint.
    #[wasm_bindgen(js_name = "openProgressive")]
    pub fn open_progressive(bytes: &[u8]) -> Result<DwgFile, JsValue> {
        DwgFile::open(bytes)
    }
}

// -------- V-21 section-box (3D) --------

/// 3D section box clipping — stable API stub. 3D rendering is V-04
/// (Three.js integration, deferred). When V-04 lands, the Viewer
/// applies this clip to the scene before rendering.
#[wasm_bindgen]
pub struct SectionBox {
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
    enabled: bool,
}

#[wasm_bindgen]
impl SectionBox {
    #[wasm_bindgen(constructor)]
    pub fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> SectionBox {
        SectionBox {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
            enabled: true,
        }
    }
    #[wasm_bindgen(js_name = "enabled")]
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    #[wasm_bindgen(js_name = "setEnabled")]
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    #[wasm_bindgen(js_name = "bounds")]
    pub fn bounds(&self) -> Box<[f64]> {
        Box::new([
            self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z,
        ])
    }
}

// -------- V-23 selection + property panel --------

#[wasm_bindgen]
impl DwgFile {
    /// Hit-test a drawing-space point against the decoded entities.
    /// Returns the handle of the closest entity within `tolerance`,
    /// or an empty string if none. Today returns empty (needs
    /// entity iteration through the trailing-handle decoder).
    #[wasm_bindgen(js_name = "hitTest")]
    pub fn hit_test(&self, _x: f64, _y: f64, _tolerance: f64) -> String {
        String::new()
    }

    /// Look up an entity's properties by handle (hex string).
    /// Returns a JSON object with `{ type, layer, color, linetype,
    /// handle_hex }` plus any typed fields. Today returns
    /// `{ "error": "not found" }` until hit_test resolves.
    #[wasm_bindgen(js_name = "entityProperties")]
    pub fn entity_properties(&self, _handle_hex: &str) -> String {
        r#"{"error":"not yet implemented — awaiting trailing-handle decoder"}"#.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hatch_solid_returns_flat_color() {
        assert_eq!(hatch_pattern_to_fill("SOLID"), "#888");
    }

    #[test]
    fn hatch_named_returns_pattern_reference() {
        assert!(hatch_pattern_to_fill("ANSI31").starts_with("url(#"));
    }

    #[test]
    fn text_shx_font_falls_back_to_arial() {
        let svg = text_to_svg_element("Hello", 0.0, 0.0, 12.0, "gdt.shx");
        assert!(svg.contains("Arial, sans-serif"));
    }

    #[test]
    fn text_truetype_pass_through() {
        let svg = text_to_svg_element("Hi", 0.0, 0.0, 10.0, "Helvetica");
        assert!(svg.contains("Helvetica, sans-serif"));
    }

    #[test]
    fn text_escapes_xml_special_chars() {
        let svg = text_to_svg_element("A & <B>", 0.0, 0.0, 10.0, "Arial");
        assert!(svg.contains("A &amp; &lt;B&gt;"));
    }

    #[test]
    fn dimension_group_carries_data_attributes() {
        let g = dimension_to_svg_group("0x42");
        assert!(g.contains(r#"data-dwg-handle="0x42""#));
        assert!(g.contains(r#"data-kind="dimension""#));
    }

    #[test]
    fn section_box_round_trip() {
        let mut sb = SectionBox::new(0.0, 0.0, 0.0, 10.0, 20.0, 30.0);
        let b = sb.bounds();
        assert_eq!(b.len(), 6);
        assert_eq!(b[3], 10.0);
        assert_eq!(b[4], 20.0);
        assert_eq!(b[5], 30.0);
        assert!(sb.enabled());
        sb.set_enabled(false);
        assert!(!sb.enabled());
    }
}
