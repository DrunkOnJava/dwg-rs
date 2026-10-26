//! Does the §20.4.97 TABLECONTENT field list close on every corpus record,
//! and do its values equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every `TABLECONTENT` record of every drawing under the corpus
//! directory this reports whether the crate's decoder closed exactly on the
//! record's data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus
//! holds AutoCAD's DXF export of the drawing, compares every decoded value
//! with the `AcDbTableContent` object whose handle (group 5) matches. The DXF
//! writes the object as the spec prints it, bracketed by marker strings
//! (`LINKEDTABLEDATACOLUMN_BEGIN`, `CELLCONTENT_BEGIN`, ...), and the probe
//! walks the groups in that order (`support/cellstyles.rs`), so that a field
//! in the wrong place shows as the wrong group: the name and description,
//! each column (name, custom data, cell style, id, width), each row and each
//! cell (state flags, tooltip, custom data, linked data, contents with their
//! values and format overrides, cell style, id, unknown flag), the row's
//! custom data, cell style, id and height, then the table's cell style and
//! merged ranges. The handles (groups 340) are not decoded and are passed
//! over. R14 has no twin in the corpus, so its records are checked against
//! their boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_tablecontent -- ../dwg-corpus
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
use dwg::objects::table_content::{
    TableCellContentKind, TableContent, TableCustomData, TableValue, TableValueData,
};
use std::process::ExitCode;
use twin::{Pairing, Probe, Record, Tally, Twin};

fn value(s: &mut Seq, t: &mut Tally, v: &TableValue) {
    if let Some(f) = v.flags {
        s.int(t, "value flags", i64::from(f), 93);
    }
    s.int(t, "value data type", i64::from(v.data_type), 90);
    match &v.data {
        TableValueData::Absent => {}
        TableValueData::Unknown(u) => s.int(t, "unknown value", i64::from(*u), 91),
        TableValueData::Long(l) => s.int(t, "long value", i64::from(*l), 91),
        TableValueData::Double(d) => s.real(t, "double value", *d, 140),
        TableValueData::String(x) => s.text(t, "string value", x, 1),
        TableValueData::Date(b) => {
            s.int(t, "date size", b.len() as i64, 92);
            let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
            s.text(t, "date bytes", &hex, 310);
        }
        TableValueData::Point3d(p) => {
            s.real(t, "point x", p[0], 11);
            s.real(t, "point y", p[1], 21);
            s.real(t, "point z", p[2], 31);
        }
        other => {
            t.compared += 1;
            t.differ += 1;
            println!("    the probe has no DXF mapping for {other:?}");
        }
    }
    if let Some(u) = v.unit_type {
        s.int(t, "unit type", i64::from(u), 94);
        s.text(
            t,
            "format string",
            v.format_string.as_deref().unwrap_or(""),
            300,
        );
        s.text(
            t,
            "value string",
            v.value_string.as_deref().unwrap_or(""),
            302,
        );
        s.marker(t, 304, "ACVALUE_END");
    }
}

fn custom_data(s: &mut Seq, t: &mut Tally, d: &TableCustomData) {
    s.int(t, "custom data", i64::from(d.flags), 91);
    s.marker(t, 301, "CUSTOMDATA");
    s.marker(t, 1, "DATAMAP_BEGIN");
    s.int(t, "custom data items", d.items.len() as i64, 90);
    for item in &d.items {
        s.text(t, "item name", &item.name, 300);
        s.marker(t, 301, "DATAMAP_VALUE");
        value(s, t, &item.value);
    }
    s.marker(t, 309, "DATAMAP_END");
}

