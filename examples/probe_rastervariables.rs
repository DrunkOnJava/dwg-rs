//! Does the §20.4.91 RASTERVARIABLES field list close on every corpus
//! record, and do its values equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every `RASTERVARIABLES` record of every drawing under the corpus
//! directory this reports whether the crate's decoder closed exactly on the
//! record's data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus
//! holds AutoCAD's DXF export of the drawing, compares the class version, the
//! display frame, the display quality and the units with groups 90, 70, 71
//! and 72 of the `AcDbRasterVariables` object whose handle (group 5)
//! matches. R14 has no twin in the corpus, so its record is checked against
//! its boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_rastervariables -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors, a value differs, a twin
//! lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::RasterVariables(ours) = decoded else {
        return false;
    };
    if let Some(rec) = rec {
        let dxf = |code| rec.first(code).and_then(Group::int);
        t.int("class version", i64::from(ours.class_version), dxf(90));
        t.int("display frame", i64::from(ours.display_frame), dxf(70));
        t.int("display quality", i64::from(ours.display_quality), dxf(71));
        t.int("units", i64::from(ours.units), dxf(72));
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "RASTERVARIABLES",
            class: Some("RASTERVARIABLES"),
            kind: None,
            dxf_kind: "RASTERVARIABLES",
            decodes_on: |_| true,
            note: "",
        },
        compare,
    )
}
