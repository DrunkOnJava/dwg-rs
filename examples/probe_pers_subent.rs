//! Does the derived ACDBPERSSUBENTMANAGER field list close on every corpus record, and do
//! its values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for the class; `src/objects/pers_subent.rs` was derived
//! from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md §7aq). For every record
//! of every drawing under the corpus directory this reports whether the decoder closed
//! exactly on the record's data-stream boundary (from R2007 with the string stream consumed)
//! and compares what it read with the object of the same handle (group 5) in the DXF twin:
//! the `BL` stream the decoder holds (the version, then each of the three arrays' count and
//! values) against the groups 90 of the object in file order, count and value by value. A
//! decode that read fewer or more values than the DXF holds groups 90 fails on the count.
//! R14 has no DXF twin in the corpus, so those records are checked against their boundary
//! alone.
//!
//! The DXF writes every number of the object as group 90, so it cannot say where an array
//! ends: three arrays after the version is the reading that closes all seven records, and is
//! unverified by value (see the note printed at the end).
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_pers_subent -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value or a count differs,
//! a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::PersSubentManager(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let mut stream = vec![i64::from(ours.version)];
    for array in &ours.arrays {
        stream.push(array.len() as i64);
        stream.extend(array.iter().map(|&v| i64::from(v)));
    }
    let theirs: Vec<Option<i64>> = rec
        .groups
        .iter()
        .filter(|g| g.code == 90)
        .map(twin::Group::int)
        .collect();
    t.int(
        "number of groups 90",
        stream.len() as i64,
        Some(theirs.len() as i64),
    );
    for (i, v) in stream.iter().enumerate() {
        t.int(
            &format!("group 90 #{i}"),
            *v,
            Some(theirs.get(i).copied().flatten().unwrap_or(-1)),
        );
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "ACDBPERSSUBENTMANAGER",
            class: Some("ACDBPERSSUBENTMANAGER"),
            kind: None,
            dxf_kind: "ACDBPERSSUBENTMANAGER",
            decodes_on: |_| true,
            note: "; unverified by value: where each array ends (the DXF writes every number as \
                   group 90, so a count then that many values, three times, is a reading that \
                   closes the records, not a measurement), the meaning of every value, the BL \
                   width (BS is bit-identical on every value of the corpus; the 32-bit form is \
                   never exercised), and any value but those the corpus holds; one drawing \
                   (ACadSharp's sample_base) supplies all seven records",
        },
        compare,
    )
}
