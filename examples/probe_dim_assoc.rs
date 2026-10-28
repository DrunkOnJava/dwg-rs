//! Does the derived DIMASSOC field list close on every corpus record, and do
//! its values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for DIMASSOC; `src/objects/dim_assoc.rs` was
//! derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md
//! §7bi). For every DIMASSOC of every drawing under the corpus directory this
//! reports whether the decoder closed exactly on the record's data-stream
//! boundary (from R2007 with the string stream consumed) and compares each
//! decoded value with the `AcDbDimAssoc` whose handle (group 5) matches in the
//! DXF twin: the flags (90), the two head values (70, 71), and for each
//! reference its class name (1), osnap mode (72), groups 73, 91, 40, 10, 20 and
//! 75, in the DXF's order. The reference count is compared with the number of
//! group 72 the DXF holds. The handles (330, 331) are in the handle stream and
//! are not compared. R14 has no DXF twin in the corpus, so its records are
//! checked against their boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_dim_assoc -- ../dwg-corpus
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
    let DecodedEntity::DimAssoc(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let int = |code: u16, n: usize| nth(rec, code, n).and_then(Group::int);
    let real = |code: u16, n: usize| nth(rec, code, n).and_then(Group::real);
    t.int("flags (90)", i64::from(ours.dxf_90), int(90, 0));
    t.int("group 70", i64::from(ours.dxf_70_71_byte), int(70, 0));
    t.int("group 71", i64::from(ours.dxf_70_71_bit), int(71, 0));
    let theirs = rec.groups.iter().filter(|g| g.code == 72).count();
    t.int(
        "reference count",
        ours.references.len() as i64,
        Some(theirs as i64),
    );
    for (i, r) in ours.references.iter().enumerate() {
        t.text(
            "class (1)",
            "AcDbOsnapPointRef",
            nth(rec, 1, i).map(|g| g.value.as_str()),
        );
        t.int("osnap mode (72)", i64::from(r.dxf_72), int(72, i));
        t.int("group 73", i64::from(r.dxf_73), int(73, i));
        t.int("group 91", i64::from(r.dxf_91), int(91, i));
        t.real("group 40", r.dxf_40, real(40, i));
        t.real("group 10", r.dxf_10, real(10, i));
        t.real("group 20", r.dxf_20, real(20, i));
        t.int("group 75", i64::from(r.dxf_75), int(75, i));
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "DIMASSOC",
            class: Some("DIMASSOC"),
            kind: None,
            dxf_kind: "DIMASSOC",
            decodes_on: |_| true,
            note: "; unverified by value: the BS between groups 72 and 73, the BD before \
                   group 40, the trailing B, the order of groups 70 and 71, and of 10 and 20 \
                   where they are equal",
        },
        compare,
    )
}
