//! Does the §20.4.103 TABLEGEOMETRY field list close on every corpus record,
//! and do its values equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every `TABLEGEOMETRY` record of every drawing under the corpus
//! directory this reports whether the crate's decoder closed exactly on the
//! record's data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus
//! holds AutoCAD's DXF export of the drawing, compares every decoded value
//! with the `AcDbTableGeometry` object whose handle (group 5) matches (the
//! objects of the corpus's second table were created while saving and have
//! other handles in the DXF; they are paired in file order), walking its
//! groups in the order §20.4.103 prints the fields so that a field in the
//! wrong place shows as the wrong group: the row, column and
//! cell counts (90, 91, 92), then per cell the flags (93), widths and heights
//! with gap (40, 41), the content count (94) and per content the two
//! distances (10 to 31), the content width and height and the width and
//! height (43 to 46) and the unknown value (95). The cell's unknown handle
//! (group 330) is not decoded and is passed over. R14 has no twin in the
//! corpus, so its records are checked against their boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_tablegeometry -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors, a value differs, a twin
//! lacks the record, or no value was compared at all.

#[path = "support/cellstyles.rs"]
mod cellstyles;
#[path = "support/twin.rs"]
mod twin;

use cellstyles::Seq;
use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Pairing, Probe, Record, Tally, Twin};

fn compare(
    decoded: &DecodedEntity,
    rec: Option<&Record>,
    _twin: Option<&Twin>,
    t: &mut Tally,
) -> bool {
    let DecodedEntity::TableGeometry(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let from = rec
        .groups
        .iter()
        .position(|g| g.code == 100 && g.value.trim() == "AcDbTableGeometry")
        .map_or(0, |i| i + 1);
    let mut s = Seq::new(&rec.groups[from..]);
    s.int(t, "rows", i64::from(ours.rows), 90);
    s.int(t, "columns", i64::from(ours.columns), 91);
    s.int(t, "cells", ours.cells.len() as i64, 92);
    for (i, cell) in ours.cells.iter().enumerate() {
        s.int(t, &format!("cell {i} flags"), i64::from(cell.flags), 93);
        s.real(
            t,
            &format!("cell {i} width with gap"),
            cell.width_with_gap,
            40,
        );
        s.real(
            t,
            &format!("cell {i} height with gap"),
            cell.height_with_gap,
            41,
        );
        s.handle(t, &format!("cell {i} handle"), 330);
        s.int(
            t,
            &format!("cell {i} contents"),
            cell.contents.len() as i64,
            94,
        );
        for (j, c) in cell.contents.iter().enumerate() {
            let w = |what: &str| format!("cell {i} content {j} {what}");
            for (k, (a, b)) in c.distance_to_top_left.iter().zip([10, 20, 30]).enumerate() {
                s.real(t, &w(&format!("top left {k}")), *a, b);
            }
            for (k, (a, b)) in c.distance_to_center.iter().zip([11, 21, 31]).enumerate() {
                s.real(t, &w(&format!("center {k}")), *a, b);
            }
            s.real(t, &w("content width"), c.content_width, 43);
            s.real(t, &w("content height"), c.content_height, 44);
            s.real(t, &w("width"), c.width, 45);
            s.real(t, &w("height"), c.height, 46);
            s.int(t, &w("unknown"), i64::from(c.unknown), 95);
        }
    }
    if s.leftover() > 0 {
        t.compared += 1;
        t.differ += 1;
        println!(
            "    the DXF holds {} groups after the decoded cells",
            s.leftover()
        );
    }
    true
}

fn main() -> ExitCode {
    twin::run_with_twin(
        &Probe {
            tag: "TABLEGEOMETRY",
            class: Some("TABLEGEOMETRY"),
            kind: None,
            dxf_kind: "TABLEGEOMETRY",
            decodes_on: |_| true,
            note: "; the cells' unknown handles (groups 330) are not decoded",
        },
        Pairing::HandleThenOrder,
        compare,
    )
}
