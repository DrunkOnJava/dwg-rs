//! Does the §20.4.96 ACAD_TABLE field list close on every corpus record, and do
//! its values equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every `ACAD_TABLE` entity of every drawing under the corpus directory
//! this reports whether the crate's decoder closed exactly on the record's
//! data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus holds
//! AutoCAD's DXF export of the drawing, compares the decoded values with the
//! `AcDbTable` entity whose handle (group 5) matches. The DXF writes a table
//! in the older structures even for R2010 and later: the insertion point
//! (10, 20, 30), the horizontal direction (11, 21, 31), the row and column
//! counts (91, 92), the column widths (142) and row heights (141), and per
//! cell its value (`301 CELL_VALUE` to `304 ACVALUE_END`, a text value as a
//! group 1 after its data type, 90, of 4). Those are compared with the
//! insertion point, horizontal direction, rows, columns, widths, heights and
//! the first value of each cell's contents.
//!
//! From R2010 the entity also embeds the table content of §20.4.97, which
//! AutoCAD's DXF export still writes as a separate `AcDbTableContent` object
//! (referenced from the table's extension dictionary). The probe pairs the
//! n-th table of the DXF with its n-th `TABLECONTENT` and walks every value of
//! the embedded copy against it as `probe_tablecontent` does a standalone
//! object.
//!
//! R14 to R2007 use §20.4.96.1's layout, whose cells the DXF writes group by
//! group (171 to 176, 145, the text as group 1, and the property overrides as
//! the groups 170, 283, 63, 64, 140 and the border groups 69, 279, 289, 65, 275,
//! 285, 66, 276, 286, 68, 278, 288), so there the probe compares the table's
//! flags (90), override flags (93 to 96), counts, widths, heights and, cell by
//! cell, the type, edge flags, merge and autofit flags, merged width and
//! height, rotation, text (before R2007) and each override present on both
//! sides; from R2007 the value of each cell too, compared with the `301
//! CELL_VALUE` block: flags (93), data type (90), the data (1, 140, 91, 11 to
//! 31 or 310), unit type (94), format string (300) and value string (302). The
//! DXF writes a cell's override flags as group 91, whole. A group the
//! DXF leaves out is counted as omitted, not compared against an assumed
//! default. R14 has no twin in the corpus.
//!
//! The insertion rotation is stored in radians and the DXF writes group 50, an
//! angle group, in degrees, so it is compared as `to_degrees()`. A cell's
//! rotation (145) is a scalar double group, not an angle group, and is compared
//! as stored. No corpus table is rotated (every group 145 reads 0.0 and no
//! group 50 appears), so neither rotation is measured on a non-zero value.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_acad_table -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record in a decoded release errors, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/cellstyles.rs"]
mod cellstyles;
#[path = "support/twin.rs"]
mod twin;

use cellstyles::Seq;
use dwg::entities::DecodedEntity;
use dwg::entities::acad_table::{AcadTableLegacy, TableOverrideValue};
use dwg::objects::color::ObjectColor;
use dwg::objects::table_content::{TableCellContentKind, TableContent, TableValue, TableValueData};
use std::process::ExitCode;
use twin::{Group, Pairing, Probe, Record, Tally, Twin};

