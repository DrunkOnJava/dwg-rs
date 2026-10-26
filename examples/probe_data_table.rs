//! Does the derived DATATABLE field list close on every corpus record, and do its
//! values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for DATATABLE; `src/objects/data_table.rs` was
//! derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md §7z).
//! For every DATATABLE of every drawing under the corpus directory this reports
//! whether the decoder closed exactly on the record's data-stream boundary and
//! compares each decoded value with the `DATATABLE` of the DXF twin: the version
//! (70), the column count (90), the row count (91), the table name (1) and, for each
//! column, its type (92), its name (2) and its cells (93 for type 1, 40 for type 2).
//! The cells of a handle column (type 6, group 360) are in the handle stream and are
//! not compared. AutoCAD gives the DWG and the DXF of one drawing different handles
//! for these objects, so a record is paired with the DXF object that no DWG record
//! shares a handle with, in file order. R14 has no DXF twin in the corpus, and
//! AutoCAD's R2000 export holds no DATATABLE, so those records are checked against
//! their boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_data_table -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use dwg::objects::data_table::DataTableCells;
use std::process::ExitCode;
use twin::{Group, Pairing, Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::DataTable(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let int = |code: u16| rec.first(code).and_then(Group::int);
    t.int("version (70)", i64::from(ours.dxf_70), int(70));
    t.int("columns (90)", ours.columns.len() as i64, int(90));
    t.int("rows (91)", i64::from(ours.rows), int(91));
    t.text(
        "table name (1)",
        &ours.dxf_1,
        rec.first(1).map(|g| g.value.trim()),
    );
    let starts: Vec<usize> = rec
        .groups
        .iter()
        .enumerate()
        .filter(|(_, g)| g.code == 92)
        .map(|(i, _)| i)
        .collect();
    t.int(
        "column type groups (92)",
        ours.columns.len() as i64,
        Some(starts.len() as i64),
    );
    for (i, col) in ours.columns.iter().enumerate() {
        let Some(&start) = starts.get(i) else {
            break;
        };
        let end = starts.get(i + 1).copied().unwrap_or(rec.groups.len());
        let segment = &rec.groups[start..end];
        t.int("column type (92)", i64::from(col.dxf_92), segment[0].int());
        t.text(
            "column name (2)",
            &col.dxf_2,
            segment.iter().find(|g| g.code == 2).map(|g| g.value.trim()),
        );
        match &col.cells {
            DataTableCells::Long(v) => {
                let theirs: Vec<&Group> = segment.iter().filter(|g| g.code == 93).collect();
                t.int("cells (93)", v.len() as i64, Some(theirs.len() as i64));
                for (a, b) in v.iter().zip(&theirs) {
                    t.int("cell (93)", i64::from(*a), b.int());
                }
            }
            DataTableCells::Double(v) => {
                let theirs: Vec<&Group> = segment.iter().filter(|g| g.code == 40).collect();
                t.int("cells (40)", v.len() as i64, Some(theirs.len() as i64));
                for (a, b) in v.iter().zip(&theirs) {
                    t.real("cell (40)", *a, b.real());
                }
            }
            DataTableCells::Handle => {}
        }
    }
    true
}

fn main() -> ExitCode {
    twin::run_with_twin(
        &Probe {
            tag: "DATATABLE",
            class: Some("DATATABLE"),
            kind: None,
            dxf_kind: "DATATABLE",
            decodes_on: |_| true,
            note: "; unverified by value: the cells of a handle column (group 360, in the \
                   handle stream) and every column type but 1, 2 and 6, which the corpus \
                   does not hold",
        },
        Pairing::HandleThenOrder,
        |d, r, _, t| compare(d, r, t),
    )
}
