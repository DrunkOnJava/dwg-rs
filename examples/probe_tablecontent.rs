//! Does the §20.4.97 TABLECONTENT field list close on every corpus record,
//! and do its values equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every `TABLECONTENT` record of every drawing under the corpus
//! directory this reports whether the crate's decoder closed exactly on the
//! record's data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus
//! holds AutoCAD's DXF export of the drawing, compares every decoded value
//! with the `AcDbTableContent` object whose handle (group 5) matches. The DXF
//! writes the object as the spec prints it, bracketed by marker strings
//! (`LINKEDTABLEDATACOLUMN_BEGIN`, `CELLCONTENT_BEGIN`, ...), and the probe
//! walks the groups in that order (`support/cellstyles.rs`), so that a field
//! in the wrong place shows as the wrong group: the name and description,
//! each column (name, custom data, cell style, id, width), each row and each
//! cell (state flags, tooltip, custom data, linked data, contents with their
//! values and format overrides, cell style, id, unknown flag), the row's
//! custom data, cell style, id and height, then the table's cell style and
//! merged ranges. The handles (groups 340) are not decoded and are passed
//! over. R14 has no twin in the corpus, so its records are checked against
//! their boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_tablecontent -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors, a value differs, a twin
//! lacks the record, or no value was compared at all.

#[path = "support/cellstyles.rs"]
mod cellstyles;
#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Pairing, Probe, Record, Tally, Twin};

fn compare(
    decoded: &DecodedEntity,
    rec: Option<&Record>,
    _twin: Option<&Twin>,
    t: &mut Tally,
) -> bool {
    let DecodedEntity::TableContent(ours) = decoded else {
        return false;
    };
    if let Some(rec) = rec {
        cellstyles::compare_content(ours, rec, t);
    }
    true
}

fn main() -> ExitCode {
    twin::run_with_twin(
        &Probe {
            tag: "TABLECONTENT",
            class: Some("TABLECONTENT"),
            kind: None,
            dxf_kind: "TABLECONTENT",
            decodes_on: |_| true,
            note: "; the handles (groups 340) are not decoded",
        },
        Pairing::HandleThenOrder,
        compare,
    )
}
