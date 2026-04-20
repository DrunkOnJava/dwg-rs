//! V-05 — pan / zoom / fit-to-view viewer state.
//!
//! Stateful class that JS callers instantiate alongside a
//! [`DwgFile`] to track the viewport: pan offset, zoom factor, and
//! fit-to-view sizing. State-only today — actual rendering with the
//! applied transform is the render.rs concern, to be wired once
//! entity geometry lands.

use wasm_bindgen::prelude::*;

/// Viewport state machine. Opaque handle for JS callers — all
/// state lives in Rust, JS holds a pointer.
#[wasm_bindgen]
pub struct Viewer {
    pan_x: f64,
    pan_y: f64,
    zoom: f64,
    viewport_w: f64,
    viewport_h: f64,
}

#[wasm_bindgen]
impl Viewer {
    /// Fresh viewer at origin, zoom 1.0, 1024×768 viewport.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Viewer {
        Viewer {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
            viewport_w: 1024.0,
            viewport_h: 768.0,
        }
    }

    /// Translate the viewport by `(dx, dy)` in pixel units.
    #[wasm_bindgen(js_name = "panBy")]
    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        self.pan_x += dx;
        self.pan_y += dy;
    }

    /// Zoom about a focal point `(px, py)` in pixel coordinates.
    /// Zoom-in scales > 1.0, zoom-out < 1.0. Focal point stays
    /// fixed relative to the viewport so scroll-wheel zoom feels
    /// natural.
    #[wasm_bindgen(js_name = "zoomAt")]
    pub fn zoom_at(&mut self, px: f64, py: f64, scale: f64) {
        if scale <= 0.0 {
            return;
        }
        let before_x = (px - self.pan_x) / self.zoom;
        let before_y = (py - self.pan_y) / self.zoom;
        self.zoom *= scale;
        self.pan_x = px - before_x * self.zoom;
        self.pan_y = py - before_y * self.zoom;
    }

    /// Reset to origin + unit zoom. Convenient for a "Reset view"
    /// button.
    #[wasm_bindgen(js_name = "reset")]
    pub fn reset(&mut self) {
        self.pan_x = 0.0;
        self.pan_y = 0.0;
        self.zoom = 1.0;
    }

    /// Update the viewport size (browser window resize / container
    /// resize). Does not re-fit — callers explicitly invoke
    /// fitToView if they want that.
    #[wasm_bindgen(js_name = "setViewportSize")]
    pub fn set_viewport_size(&mut self, w: f64, h: f64) {
        self.viewport_w = w.max(1.0);
        self.viewport_h = h.max(1.0);
    }

    /// Fit a drawing-bounds rectangle to the viewport with a margin.
    /// `margin` defaults to 0.05 (5%). Centers the drawing in the
    /// viewport and sets zoom to the largest value where the
    /// drawing still fits both axes.
    #[wasm_bindgen(js_name = "fitToView")]
    pub fn fit_to_view(
        &mut self,
        drawing_min_x: f64,
        drawing_min_y: f64,
        drawing_max_x: f64,
        drawing_max_y: f64,
        margin: f64,
    ) {
        let dw = (drawing_max_x - drawing_min_x).max(1e-9);
        let dh = (drawing_max_y - drawing_min_y).max(1e-9);
        let m = margin.clamp(0.0, 0.45);
        let avail_w = self.viewport_w * (1.0 - 2.0 * m);
        let avail_h = self.viewport_h * (1.0 - 2.0 * m);
        let zx = avail_w / dw;
        let zy = avail_h / dh;
        self.zoom = zx.min(zy);
        // Center the drawing in the viewport.
        let dcx = (drawing_min_x + drawing_max_x) * 0.5;
        let dcy = (drawing_min_y + drawing_max_y) * 0.5;
        self.pan_x = self.viewport_w * 0.5 - dcx * self.zoom;
        self.pan_y = self.viewport_h * 0.5 - dcy * self.zoom;
    }

    /// Map a drawing-space point to viewport-space pixels with the
    /// current pan/zoom applied. JS-side renderers use this to
    /// layout overlays (selection handles, measurement labels).
    #[wasm_bindgen(js_name = "drawingToViewport")]
    pub fn drawing_to_viewport(&self, dx: f64, dy: f64) -> Box<[f64]> {
        Box::new([dx * self.zoom + self.pan_x, dy * self.zoom + self.pan_y])
    }

    /// Inverse of [`drawing_to_viewport`] — convert a pixel-space
    /// point (e.g. mouse click) back into drawing coordinates.
    #[wasm_bindgen(js_name = "viewportToDrawing")]
    pub fn viewport_to_drawing(&self, px: f64, py: f64) -> Box<[f64]> {
        if self.zoom == 0.0 {
            return Box::new([0.0, 0.0]);
        }
        Box::new([(px - self.pan_x) / self.zoom, (py - self.pan_y) / self.zoom])
    }

    #[wasm_bindgen(js_name = "panX")]
    pub fn pan_x(&self) -> f64 {
        self.pan_x
    }

    #[wasm_bindgen(js_name = "panY")]
    pub fn pan_y(&self) -> f64 {
        self.pan_y
    }

    #[wasm_bindgen(js_name = "zoomFactor")]
    pub fn zoom_factor(&self) -> f64 {
        self.zoom
    }
}

