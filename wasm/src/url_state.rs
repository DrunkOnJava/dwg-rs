//! V-24 — URL-share of viewer state.
//!
//! Serializes a minimal `ViewerState` struct (pan + zoom + active
//! layout + layer visibility bitmap) into a URL-safe base64 string,
//! and decodes back on load. Target ~ ≤ 1 KB before encoding so the
//! result fits in a typical 2000-char URL.
//!
//! # Encoding
//!
//! Hand-rolled base64url (no `=` padding) over a hand-packed binary
//! state to keep the wasm crate free of base64 + serde_json deps.
//!
//! Layout:
//!
//! ```text
//!  [0..8]   magic bytes   = "DWGV1   " (ASCII, 8 bytes)
//!  [8..16]  pan_x         = f64 LE
//!  [16..24] pan_y         = f64 LE
//!  [24..32] zoom          = f64 LE
//!  [32..33] space         = 0 (model) | 1 (paper)
//!  [33..35] layout_index  = u16 LE  (0 ⇒ default, ≥1 ⇒ paper-space tab index)
//!  [35..37] layer_count   = u16 LE  (max 512, otherwise reject)
//!  [37..]   layer_visibility bitmap (layer_count bits, rounded up to bytes)
//! ```

use wasm_bindgen::prelude::*;

const MAGIC: &[u8; 8] = b"DWGV1   ";
const MIN_LEN: usize = 37;
const MAX_LAYERS: u16 = 512;

/// Viewer state encoded as a shareable URL-safe string.
#[wasm_bindgen]
pub struct ViewerState {
    pan_x: f64,
    pan_y: f64,
    zoom: f64,
    /// 0 = model space, 1 = paper space.
    space: u8,
    /// Paper-space layout tab index (0 = Layout1 / first, up to 65_535).
    layout_index: u16,
    /// One bit per layer. `true` = visible. Index 0 = layer 0 ("0").
    layer_visible: Vec<bool>,
}

#[wasm_bindgen]
impl ViewerState {
    /// Construct a default (model-space, unit-zoom, no layers) state.
    #[wasm_bindgen(constructor)]
    pub fn new() -> ViewerState {
        ViewerState {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
            space: 0,
            layout_index: 0,
            layer_visible: Vec::new(),
        }
    }

    #[wasm_bindgen(js_name = "setPan")]
    pub fn set_pan(&mut self, x: f64, y: f64) {
        self.pan_x = x;
        self.pan_y = y;
    }

    #[wasm_bindgen(js_name = "setZoom")]
    pub fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom;
    }

    #[wasm_bindgen(js_name = "setSpace")]
    pub fn set_space(&mut self, model_space: bool, layout_index: u16) {
        self.space = if model_space { 0 } else { 1 };
        self.layout_index = layout_index;
    }

    #[wasm_bindgen(js_name = "setLayerVisible")]
    pub fn set_layer_visible(&mut self, index: u16, visible: bool) -> Result<(), JsValue> {
        if index >= MAX_LAYERS {
            return Err(JsValue::from_str(&format!(
                "layer index {index} exceeds maximum {MAX_LAYERS}"
            )));
        }
        let i = index as usize;
        while self.layer_visible.len() <= i {
            self.layer_visible.push(true);
        }
        self.layer_visible[i] = visible;
        Ok(())
    }

    #[wasm_bindgen(js_name = "panX")]
    pub fn pan_x(&self) -> f64 {
        self.pan_x
    }

    #[wasm_bindgen(js_name = "panY")]
    pub fn pan_y(&self) -> f64 {
        self.pan_y
    }

    #[wasm_bindgen(js_name = "zoom")]
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    #[wasm_bindgen(js_name = "isModelSpace")]
    pub fn is_model_space(&self) -> bool {
        self.space == 0
    }

    #[wasm_bindgen(js_name = "layoutIndex")]
    pub fn layout_index(&self) -> u16 {
        self.layout_index
    }

    /// Serialize to a URL-safe base64 string.
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string_js(&self) -> String {
        let mut buf = Vec::with_capacity(MIN_LEN + self.layer_visible.len().div_ceil(8));
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&self.pan_x.to_le_bytes());
        buf.extend_from_slice(&self.pan_y.to_le_bytes());
        buf.extend_from_slice(&self.zoom.to_le_bytes());
        buf.push(self.space);
        buf.extend_from_slice(&self.layout_index.to_le_bytes());
        let layer_count = self.layer_visible.len().min(MAX_LAYERS as usize) as u16;
        buf.extend_from_slice(&layer_count.to_le_bytes());
        let nbytes = (layer_count as usize).div_ceil(8);
        let mut bitmap = vec![0u8; nbytes];
        for (i, &v) in self.layer_visible.iter().take(layer_count as usize).enumerate() {
            if v {
                bitmap[i / 8] |= 1 << (i % 8);
            }
        }
        buf.extend_from_slice(&bitmap);
        base64url_encode(&buf)
    }

    /// Parse a URL-safe base64 string back into a ViewerState.
    #[wasm_bindgen(js_name = "fromString")]
    pub fn from_string_js(encoded: &str) -> Result<ViewerState, JsValue> {
        from_string_inner(encoded).map_err(|e| JsValue::from_str(&e))
    }
}