fn dxf_cell_texts(rec: &Record) -> Vec<Option<String>> {
    let mut out = Vec::new();
    let g = &rec.groups;
    let mut i = 0;
    while i < g.len() {
        if g[i].code == 301 && g[i].value.trim() == "CELL_VALUE" {
            let end = (i..g.len()).find(|&j| g[j].code == 304).unwrap_or(g.len());
            let body = &g[i..end];
            let is_text = body.iter().any(|x| x.code == 90 && x.int() == Some(4));
            out.push(if is_text {
                body.iter().find(|x| x.code == 1).map(|x| x.value.clone())
            } else {
                None
            });
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

fn color_index(c: &ObjectColor) -> i64 {
    if c.rgb == 0 {
        return i64::from(c.index);
    }
    match c.method() {
        0xC0 => 256,
        0xC1 => 0,
        _ => i64::from(c.payload()),
    }
}

const CELL_OVERRIDE_GROUPS: [(u32, u16); 18] = [
    (0x1, 170),
    (0x2, 283),
    (0x4, 63),
    (0x8, 64),
    (0x20, 140),
    (0x40, 69),
    (0x400, 279),
    (0x4000, 289),
    (0x80, 65),
    (0x800, 275),
    (0x8000, 285),
    (0x100, 66),
    (0x1000, 276),
    (0x10000, 286),
    (0x200, 68),
    (0x2000, 278),
    (0x20000, 288),
    (0x10, 7),
];

/// One cell's value against the `301 CELL_VALUE` to `304 ACVALUE_END` block
/// of the cell's groups: flags (93), data type (90), the data (1, 140, 91, 11
/// to 31 or 310), unit type (94), format string (300) and value string (302).
fn compare_value(cell: &[Group], v: &TableValue, t: &mut Tally, w: &dyn Fn(&str) -> String) {
    let from = cell.iter().position(|x| x.code == 301);
    let to = cell
        .iter()
        .position(|x| x.code == 304)
        .unwrap_or(cell.len());
    let vs = from.map_or(&cell[..0], |f| &cell[f..to]);
    let vf = |code: u16| vs.iter().find(|x| x.code == code);
    if let Some(f) = v.flags {
        t.int(&w("value flags"), i64::from(f), vf(93).and_then(Group::int));
    }
    t.int(
        &w("value data type"),
        i64::from(v.data_type),
        vf(90).and_then(Group::int),
    );
    match &v.data {
        TableValueData::Long(n) | TableValueData::Unknown(n) => {
            t.int(&w("value long"), i64::from(*n), vf(91).and_then(Group::int));
        }
        TableValueData::Double(d) => {
            t.real(&w("value double"), *d, vf(140).and_then(Group::real));
        }
        TableValueData::String(x) => {
            t.text(&w("value string data"), x, vf(1).map(|g| g.value.as_str()));
        }
        TableValueData::Point3d(p) => {
            t.real(&w("value point x"), p[0], vf(11).and_then(Group::real));
            t.real(&w("value point y"), p[1], vf(21).and_then(Group::real));
            t.real(&w("value point z"), p[2], vf(31).and_then(Group::real));
        }
        TableValueData::Date(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
            t.text(&w("value date"), &hex, vf(310).map(|g| g.value.as_str()));
        }
        _ => {}
    }
    if let Some(u) = v.unit_type {
        t.int(
            &w("value unit type"),
            i64::from(u),
            vf(94).and_then(Group::int),
        );
    }
    if let Some(x) = &v.format_string {
        t.text(&w("value format"), x, vf(300).map(|g| g.value.as_str()));
    }
    if let Some(x) = &v.value_string {
        t.text(&w("value string"), x, vf(302).map(|g| g.value.as_str()));
    }
}

fn compare_legacy(ours: &AcadTableLegacy, rec: &Record, t: &mut Tally) {
    let from = rec
        .groups
        .iter()
        .position(|g| g.code == 100 && g.value.trim() == "AcDbBlockReference")
        .map_or(0, |i| i + 1);
    let g = &rec.groups[from..];
    let first = |code: u16| g.iter().find(|x| x.code == code);
    let real = |code: u16| first(code).and_then(Group::real);
    let int = |code: u16| first(code).and_then(Group::int);
    let reals = |code: u16| -> Vec<f64> {
        g.iter()
            .filter(|x| x.code == code)
            .filter_map(Group::real)
            .collect()
    };
    let ins = &ours.insert;
    t.real("insertion x", ins.insertion_point.x, real(10));
    t.real("insertion y", ins.insertion_point.y, real(20));
    t.real("insertion z", ins.insertion_point.z, real(30));
    t.real("horizontal x", ours.horizontal_direction[0], real(11));
    t.real("horizontal y", ours.horizontal_direction[1], real(21));
    t.real("horizontal z", ours.horizontal_direction[2], real(31));
    t.int("table flags", i64::from(ours.table_flags), int(90));
    t.int("rows", ours.row_heights.len() as i64, int(91));
    t.int("columns", ours.column_widths.len() as i64, int(92));
    let flags = |o: &Option<dwg::entities::acad_table::TableOverrides>| {
        i64::from(o.as_ref().map_or(0, |o| o.flags))
    };
    t.int(
        "table override flags",
        flags(&ours.table_overrides),
        int(93),
    );
    t.int(
        "border colour flags",
        flags(&ours.border_color_overrides),
        int(94),
    );
    t.int(
        "border line weight flags",
        flags(&ours.border_line_weight_overrides),
        int(95),
    );
    t.int(
        "border visibility flags",
        flags(&ours.border_visibility_overrides),
        int(96),
    );
    let (widths, heights) = (reals(142), reals(141));
    for (i, w) in ours.column_widths.iter().enumerate() {
        t.real(&format!("column {i} width"), *w, widths.get(i).copied());
    }
    for (i, h) in ours.row_heights.iter().enumerate() {
        t.real(&format!("row {i} height"), *h, heights.get(i).copied());
    }
    let starts: Vec<usize> = (0..g.len()).filter(|&i| g[i].code == 171).collect();
    t.int("cells", ours.cells.len() as i64, Some(starts.len() as i64));
    for (i, cell) in ours.cells.iter().enumerate() {
        let Some(&a) = starts.get(i) else { break };
        let b = starts.get(i + 1).copied().unwrap_or(g.len());
        let s = &g[a..b];
        let sf = |code: u16| s.iter().find(|x| x.code == code);
        let w = |what: &str| format!("cell {i} {what}");
        t.int(
            &w("type"),
            i64::from(cell.cell_type),
            sf(171).and_then(Group::int),
        );
        t.int(
            &w("edge flags"),
            i64::from(cell.edge_flags),
            sf(172).and_then(Group::int),
        );
        t.int(
            &w("merged"),
            i64::from(cell.merged),
            sf(173).and_then(Group::int),
        );
        t.int(
            &w("autofit"),
            i64::from(cell.autofit),
            sf(174).and_then(Group::int),
        );
        t.int(
            &w("merged width"),
            i64::from(cell.merged_width),
            sf(175).and_then(Group::int),
        );
        t.int(
            &w("merged height"),
            i64::from(cell.merged_height),
            sf(176).and_then(Group::int),
        );
        t.real(&w("rotation"), cell.rotation, sf(145).and_then(Group::real));
        if let Some(text) = &cell.text {
            t.text(&w("text"), text, sf(1).map(|x| x.value.as_str()));
        }
        if let Some((_, v)) = &cell.value {
            compare_value(s, v, t, &w);
        }
        if let Some((virtual_edges, o)) = &cell.overrides {
            t.int(
                &w("override flags"),
                i64::from(o.flags),
                sf(91).and_then(Group::int),
            );
            t.int(
                &w("virtual edges"),
                i64::from(*virtual_edges),
                sf(178).and_then(Group::int),
            );
            for (bit, value) in &o.values {
                let Some(&(_, code)) = CELL_OVERRIDE_GROUPS.iter().find(|(b, _)| b == bit) else {
                    continue;
                };
                let theirs = sf(code);
                let name = w(&format!("override 0x{bit:X}"));
                match value {
                    TableOverrideValue::Bool(v) => {
                        t.int(&name, i64::from(*v), theirs.and_then(Group::int));
                    }
                    TableOverrideValue::Short(v) => {
                        t.int(&name, i64::from(*v), theirs.and_then(Group::int));
                    }
                    TableOverrideValue::Real(v) => {
                        t.real(&name, *v, theirs.and_then(Group::real));
                    }
                    TableOverrideValue::Color(c) => {
                        t.int(&name, color_index(c), theirs.and_then(Group::int));
                    }
                }
            }
        }
    }
}

/// The laid-out cell geometry the entity embeds against the DXF's
/// `TABLEGEOMETRY` object of the same table, cell by cell in row order.
fn compare_geometry(content: &TableContent, rec: &Record, t: &mut Tally) {
    let from = rec
        .groups
        .iter()
        .position(|g| g.code == 100 && g.value.trim() == "AcDbTableGeometry")
        .map_or(0, |i| i + 1);
    let mut s = Seq::new(&rec.groups[from..]);
    let cells: Vec<_> = content.rows.iter().flat_map(|r| &r.cells).collect();
    s.int(t, "geometry rows", content.rows.len() as i64, 90);
    s.int(t, "geometry columns", content.columns.len() as i64, 91);
    s.int(t, "geometry cells", cells.len() as i64, 92);
    for (i, cell) in cells.iter().enumerate() {
        let Some(x) = &cell.extra else {
            t.compared += 1;
            t.differ += 1;
            println!("    cell {i} has no embedded geometry");
            return;
        };
        let w = |what: &str| format!("cell {i} {what}");
        s.int(t, &w("geometry flags"), i64::from(x.unknown_a), 93);
        s.real(t, &w("width with gap"), x.unknown_b, 40);
        s.real(t, &w("height with gap"), x.unknown_c, 41);
        s.handle(t, &w("handle"), 330);
        s.int(
            t,
            &w("geometry contents"),
            i64::from(x.geometry.is_some()),
            94,
        );
        if let Some(g) = &x.geometry {
            for (k, (a, b)) in g.distance_to_top_left.iter().zip([10, 20, 30]).enumerate() {
                s.real(t, &w(&format!("top left {k}")), *a, b);
            }
            for (k, (a, b)) in g.distance_to_center.iter().zip([11, 21, 31]).enumerate() {
                s.real(t, &w(&format!("center {k}")), *a, b);
            }
            s.real(t, &w("content width"), g.content_width, 43);
            s.real(t, &w("content height"), g.content_height, 44);
            s.real(t, &w("width"), g.width, 45);
            s.real(t, &w("height"), g.height, 46);
            s.int(t, &w("geometry unknown"), i64::from(g.unknown_flags), 95);
        }
    }
    if s.leftover() > 0 {
        t.compared += 1;
        t.differ += 1;
        println!("    the DXF holds {} groups after the cells", s.leftover());
    }
}

fn compare(
    decoded: &DecodedEntity,
    rec: Option<&Record>,
    twin: Option<&Twin>,
    t: &mut Tally,
) -> bool {
    if let DecodedEntity::AcadTableLegacy(ours) = decoded {
        if let Some(rec) = rec {
            compare_legacy(ours, rec, t);
        }
        return true;
    }
    let DecodedEntity::AcadTable(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let from = rec
        .groups
        .iter()
        .position(|g| g.code == 100 && g.value.trim() == "AcDbBlockReference")
        .map_or(0, |i| i + 1);
    let g = &rec.groups[from..];
    let real = |code: u16| g.iter().find(|x| x.code == code).and_then(Group::real);
    let reals = |code: u16| -> Vec<f64> {
        g.iter()
            .filter(|x| x.code == code)
            .filter_map(Group::real)
            .collect()
    };
    let ins = &ours.insert;
    let block = &g[..g
        .iter()
        .position(|x| x.code == 100 && x.value.trim() == "AcDbTable")
        .unwrap_or(g.len())];
    let breal = |code: u16| block.iter().find(|x| x.code == code).and_then(Group::real);
    t.real("insertion x", ins.insertion_point.x, breal(10));
    t.real("insertion y", ins.insertion_point.y, breal(20));
    t.real("insertion z", ins.insertion_point.z, breal(30));
    for (what, v, code) in [
        ("scale x", ins.scale.x, 41),
        ("scale y", ins.scale.y, 42),
        ("scale z", ins.scale.z, 43),
        ("rotation", ins.rotation.to_degrees(), 50),
    ] {
        t.real(what, v, breal(code));
    }
    let dir = ours.horizontal_direction;
    t.real("horizontal x", dir[0], real(11));
    t.real("horizontal y", dir[1], real(21));
    t.real("horizontal z", dir[2], real(31));
    let int = |code: u16| g.iter().find(|x| x.code == code).and_then(Group::int);
    t.int("rows", ours.content.rows.len() as i64, int(91));
    t.int("columns", ours.content.columns.len() as i64, int(92));
    let (widths, heights) = (reals(142), reals(141));
    for (i, c) in ours.content.columns.iter().enumerate() {
        t.real(
            &format!("column {i} width"),
            c.width,
            widths.get(i).copied(),
        );
    }
    for (i, r) in ours.content.rows.iter().enumerate() {
        t.real(
            &format!("row {i} height"),
            r.height,
            heights.get(i).copied(),
        );
    }
    let round_trip = |what: &str| {
        twin.and_then(|tw| {
            let at = tw
                .records_of("ACAD_TABLE")
                .iter()
                .position(|r| r.handle == rec.handle)?;
            tw.records_of(what).get(at).copied()
        })
    };
    let (content, geometry) = (round_trip("TABLECONTENT"), round_trip("TABLEGEOMETRY"));
    let starts: Vec<usize> = (0..g.len()).filter(|&i| g[i].code == 171).collect();
    let cells: Vec<_> = ours.content.rows.iter().flat_map(|r| &r.cells).collect();
    t.int("cells", cells.len() as i64, Some(starts.len() as i64));
    for (i, cell) in cells.iter().enumerate() {
        let Some(&a) = starts.get(i) else { break };
        let b = starts.get(i + 1).copied().unwrap_or(g.len());
        let s = &g[a..b];
        let w = |what: &str| format!("cell {i} {what}");
        match cell.contents.first().map(|c| (&c.kind, &c.format)) {
            Some((TableCellContentKind::Value(v), _)) => compare_value(s, v, t, &w),
            Some((TableCellContentKind::Block, Some(f))) => {
                t.real(
                    &w("block scale"),
                    f.block_scale,
                    s.iter().find(|x| x.code == 144).and_then(Group::real),
                );
            }
            _ => {}
        }
    }
    if content.is_some() || geometry.is_some() {
        println!(
            "    against the round-trip TABLECONTENT and TABLEGEOMETRY objects of this table, \
             which are separate encodings and are not counted:"
        );
        let mut x = Tally::default();
        if let Some(c) = content {
            cellstyles::compare_content(&ours.content, c, &mut x, true);
        }
        if let Some(c) = geometry {
            compare_geometry(&ours.content, c, &mut x);
        }
        println!(
            "    round-trip objects: {} values equal, {} differ",
            x.compared - x.differ,
            x.differ
        );
    }
    let theirs = dxf_cell_texts(rec);
    let cells: Vec<_> = ours.content.rows.iter().flat_map(|r| &r.cells).collect();
    t.int("cell values", cells.len() as i64, Some(theirs.len() as i64));
    for (i, cell) in cells.iter().enumerate() {
        let mine = cell.contents.first().and_then(|c| match &c.kind {
            TableCellContentKind::Value(v) => match &v.data {
                TableValueData::String(s) => Some(s.as_str()),
                _ => None,
            },
            _ => None,
        });
        t.text(
            &format!("cell {i} text"),
            mine.unwrap_or(""),
            Some(theirs.get(i).and_then(|x| x.as_deref()).unwrap_or("")),
        );
    }
    true
}

fn main() -> ExitCode {
    twin::run_with_twin(
        &Probe {
            tag: "ACAD_TABLE",
            class: Some("ACAD_TABLE"),
            kind: None,
            dxf_kind: "ACAD_TABLE",
            decodes_on: |_| true,
            note: "; the handles and the table style are in the handle stream",
        },
        Pairing::HandleThenOrder,
        compare,
    )
}
