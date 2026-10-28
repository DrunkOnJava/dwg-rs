//! Does the derived WIPEOUTVARIABLES field list close on every corpus record, and do
//! its values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for WIPEOUTVARIABLES; `src/objects/small_classes.rs`
//! was derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md
//! §7bd). For every WIPEOUTVARIABLES of every drawing under the corpus directory this
//! reports whether the decoder closed exactly on the record's data-stream
//! boundary (from R2007 with the string stream consumed) and compares the display frame (group 70)
//! with the `WIPEOUTVARIABLES` whose handle (group 5) matches in the DXF twin. A group the
//! DXF leaves out is counted as omitted, not compared against an assumed
//! default.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_wipeout_variables -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::WipeoutVariables(ours) = decoded else {
        return false;
    };
    if let Some(rec) = rec {
        t.int(
            "display frame",
            i64::from(ours.display_frame),
            rec.first(70).and_then(Group::int),
        );
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "WIPEOUTVARIABLES",
            class: Some("WIPEOUTVARIABLES"),
            kind: None,
            dxf_kind: "WIPEOUTVARIABLES",
            decodes_on: |_| true,
            note: "",
        },
        compare,
    )
}