/// Pure-Rust core of the deserialization path. Returns a `String`
/// error so unit tests can run in the standard `cargo test` harness
/// without crossing the wasm_bindgen boundary (which requires a JS
/// runtime for `JsValue::from_str`).
pub(crate) fn from_string_inner(encoded: &str) -> Result<ViewerState, String> {
    let buf = base64url_decode(encoded).map_err(|e| format!("base64 decode: {e}"))?;
    if buf.len() < MIN_LEN {
        return Err(format!(
            "ViewerState blob too short: {} < {MIN_LEN}",
            buf.len()
        ));
    }
    if &buf[0..8] != MAGIC {
        return Err("ViewerState magic mismatch".to_string());
    }
    let pan_x = f64::from_le_bytes(buf[8..16].try_into().unwrap());
    let pan_y = f64::from_le_bytes(buf[16..24].try_into().unwrap());
    let zoom = f64::from_le_bytes(buf[24..32].try_into().unwrap());
    let space = buf[32];
    let layout_index = u16::from_le_bytes(buf[33..35].try_into().unwrap());
    let layer_count = u16::from_le_bytes(buf[35..37].try_into().unwrap());
    if layer_count > MAX_LAYERS {
        return Err(format!(
            "layer_count {layer_count} exceeds max {MAX_LAYERS}"
        ));
    }
    let nbytes = (layer_count as usize).div_ceil(8);
    if buf.len() < MIN_LEN + nbytes {
        return Err("layer bitmap truncated".to_string());
    }
    let bitmap = &buf[MIN_LEN..MIN_LEN + nbytes];
    let mut layer_visible = Vec::with_capacity(layer_count as usize);
    for i in 0..(layer_count as usize) {
        layer_visible.push((bitmap[i / 8] >> (i % 8)) & 1 == 1);
    }
    Ok(ViewerState {
        pan_x,
        pan_y,
        zoom,
        space,
        layout_index,
        layer_visible,
    })
}

impl Default for ViewerState {
    fn default() -> Self {
        Self::new()
    }
}

// ----- Hand-rolled base64url (no deps) -----

const B64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let bits = (b0 << 16) | (b1 << 8) | b2;
        let c0 = (bits >> 18) & 0x3F;
        let c1 = (bits >> 12) & 0x3F;
        let c2 = (bits >> 6) & 0x3F;
        let c3 = bits & 0x3F;
        out.push(B64_ALPHABET[c0 as usize] as char);
        out.push(B64_ALPHABET[c1 as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_ALPHABET[c2 as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(B64_ALPHABET[c3 as usize] as char);
        }
    }
    out
}

fn base64url_decode(input: &str) -> Result<Vec<u8>, String> {
    let mut table = [0i8; 256];
    for (i, &c) in B64_ALPHABET.iter().enumerate() {
        table[c as usize] = i as i8;
    }
    let mut valid = [false; 256];
    for &c in B64_ALPHABET.iter() {
        valid[c as usize] = true;
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut bits: u32 = 0;
    let mut acc: u32 = 0;
    for &b in bytes {
        if !valid[b as usize] {
            return Err(format!("invalid base64 char: 0x{b:02x}"));
        }
        acc = (acc << 6) | (table[b as usize] as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_roundtrip_empty() {
        let encoded = base64url_encode(&[]);
        let decoded = base64url_decode(&encoded).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn base64url_roundtrip_bytes() {
        let data: Vec<u8> = (0u8..=255).collect();
        let encoded = base64url_encode(&data);
        let decoded = base64url_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn viewer_state_default_is_model_space() {
        let s = ViewerState::new();
        assert_eq!(s.pan_x(), 0.0);
        assert_eq!(s.pan_y(), 0.0);
        assert_eq!(s.zoom(), 1.0);
        assert!(s.is_model_space());
        assert_eq!(s.layout_index(), 0);
    }

    #[test]
    fn viewer_state_serialize_deserialize_roundtrip() {
        // Tests bypass the wasm_bindgen layer to avoid JsValue in
        // non-wasm `cargo test` runs; the core fields are public to
        // this module for that reason.
        let mut s = ViewerState::new();
        s.pan_x = 123.456;
        s.pan_y = -78.9;
        s.zoom = 2.5;
        s.space = 1;
        s.layout_index = 3;
        // Direct field manipulation mirrors set_layer_visible's effect.
        while s.layer_visible.len() <= 9 {
            s.layer_visible.push(true);
        }
        s.layer_visible[0] = true;
        s.layer_visible[5] = false;
        s.layer_visible[9] = true;
        let encoded = s.to_string_js();
        let decoded = from_string_inner(&encoded).unwrap();
        assert_eq!(decoded.pan_x(), 123.456);
        assert_eq!(decoded.pan_y(), -78.9);
        assert_eq!(decoded.zoom(), 2.5);
        assert!(!decoded.is_model_space());
        assert_eq!(decoded.layout_index(), 3);
        assert_eq!(decoded.layer_visible.len(), 10);
        assert!(decoded.layer_visible[0]);
        assert!(!decoded.layer_visible[5]);
        assert!(decoded.layer_visible[9]);
    }

    #[test]
    fn viewer_state_rejects_invalid_blob() {
        // Use the inner (String-err) helper so this test runs in the
        // standard cargo test harness without a JS runtime.
        assert!(from_string_inner("").is_err());
        assert!(from_string_inner("BOGUS").is_err());
    }

    #[test]
    fn viewer_state_encoded_size_is_under_2000_chars_typical() {
        let mut s = ViewerState::new();
        // Direct manipulation — same as set_layer_visible sans the
        // JsValue boundary.
        while s.layer_visible.len() < 50 {
            s.layer_visible.push(true);
        }
        for i in 0..50 {
            s.layer_visible[i] = i % 3 == 0;
        }
        let encoded = s.to_string_js();
        assert!(
            encoded.len() < 2000,
            "typical viewer state must fit in a 2000-char URL; got {}",
            encoded.len()
        );
    }
}
