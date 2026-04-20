//! V-16 — WebWorker offloading (Rust side + JS glue docs).
//!
//! # Why offload DWG parsing to a WebWorker
//!
//! A 5-10 MB DWG with a full section-map walk + LZ77 decompression
//! takes hundreds of milliseconds on a mid-range laptop. Running that
//! on the main UI thread blocks rendering, stalls scroll handlers,
//! and trips Chrome's "unresponsive page" banner. A WebWorker pushes
//! the parse off the main thread — the UI stays interactive, and the
//! `message` event posts the parsed result back when ready.
//!
//! # Rust-side contract
//!
//! `DwgFile` is intentionally kept `Send + Sync`-compatible. The
//! parent crate's [`dwg::DwgFile`] owns its bytes in a `Vec<u8>`; it
//! holds no references, no raw pointers, no `Rc`/`Cell`/`RefCell`,
//! and no thread-local state. That means it can be moved across the
//! worker boundary without any `wasm-bindgen` `Send` shim.
//!
//! The wasm-bindgen wrapper type `crate::DwgFile` is NOT automatically
//! `Send` from a pure-Rust perspective — `wasm_bindgen` types are
//! opaque JS-owned handles in the generated glue. The *semantic*
//! thread-safety lives in how the glue ships messages across the
//! worker boundary: the worker runs a fresh wasm instance, parses
//! bytes received via `postMessage`, and posts back a serializable
//! snapshot (sections list, version, warnings) — not the `DwgFile`
//! handle itself.
//!
//! [`WorkerRequest`] / [`WorkerResponse`] below are the serializable
//! message shapes. The worker entry point parses a `WorkerRequest`,
//! opens the file, and posts back a `WorkerResponse`. No `DwgFile`
//! handle crosses the wire.
//!
//! # JS glue pattern
//!
//! ```javascript
//! // main.js — main thread
//! const worker = new Worker('./dwg-worker.js', { type: 'module' });
//!
//! worker.addEventListener('message', (e) => {
//!     const res = e.data; // WorkerResponse
//!     if (res.ok) {
//!         console.log('parsed', res.version, res.sections.length, 'sections');
//!         // rebuild a DwgFile in the main thread IF you need the
//!         // typed Rust handle (reparsing the bytes is fast now that
//!         // they're in a shared ArrayBuffer; or keep the summary and
//!         // defer reparse until someone scrolls to that layer).
//!     } else {
//!         console.error('parse failed', res.error);
//!     }
//! });
//!
//! // Send an uploaded File to the worker.
//! async function parseInWorker(file) {
//!     const bytes = new Uint8Array(await file.arrayBuffer());
//!     // `transfer` moves the underlying buffer — the main thread
//!     // loses access after postMessage, which is fine: the worker
//!     // will post back a snapshot, not the bytes.
//!     worker.postMessage({ kind: 'open', bytes }, [bytes.buffer]);
//! }
//! ```
//!
//! ```javascript
//! // dwg-worker.js — worker thread
//! import init, { DwgFile, workerHandle } from './pkg/dwg_wasm.js';
//!
//! await init();
//!
//! self.addEventListener('message', (e) => {
//!     const req = e.data;
//!     const response = workerHandle(req);
//!     self.postMessage(response);
//! });
//! ```
//!
//! # Memory safety
//!
//! WebWorkers are strictly isolated address spaces. No shared memory
//! between main + worker unless `SharedArrayBuffer` is explicitly
//! opted in (and COOP/COEP headers are set). This crate does NOT use
//! `SharedArrayBuffer` — the `Uint8Array` transferred to the worker is
//! moved, not shared, so the Rust side never sees concurrent mutation.
//!
//! # Build-time check
//!
//! The worker + main thread link against independent copies of the
//! wasm module. The `#[wasm_bindgen]` surface is stateless (no static
//! mut, no once-cell, no `thread_local!`), so there's no global state
//! that could diverge between instances.

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// A request posted by the main thread to a worker.
///
/// JS-side shape:
/// ```json
/// { "kind": "open", "bytes": Uint8Array }
/// ```
///
/// The `bytes` field is a `Uint8Array`; it arrives on the Rust side
/// as a `Vec<u8>` after `serde-wasm-bindgen` deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerRequest {
    /// Parse a DWG file. Rust returns a summary snapshot, not a
    /// `DwgFile` handle — handles don't survive the worker boundary.
    Open {
        /// Raw file bytes. Serialized as a plain `Vec<u8>` — the JS
        /// side passes a `Uint8Array` which `serde-wasm-bindgen`
        /// round-trips into a `Vec<u8>`.
        bytes: Vec<u8>,
    },
}

