//! Does the §20.4.82 IMAGEDEFREACTOR field list close on every corpus
//! record, and does its class version equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every `IMAGEDEF_REACTOR` record of every drawing under the corpus
//! directory this reports whether the crate's decoder closed exactly on the
//! record's data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus
//! holds AutoCAD's DXF export of the drawing, compares the class version
//! with group 90 of the `AcDbRasterImageDefReactor` object whose handle
//! (group 5) matches. The record's parent, the IMAGE entity it serves, is a
//! handle in the handle stream and is not compared. R14 has no twin in the
//! corpus, so its record is checked against its boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_imagedef_reactor -- ../dwg-corpus
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
    let DecodedEntity::ImageDefReactor(ours) = decoded else {
        return false;
    };
    if let Some(rec) = rec {
        t.int(
            "class version",
            i64::from(ours.class_version),
            rec.first(90).and_then(Group::int),
        );
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "IMAGEDEF_REACTOR",
            class: Some("IMAGEDEF_REACTOR"),
            kind: None,
            dxf_kind: "IMAGEDEF_REACTOR",
            decodes_on: |_| true,
            note: "",
        },
        compare,
    )
}