impl Default for Viewer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state() {
        let v = Viewer::new();
        assert_eq!(v.pan_x(), 0.0);
        assert_eq!(v.pan_y(), 0.0);
        assert_eq!(v.zoom_factor(), 1.0);
    }

    #[test]
    fn pan_accumulates() {
        let mut v = Viewer::new();
        v.pan_by(10.0, 20.0);
        v.pan_by(5.0, -3.0);
        assert_eq!(v.pan_x(), 15.0);
        assert_eq!(v.pan_y(), 17.0);
    }

    #[test]
    fn zoom_at_origin_scales_zoom() {
        let mut v = Viewer::new();
        v.zoom_at(0.0, 0.0, 2.0);
        assert_eq!(v.zoom_factor(), 2.0);
        // (0,0) in pixel space stays at (0,0) in drawing space.
        assert_eq!(v.pan_x(), 0.0);
        assert_eq!(v.pan_y(), 0.0);
    }

    #[test]
    fn zoom_at_nonzero_focal_preserves_that_point() {
        let mut v = Viewer::new();
        v.zoom_at(100.0, 100.0, 2.0);
        // (100, 100) in pixel space → before: (100/1, 100/1) = (100, 100)
        // in drawing. After zoom 2.0: (100, 100) in pixel should still
        // map to (100, 100) in drawing.
        let d = v.viewport_to_drawing(100.0, 100.0);
        assert!((d[0] - 100.0).abs() < 1e-9);
        assert!((d[1] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn reset_clears_pan_and_zoom() {
        let mut v = Viewer::new();
        v.pan_by(50.0, 50.0);
        v.zoom_at(10.0, 10.0, 3.0);
        v.reset();
        assert_eq!(v.pan_x(), 0.0);
        assert_eq!(v.zoom_factor(), 1.0);
    }

    #[test]
    fn fit_to_view_centers_and_scales() {
        let mut v = Viewer::new();
        v.set_viewport_size(1000.0, 1000.0);
        v.fit_to_view(0.0, 0.0, 500.0, 500.0, 0.0);
        // Drawing is 500×500 in a 1000×1000 viewport — zoom should
        // be 2.0 (no margin), and (250, 250) drawing center should
        // land at pixel (500, 500).
        assert!((v.zoom_factor() - 2.0).abs() < 1e-9);
        let center = v.drawing_to_viewport(250.0, 250.0);
        assert!((center[0] - 500.0).abs() < 1e-9);
        assert!((center[1] - 500.0).abs() < 1e-9);
    }

    #[test]
    fn drawing_viewport_roundtrip() {
        let mut v = Viewer::new();
        v.pan_by(42.0, -17.0);
        v.zoom_at(0.0, 0.0, 1.5);
        let before = (123.456, -78.9);
        let vp = v.drawing_to_viewport(before.0, before.1);
        let back = v.viewport_to_drawing(vp[0], vp[1]);
        assert!((back[0] - before.0).abs() < 1e-9);
        assert!((back[1] - before.1).abs() < 1e-9);
    }

    #[test]
    fn zoom_at_rejects_non_positive_scale() {
        let mut v = Viewer::new();
        v.zoom_at(0.0, 0.0, 0.0);
        assert_eq!(v.zoom_factor(), 1.0);
        v.zoom_at(0.0, 0.0, -1.0);
        assert_eq!(v.zoom_factor(), 1.0);
    }
}