/// The snapshot a worker posts back to the main thread.
///
/// Only serializable fields — no `DwgFile` handle, no borrowed data.
/// The main thread rebuilds a `DwgFile` via a regular `DwgFile.open()`
/// call if it needs the typed handle (cheap once bytes are already in
/// memory). This snapshot is enough to render a "file loaded, N
/// sections, version R2018" banner and kick off UI population.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResponse {
    /// `true` when the parse succeeded; `false` when `error` is set.
    pub ok: bool,
    /// Human-readable version name (e.g. "R2018"). Empty on error.
    pub version: String,
    /// Version magic (e.g. "AC1032"). Empty on error.
    pub version_magic: String,
    /// Serialized section list — one entry per named section.
    pub sections: Vec<WorkerSectionView>,
    /// Section map status: `"Full" | "Fallback" | "Deferred"`.
    /// Empty on error.
    pub section_map_status: String,
    /// Error message when `ok == false`. Empty on success.
    pub error: String,
}

/// Serializable view of one section — the main thread never sees the
/// `SectionKind` enum directly because the worker boundary is
/// language-neutral JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSectionView {
    /// Section name, e.g. `"AcDb:Header"`.
    pub name: String,
    /// On-disk byte size.
    pub size: u64,
    /// Absolute byte offset into the file.
    pub offset: u64,
    /// `SectionKind` Debug string.
    pub kind: String,
}

/// WebWorker entry point — parse a [`WorkerRequest`], return a
/// [`WorkerResponse`].
///
/// JS glue:
///
/// ```javascript
/// // dwg-worker.js — runs inside a Worker
/// import init, { workerHandle } from './pkg/dwg_wasm.js';
/// await init();
/// self.addEventListener('message', (e) => {
///     const response = workerHandle(e.data);
///     self.postMessage(response);
/// });
/// ```
///
/// This function is pure: no global state is mutated, no side
/// effects happen outside the returned value. That makes it trivially
/// safe to run in a worker — the only shared resource with the main
/// thread is the `postMessage` channel, which is already single-ref
/// by construction.
#[wasm_bindgen(js_name = "workerHandle")]
pub fn worker_handle(request: JsValue) -> Result<JsValue, JsValue> {
    let req: WorkerRequest = serde_wasm_bindgen::from_value(request)
        .map_err(|e| JsValue::from_str(&format!("invalid worker request: {e}")))?;
    let response = match req {
        WorkerRequest::Open { bytes } => parse_to_response(bytes),
    };
    serde_wasm_bindgen::to_value(&response)
        .map_err(|e| JsValue::from_str(&format!("response serialize: {e}")))
}

/// Pure helper: parse bytes → build a [`WorkerResponse`].
///
/// Exposed as a crate-public function so the main-thread entry point
/// can share the same snapshot-construction path if it ever wants to
/// skip `wasm_bindgen` round-trips.
///
/// Calls the parent crate's [`dwg::DwgFile::from_bytes`] directly so
/// this path does NOT depend on the wasm-bindgen `DwgFile` wrapper —
/// making it callable from tests and non-wasm contexts.
pub fn parse_to_response(bytes: Vec<u8>) -> WorkerResponse {
    match dwg::DwgFile::from_bytes(bytes) {
        Ok(file) => {
            let version_name = format!("{}", file.version());
            let version_magic = String::from_utf8_lossy(&file.version().magic()).into_owned();
            let section_map_status = match file.section_map_status() {
                dwg::SectionMapStatus::Full => "Full".to_string(),
                dwg::SectionMapStatus::Fallback { .. } => "Fallback".to_string(),
                dwg::SectionMapStatus::Deferred { .. } => "Deferred".to_string(),
            };
            let sections = file
                .sections()
                .iter()
                .map(|s| WorkerSectionView {
                    name: s.name.clone(),
                    size: s.size,
                    offset: s.offset,
                    kind: format!("{:?}", s.kind),
                })
                .collect();
            WorkerResponse {
                ok: true,
                version: version_name,
                version_magic,
                sections,
                section_map_status,
                error: String::new(),
            }
        }
        Err(e) => WorkerResponse {
            ok: false,
            version: String::new(),
            version_magic: String::new(),
            sections: Vec::new(),
            section_map_status: String::new(),
            error: format!("{e}"),
        },
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn worker_response_default_shape_is_failure() {
        // `Default::default()` isn't derived; build by hand to make
        // sure the "ok=false + empty" shape is what callers see when
        // the worker hits an error path before setting any fields.
        let r = WorkerResponse {
            ok: false,
            version: String::new(),
            version_magic: String::new(),
            sections: Vec::new(),
            section_map_status: String::new(),
            error: "simulated".into(),
        };
        assert!(!r.ok);
        assert_eq!(r.error, "simulated");
        assert!(r.sections.is_empty());
    }

    #[test]
    fn parse_to_response_rejects_empty_bytes() {
        let resp = parse_to_response(Vec::new());
        assert!(!resp.ok);
        assert!(!resp.error.is_empty());
        assert!(resp.sections.is_empty());
    }
}
