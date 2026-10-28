//! Does the derived PDFDEFINITION field list close on every corpus record, and do
//! its values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for PDFDEFINITION; `src/objects/small_classes.rs`
//! was derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md
//! §7bd). For every PDFDEFINITION of every drawing under the corpus directory this
//! reports whether the decoder closed exactly on the record's data-stream
//! boundary (from R2007 with the string stream consumed) and compares the file name (group 1) and the page (group 2)
//! with the `PDFDEFINITION` whose handle (group 5) matches in the DXF twin. A group the
//! DXF leaves out is counted as omitted, not compared against an assumed
//! default.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_pdf_definition -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::PdfDefinition(ours) = decoded else {
        return false;
    };
    if let Some(rec) = rec {
        t.text("file", &ours.file, rec.first(1).map(|g| g.value.as_str()));
        t.text("page", &ours.page, rec.first(2).map(|g| g.value.as_str()));
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "PDFDEFINITION",
            class: Some("PDFDEFINITION"),
            kind: None,
            dxf_kind: "PDFDEFINITION",
            decodes_on: |_| true,
            note: "",
        },
        compare,
    )
}
