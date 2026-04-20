//! V-06 — layer panel helpers for the JS viewer.
//!
//! Exposes the parent crate's LAYER table entries through a
//! JS-friendly `LayerInfo` shape so the browser can render a layer
//! panel (visibility toggles, color swatches).
//!
//! Visibility state itself lives in the [`crate::url_state::ViewerState`]
//! — this module provides the DWG side of the mapping (layer name
//! to index + color).

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::DwgFile;

/// JS-side view of one LAYER table entry.
#[derive(Serialize)]
struct LayerPanelEntry {
    /// 0-based index; stable across a single DwgFile instance.
    index: u32,
    /// Layer name (e.g. `"0"`, `"DIMENSIONS"`).
    name: String,
    /// AutoCAD Color Index 1..=255 for indexed colors; 0 for
    /// ByBlock / negative for special values.
    aci: i16,
    /// True when the layer has the frozen flag set.
    frozen: bool,
    /// `#RRGGBB` hex swatch resolved from the ACI.
    color_hex: String,
}

#[wasm_bindgen]
impl DwgFile {
    /// Return layer-panel entries as a JS array of `{ index, name,
    /// aci, frozen, color_hex }` records. Order is the LAYER table
    /// enumeration order (stable across opens of the same file).
    ///
    /// Today this is a skeleton: the full LAYER table extraction
    /// requires the parent crate's resolve_layer graph walker,
    /// which depends on trailing-handle decoding (see
    /// `src/graph.rs` module-level limitations note). Until that
    /// lands, this returns an empty array — the wasm API surface
    /// is stable; only the implementation is a stub.
    #[wasm_bindgen(js_name = "layerPanelEntries")]
    pub fn layer_panel_entries(&self) -> Result<JsValue, JsValue> {
        let entries: Vec<LayerPanelEntry> = Vec::new();
        // TODO: once trailing-handle decode lands, walk
        // `self.inner.all_objects()` collecting LAYER-type objects
        // and resolving their color / frozen flags. For now the
        // API surface exists so the JS panel code can be written
        // against a stable shape.
        serde_wasm_bindgen::to_value(&entries).map_err(|e| JsValue::from_str(&format!("{e}")))
    }
}
