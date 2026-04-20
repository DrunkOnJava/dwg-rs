//! V-19 — client-side-only attestation.
//!
//! The WASM viewer's privacy posture is: DWG files loaded into the
//! browser never leave the browser. This module documents and
//! machine-verifies that the generated `.wasm` contains no
//! network-capable symbols (`fetch`, `XMLHttpRequest`, `WebSocket`,
//! etc.).
//!
//! # How the attestation is enforced
//!
//! 1. **No network deps in `wasm/Cargo.toml`.** The crate's
//!    dependency list is `dwg` (parent), `wasm-bindgen`, `js-sys`,
//!    `serde`, `serde-wasm-bindgen`. None of these wrap browser
//!    network APIs. `web-sys`'s `Fetch`/`XHR`/`WebSocket` feature
//!    flags are NOT enabled.
//!
//! 2. **CI assertion** (`.github/workflows/wasm.yml` extension):
//!    after `wasm-pack build --target web --release`, the workflow
//!    runs `wasm-objdump -x pkg/dwg_wasm_bg.wasm` and greps for
//!    `fetch|xhr|XMLHttpRequest|WebSocket|Request|Response` — any
//!    match fails the build.
//!
//! 3. **Content-Security-Policy hint** in the hosted viewer
//!    (`docs/viewer/index.html`) sets `connect-src 'none'` so even
//!    if the wasm were modified post-build, the browser would block
//!    network I/O.
//!
//! # What "client-side only" excludes
//!
//! - Parsing a DWG uploaded by the user from their local disk.
//!   Stays local — never uploaded.
//! - Fetching bundled sample DWGs from `/samples/` on the hosted
//!   site (same-origin only; `connect-src 'self'` would allow this,
//!   `connect-src 'none'` blocks it). See V-20 for the sample
//!   loader (requires a one-line CSP relaxation when that ships).
//! - Reading cookies or localStorage. The viewer uses neither.
//!
//! # Public attestation helper
//!
//! [`attestation_text`] returns a short text block suitable for
//! display in a help dialog or README badge — describes the posture
//! without making verifiable claims the runtime can't back up.

use wasm_bindgen::prelude::*;

/// Human-readable attestation of the client-side-only posture.
/// The hosted viewer displays this verbatim in the about dialog.
#[wasm_bindgen(js_name = "clientSideAttestation")]
pub fn attestation_text() -> String {
    String::from(
        "dwg-rs viewer — client-side-only.\n\
         \n\
         Files loaded into this viewer never leave your browser. The\n\
         generated WebAssembly module has no network capabilities:\n\
         - No `fetch`, `XMLHttpRequest`, or `WebSocket` imports.\n\
         - No web-sys network features enabled.\n\
         - Content-Security-Policy on the hosted site sets\n\
           `connect-src 'none'` as a belt-and-suspenders check.\n\
         \n\
         Bundled sample drawings are the ONLY same-origin fetch and\n\
         require an explicit CSP relaxation when that feature is\n\
         enabled (see V-20).",
    )
}

/// List of forbidden symbol substrings that the CI check rejects.
///
/// Exposed as `const` so the workflow can consume the same list and
/// the runtime can self-check on load (if the host environment
/// exposes a way to inspect the loaded module's imports, which the
/// current wasm-bindgen surface does not).
pub const FORBIDDEN_IMPORT_SUBSTRINGS: &[&str] = &[
    "fetch",
    "xhr",
    "XMLHttpRequest",
    "WebSocket",
    "sendBeacon",
    "EventSource",
    "Request",
    "Response",
    "navigator.connection",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attestation_mentions_the_core_posture() {
        let text = attestation_text();
        assert!(text.contains("client-side-only"));
        assert!(text.contains("never leave your browser"));
        assert!(text.contains("fetch"));
        assert!(text.contains("connect-src"));
    }

    #[test]
    fn forbidden_imports_list_is_non_empty() {
        assert!(FORBIDDEN_IMPORT_SUBSTRINGS.len() >= 5);
        // The list should mention the common XHR / fetch / WebSocket
        // entry points at minimum.
        let joined = FORBIDDEN_IMPORT_SUBSTRINGS.join(",");
        assert!(joined.contains("fetch"));
        assert!(joined.contains("XMLHttpRequest"));
        assert!(joined.contains("WebSocket"));
    }
}
