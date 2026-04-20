//! V-14 — export buttons: SVG / DXF / glTF / PDF as Uint8Array.
//!
//! Surfaces the parent crate's converters through a wasm-bindgen
//! boundary so JS can trigger a download. Each method returns a
//! `Uint8Array`; the JS side wraps in a Blob + anchor click:
//!
//! ```js
//! const bytes = f.exportDxf("R2018");
//! const blob = new Blob([bytes], { type: "application/dxf" });
//! const url = URL.createObjectURL(blob);
//! const a = document.createElement("a");
//! a.href = url;
//! a.download = "drawing.dxf";
//! a.click();
//! URL.revokeObjectURL(url);
//! ```

use wasm_bindgen::prelude::*;

use crate::DwgFile;

#[wasm_bindgen]
impl DwgFile {
    /// Export as ASCII DXF at the target version (R12, R14, R2000,
    /// R2004, R2007, R2010, R2013, R2018). Returns the UTF-8 bytes
    /// of the DXF.
    ///
    /// Returns a JS Error if the version string is not recognized
    /// or if the parent crate's converter errors out.
    #[wasm_bindgen(js_name = "exportDxf")]
    pub fn export_dxf(&self, version_str: &str) -> Result<Vec<u8>, JsValue> {
        let version = parse_dxf_version(version_str)
            .ok_or_else(|| JsValue::from_str(&format!("unknown DXF version {version_str:?}")))?;
        let dxf = dwg::dxf_convert::convert_dwg_to_dxf(&self.inner, version)
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        Ok(dxf.into_bytes())
    }

    /// Export as a standalone SVG (the V-03 skeleton renderer).
    /// Returns UTF-8 bytes of the SVG document.
    #[wasm_bindgen(js_name = "exportSvg")]
    pub fn export_svg(&self) -> Vec<u8> {
        self.render_svg().into_bytes()
    }

    /// Export as glTF 2.0. `format` is `"gltf"` (JSON-only,
    /// external `.bin` separated) or `"glb"` (single binary).
    ///
    /// Today this returns an empty glTF skeleton — the parent
    /// crate's glTF converter takes a filesystem path
    /// (`convert_file_to_gltf`) which doesn't exist in the
    /// browser. A &DwgFile-based variant lands once the parent
    /// crate refactors that entry point.
    #[wasm_bindgen(js_name = "exportGltf")]
    pub fn export_gltf(&self, format: &str) -> Result<Vec<u8>, JsValue> {
        match format {
            "gltf" | "glb" => {}
            other => {
                return Err(JsValue::from_str(&format!(
                    "unknown glTF format {other:?}; expected 'gltf' or 'glb'"
                )));
            }
        }
        // Placeholder empty glTF 2.0 document.
        Ok(r#"{"asset":{"version":"2.0","generator":"dwg-rs-wasm"},"scenes":[{"nodes":[]}],"scene":0,"nodes":[],"meshes":[]}"#.as_bytes().to_vec())
    }

    /// Export as a "print-preview" SVG shaped for the browser's
    /// headless Save-As-PDF flow. Returns the same SVG bytes as
    /// `exportSvg` today; when V-13 print preview lands, this
    /// swaps to the page-size-aware flavor.
    #[wasm_bindgen(js_name = "exportPdf")]
    pub fn export_pdf(&self) -> Vec<u8> {
        self.export_svg()
    }
}

fn parse_dxf_version(s: &str) -> Option<dwg::dxf::DxfVersion> {
    dwg::dxf::DxfVersion::parse_cli(s)
}
