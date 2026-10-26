//! Does the §20.4.93 SORTENTSTABLE field list close on every corpus record,
//! and do its sort handles equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every `SORTENTSTABLE` record of every drawing under the corpus
//! directory this reports whether the crate's decoder closed exactly on the
//! record's data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus
//! holds AutoCAD's DXF export of the drawing, compares the number of entries
//! and each sort handle with the `AcDbSortentsTable` object whose handle
//! (group 5) matches. The DXF writes each entry as a group 331 (the entity,
//! a handle of the handle stream, which the decoder does not read) and then
//! the sort handle as a further group 5, after the record's own. R14 has no
//! twin in the corpus, so its records are checked against their boundary
//! alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_sortentstable -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors, a value differs, a twin
//! lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::SortEntsTable(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let theirs: Vec<Option<i64>> = rec
        .groups
        .iter()
        .filter(|g| g.code == 5)
        .skip(1)
        .map(|g| i64::from_str_radix(g.value.trim(), 16).ok())
        .collect();
    t.int(
        "entries",
        ours.sort_handles.len() as i64,
        Some(theirs.len() as i64),
    );
    for (i, h) in ours.sort_handles.iter().enumerate() {
        t.int(
            &format!("sort handle {i}"),
            *h as i64,
            theirs.get(i).copied().flatten(),
        );
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "SORTENTSTABLE",
            class: Some("SORTENTSTABLE"),
            kind: None,
            dxf_kind: "SORTENTSTABLE",
            decodes_on: |_| true,
            note: "; the entity handles (group 331) are not decoded",
        },
        compare,
    )
}
