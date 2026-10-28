//! Does the §20.4.102 CELLSTYLEMAP field list close on every corpus record,
//! and do its cell styles equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every `CELLSTYLEMAP` record of every drawing under the corpus
//! directory this reports whether the crate's decoder closed exactly on the
//! record's data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus
//! holds AutoCAD's DXF export of the drawing, compares every decoded value
//! with the groups of the `AcDbCellStyleMap` object, walking them in the
//! order §20.4.101.4 prints the fields so that a field in the wrong place
//! shows as the wrong group (`support/cellstyles.rs`): per cell style the
//! type, the data flag, the override and merge flags, the background colour,
//! the layout flags, the content format, the margins, the borders, then the
//! id, class and name. The text style and line type handles (groups 340) are
//! not decoded and are not compared.
//!
//! AutoCAD writes a CELLSTYLEMAP afresh on every save, so the DWG's and the
//! DXF's never share a handle; the DXF object paired with a DWG record is
//! the only one in the twin. R14 has no twin in the corpus, so its record is
//! checked against its boundary alone. The corpus holds no CELLSTYLEMAP from
//! R2010 on: those drawings carry the styles in their TABLESTYLE, which
//! `probe_tablestyle` compares.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_cellstylemap -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors, a value differs, a twin
//! has no single CELLSTYLEMAP to pair with, or no value was compared at all.

#[path = "support/cellstyles.rs"]
mod cellstyles;
#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Pairing, Probe, Record, Tally, Twin};

fn compare(
    decoded: &DecodedEntity,
    rec: Option<&Record>,
    _twin: Option<&Twin>,
    t: &mut Tally,
) -> bool {
    let DecodedEntity::CellStyleMap(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let from = rec
        .groups
        .iter()
        .position(|g| g.code == 100 && g.value.trim() == "AcDbCellStyleMap")
        .map_or(0, |i| i + 1);
    let mut s = cellstyles::Seq::new(&rec.groups[from..]);
    s.int(t, "cell style count", ours.cell_styles.len() as i64, 90);
    for style in &ours.cell_styles {
        cellstyles::named_cell_style(&mut s, t, style);
    }
    if s.leftover() > 0 {
        t.compared += 1;
        t.differ += 1;
        println!(
            "    the DXF holds {} groups after the decoded cell styles",
            s.leftover()
        );
    }
    true
}

fn main() -> ExitCode {
    twin::run_with_twin(
        &Probe {
            tag: "CELLSTYLEMAP",
            class: Some("CELLSTYLEMAP"),
            kind: None,
            dxf_kind: "CELLSTYLEMAP",
            decodes_on: |_| true,
            note: "; the text style and line type handles (groups 340) are not decoded",
        },
        Pairing::OnlyOfKind,
        compare,
    )
}
