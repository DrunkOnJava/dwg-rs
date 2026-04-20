//! V-18 — drag-and-drop (JS-side helpers + Rust-side docs).
//!
//! HTML5 drag-and-drop is pure JS — this module exists to document
//! the expected pattern and to expose any Rust-side helpers the JS
//! glue might need. Today the JS helper is already demonstrated in
//! `docs/viewer/index.html`:
//!
//! ```js
//! dropzone.addEventListener('drop', async e => {
//!   e.preventDefault();
//!   const file = e.dataTransfer.files[0];
//!   if (!file) return;
//!   const bytes = new Uint8Array(await file.arrayBuffer());
//!   try {
//!     const dwg = DwgFile.open(bytes);
//!     // ...
//!   } catch (err) {
//!     console.error('DWG parse failed:', err);
//!   }
//! });
//! ```
//!
//! No Rust-side runtime surface: `DwgFile.open(bytes: &[u8])`
//! already accepts the bytes — there's nothing drag-and-drop
//! specific to add.

#![allow(dead_code)]

/// MIME types the drop zone should accept. Exposed as a `const`
/// so JS code can build `<input type=file accept="...">` from it.
pub const ACCEPTED_MIME_TYPES: &[&str] = &[
    "application/dwg",
    "application/acad",
    "application/x-acad",
    "application/x-autocad",
    "image/vnd.dwg",
];

/// File extensions the drop zone should accept.
pub const ACCEPTED_EXTENSIONS: &[&str] = &[".dwg", ".DWG"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_types_include_common_variants() {
        assert!(ACCEPTED_MIME_TYPES.contains(&"application/dwg"));
        assert!(ACCEPTED_MIME_TYPES.contains(&"image/vnd.dwg"));
    }

    #[test]
    fn extensions_case_variants() {
        assert!(ACCEPTED_EXTENSIONS.contains(&".dwg"));
        assert!(ACCEPTED_EXTENSIONS.contains(&".DWG"));
    }
}
