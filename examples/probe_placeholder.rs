//! Does ACDBPLACEHOLDER close on every corpus record, class-numbered or
//! fixed-code, and does its reactor count equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! The specification lists ACDBPLACEHOLDER with the fixed object types
//! (0x50, §5) and gives it no field list: it is the common object data and
//! nothing else (`src/objects/placeholder.rs`). R14 and R2000 files do not
//! write it under 0x50 but under a class number from `AcDb:Classes`, and the
//! dispatcher did not route that class, so those records were skipped. For
//! every placeholder of every drawing under the corpus directory, by either
//! route, this reports whether the decoder closed exactly on the record's
//! data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus holds
//! AutoCAD's DXF export of the drawing, compares the record's `BL` reactor
//! count with the number of reactors (group 330 inside the `{ACAD_REACTORS`
//! bracket) of the `ACDBPLACEHOLDER` object whose handle (group 5) matches.
//! R14 has no twin in the corpus, so its records are checked against their
//! boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_placeholder -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors, a value differs, a twin
//! lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::ObjectType;
use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::Placeholder(ours) = decoded else {
        return false;
    };
    if let Some(rec) = rec {
        t.int(
            "reactors",
            i64::from(ours.num_reactors),
            Some(rec.reactors as i64),
        );
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "ACDBPLACEHOLDER",
            class: Some("ACDBPLACEHOLDER"),
            kind: Some(ObjectType::AcDbPlaceholder),
            dxf_kind: "ACDBPLACEHOLDER",
            decodes_on: |_| true,
            note: "",
        },
        compare,
    )
}
