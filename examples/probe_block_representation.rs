//! Does the derived ACDB_BLOCKREPRESENTATION_DATA field list close on every corpus record, and do
//! its values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for ACDB_BLOCKREPRESENTATION_DATA; `src/objects/small_classes.rs`
//! was derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md
//! §7bf). For every ACDB_BLOCKREPRESENTATION_DATA of every drawing under the corpus directory this
//! reports whether the decoder closed exactly on the record's data-stream
//! boundary (from R2007 with the string stream consumed) and compares the flag (group 70); the hard handle (group 340) is in the handle stream and is not read
//! with the `ACDB_BLOCKREPRESENTATION_DATA` whose handle (group 5) matches in the DXF twin. A group the
//! DXF leaves out is counted as omitted, not compared against an assumed
//! default.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_block_representation -- ../dwg-corpus
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
    let DecodedEntity::BlockRepresentationData(ours) = decoded else {
        return false;
    };
    if let Some(rec) = rec {
        t.int(
            "flag",
            i64::from(ours.flag),
            rec.first(70).and_then(Group::int),
        );
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "ACDB_BLOCKREPRESENTATION_DATA",
            class: Some("ACDB_BLOCKREPRESENTATION_DATA"),
            kind: None,
            dxf_kind: "ACDB_BLOCKREPRESENTATION_DATA",
            decodes_on: |_| true,
            note: "; the hard handle (group 340) is in the handle stream and is not compared",
        },
        compare,
    )
}
