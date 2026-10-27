//! Does the derived ACDB_TEXTOBJECTCONTEXTDATA_CLASS field list close on every
//! corpus record, and do its values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for the class; `src/objects/text_context_data.rs`
//! was derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md §7bc).
//! For every record of every drawing under the corpus directory this reports whether
//! the decoder closed exactly on the record's data-stream boundary and compares each
//! decoded value with the object of the same handle (group 5) in the DXF twin: the
//! version (the first group 70), the default flag (290), the text flag (the second
//! group 70), the rotation (50) and the points (10, 20, 11, 21). The handle of the
//! annotation scale (340) is in the handle stream and is not compared. A drawing
//! with no DXF twin (one R2018 drawing here) is checked against its boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_text_context_data -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

fn nth(rec: &Record, code: u16, n: usize) -> Option<&Group> {
    rec.groups.iter().filter(|g| g.code == code).nth(n)
}

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::TextObjectContextData(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let int = |code: u16, n: usize| nth(rec, code, n).and_then(Group::int);
    let real = |code: u16| nth(rec, code, 0).and_then(Group::real);
    t.int("version (70)", i64::from(ours.dxf_70_version), int(70, 0));
    t.int("default flag (290)", i64::from(ours.dxf_290), int(290, 0));
    t.int("text flag (70)", i64::from(ours.dxf_70_flag), int(70, 1));
    t.real("rotation (50)", ours.dxf_50, real(50));
    t.real("group 10", ours.dxf_10, real(10));
    t.real("group 20", ours.dxf_20, real(20));
    t.real("group 11", ours.dxf_11, real(11));
    t.real("group 21", ours.dxf_21, real(21));
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "ACDB_TEXTOBJECTCONTEXTDATA_CLASS",
            class: Some("ACDB_TEXTOBJECTCONTEXTDATA_CLASS"),
            kind: None,
            dxf_kind: "ACDB_TEXTOBJECTCONTEXTDATA_CLASS",
            decodes_on: |v| v.is_r2010_plus(),
            note: "; unverified by value: the unit of the rotation (0 in both twins), the \
                   order of groups 11 and 21 (both 0), and any release before R2010",
        },
        compare,
    )
}
