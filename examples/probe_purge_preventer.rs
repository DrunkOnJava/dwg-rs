//! Does the derived ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION field list close on every corpus record, and do
//! its values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION; `src/objects/small_classes.rs`
//! was derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md
//! §7bd). For every ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION of every drawing under the corpus directory this
//! reports whether the decoder closed exactly on the record's data-stream
//! boundary (from R2007 with the string stream consumed) and compares the version (group 70)
//! with the `ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION` whose handle (group 5) matches in the DXF twin. A group the
//! DXF leaves out is counted as omitted, not compared against an assumed
//! default.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_purge_preventer -- ../dwg-corpus
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
    let DecodedEntity::DynamicBlockPurgePreventer(ours) = decoded else {
        return false;
    };
    if let Some(rec) = rec {
        t.int(
            "version",
            i64::from(ours.version),
            rec.first(70).and_then(Group::int),
        );
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION",
            class: Some("ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION"),
            kind: None,
            dxf_kind: "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION",
            decodes_on: |_| true,
            note: "",
        },
        compare,
    )
}