fn compare_content(ours: &TableContent, rec: &Record, t: &mut Tally) {
    let from = rec
        .groups
        .iter()
        .position(|g| g.code == 100 && g.value.trim() == "AcDbLinkedData")
        .map_or(0, |i| i + 1);
    let mut s = Seq::new(&rec.groups[from..]);
    let s = &mut s;
    s.text(t, "name", &ours.name, 1);
    s.text(t, "description", &ours.description, 300);
    s.marker(t, 100, "AcDbLinkedTableData");
    s.int(t, "columns", ours.columns.len() as i64, 90);
    for col in &ours.columns {
        s.marker(t, 300, "COLUMN");
        s.marker(t, 1, "LINKEDTABLEDATACOLUMN_BEGIN");
        s.text(t, "column name", &col.name, 300);
        custom_data(s, t, &col.custom_data);
        s.marker(t, 309, "LINKEDTABLEDATACOLUMN_END");
        s.marker(t, 1, "FORMATTEDTABLEDATACOLUMN_BEGIN");
        s.marker(t, 300, "COLUMNTABLEFORMAT");
        cellstyles::cell_style(s, t, &col.cell_style);
        s.marker(t, 309, "FORMATTEDTABLEDATACOLUMN_END");
        s.marker(t, 1, "TABLECOLUMN_BEGIN");
        s.int(t, "column style id", i64::from(col.cell_style_id), 90);
        s.real(t, "column width", col.width, 40);
        s.marker(t, 309, "TABLECOLUMN_END");
    }
    s.int(t, "rows", ours.rows.len() as i64, 91);
    for row in &ours.rows {
        s.marker(t, 301, "ROW");
        s.marker(t, 1, "LINKEDTABLEDATAROW_BEGIN");
        s.int(t, "cells", row.cells.len() as i64, 90);
        for cell in &row.cells {
            s.marker(t, 300, "CELL");
            s.marker(t, 1, "LINKEDTABLEDATACELL_BEGIN");
            s.int(t, "state flags", i64::from(cell.state_flags), 90);
            s.text(t, "tooltip", &cell.tooltip, 300);
            custom_data(s, t, &cell.custom_data);
            s.int(t, "linked data", i64::from(cell.linked_data.is_some()), 92);
            if let Some(l) = &cell.linked_data {
                s.handle(t, "linked data handle", 340);
                s.int(t, "linked rows", i64::from(l.rows), 93);
                s.int(t, "linked columns", i64::from(l.columns), 94);
                s.int(t, "linked unknown", i64::from(l.unknown), 96);
            }
            s.int(t, "contents", cell.contents.len() as i64, 95);
            for content in &cell.contents {
                s.marker(t, 302, "CONTENT");
                s.marker(t, 1, "CELLCONTENT_BEGIN");
                s.int(t, "content type", i64::from(content.content_type), 90);
                match &content.kind {
                    TableCellContentKind::Value(v) => {
                        s.marker(t, 300, "VALUE");
                        value(s, t, v);
                    }
                    TableCellContentKind::Field | TableCellContentKind::Block => {
                        s.handle(t, "content handle", 340);
                    }
                    TableCellContentKind::Unknown => {}
                }
                s.int(t, "attributes", content.attributes.len() as i64, 91);
                s.marker(t, 309, "CELLCONTENT_END");
                s.marker(t, 1, "FORMATTEDCELLCONTENT_BEGIN");
                s.int(
                    t,
                    "format flag",
                    i64::from(content.format_override_flag),
                    170,
                );
                if let Some(f) = &content.format {
                    cellstyles::content_format(s, t, f);
                }
                s.marker(t, 309, "FORMATTEDCELLCONTENT_END");
            }
            s.marker(t, 309, "LINKEDTABLEDATACELL_END");
            s.marker(t, 1, "FORMATTEDTABLEDATACELL_BEGIN");
            s.marker(t, 300, "CELLTABLEFORMAT");
            cellstyles::cell_style(s, t, &cell.cell_style);
            s.marker(t, 309, "FORMATTEDTABLEDATACELL_END");
            s.marker(t, 1, "TABLECELL_BEGIN");
            s.int(t, "cell style id", i64::from(cell.cell_style_id), 90);
            s.int(t, "unknown flag", i64::from(cell.extra.is_some()), 91);
            if let Some(x) = &cell.extra {
                s.int(t, "extra unknown", i64::from(x.unknown_a), 91);
                s.real(t, "extra width", x.unknown_b, 40);
                s.real(t, "extra height", x.unknown_c, 41);
                s.handle(t, "extra handle", 330);
                s.int(t, "geometry flags", i64::from(x.geometry_flags), 92);
                if let Some(g) = &x.geometry {
                    for (k, (v, code)) in g
                        .distance_to_top_left
                        .iter()
                        .zip([10, 20, 30])
                        .chain(g.distance_to_center.iter().zip([11, 21, 31]))
                        .enumerate()
                    {
                        s.real(t, &format!("geometry distance {k}"), *v, code);
                    }
                    s.real(t, "geometry content width", g.content_width, 43);
                    s.real(t, "geometry content height", g.content_height, 44);
                    s.real(t, "geometry width", g.width, 45);
                    s.real(t, "geometry height", g.height, 46);
                    s.int(t, "geometry unknown", i64::from(g.unknown_flags), 95);
                }
            }
            s.marker(t, 309, "TABLECELL_END");
        }
        custom_data(s, t, &row.custom_data);
        s.marker(t, 309, "LINKEDTABLEDATAROW_END");
        s.marker(t, 1, "FORMATTEDTABLEDATAROW_BEGIN");
        s.marker(t, 300, "ROWTABLEFORMAT");
        cellstyles::cell_style(s, t, &row.cell_style);
        s.marker(t, 309, "FORMATTEDTABLEDATAROW_END");
        s.marker(t, 1, "TABLEROW_BEGIN");
        s.int(t, "row style id", i64::from(row.cell_style_id), 90);
        s.real(t, "row height", row.height, 40);
        s.marker(t, 309, "TABLEROW_END");
    }
    s.int(
        t,
        "field references",
        i64::from(ours.field_reference_count),
        92,
    );
    s.marker(t, 100, "AcDbFormattedTableData");
    s.marker(t, 300, "TABLEFORMAT");
    cellstyles::cell_style(s, t, &ours.cell_style);
    s.int(t, "merged ranges", ours.merged_ranges.len() as i64, 90);
    for r in &ours.merged_ranges {
        s.int(t, "top row", i64::from(r.top_row), 91);
        s.int(t, "left column", i64::from(r.left_column), 92);
        s.int(t, "bottom row", i64::from(r.bottom_row), 93);
        s.int(t, "right column", i64::from(r.right_column), 94);
    }
    s.marker(t, 100, "AcDbTableContent");
    s.handle(t, "table style", 340);
    if s.leftover() > 0 {
        t.compared += 1;
        t.differ += 1;
        println!(
            "    the DXF holds {} groups after the table style",
            s.leftover()
        );
    }
}

fn compare(
    decoded: &DecodedEntity,
    rec: Option<&Record>,
    _twin: Option<&Twin>,
    t: &mut Tally,
) -> bool {
    let DecodedEntity::TableContent(ours) = decoded else {
        return false;
    };
    if let Some(rec) = rec {
        compare_content(ours, rec, t);
    }
    true
}

fn main() -> ExitCode {
    twin::run_with_twin(
        &Probe {
            tag: "TABLECONTENT",
            class: Some("TABLECONTENT"),
            kind: None,
            dxf_kind: "TABLECONTENT",
            decodes_on: |_| true,
            note: "; the handles (groups 340) are not decoded",
        },
        Pairing::HandleThenOrder,
        compare,
    )
}
