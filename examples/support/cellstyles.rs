//! Comparing decoded cell styles with the groups AutoCAD's DXF export writes
//! for them in a CELLSTYLEMAP, shared by `probe_cellstylemap` and the R2010
//! half of `probe_tablestyle`, and the walk of a whole TABLECONTENT (a standalone
//! object in `probe_tablecontent`, a table's embedded copy in
//! `probe_acad_table`).
//!
//! The DXF writes a cell style as a sequence of groups in the order §20.4.101
//! prints its fields, bracketed by marker strings: `300 CELLSTYLE`, `1
//! TABLEFORMAT_BEGIN`, the style, `309 TABLEFORMAT_END`, `1 CELLSTYLE_BEGIN`,
//! the id, class and name, `309 CELLSTYLE_END`. This walks the groups in that
//! order and compares each value with the decoded field, so a field in the
//! wrong place shows as a group that is not the one expected, not only as a
//! wrong value. After the first such group nothing further in the record is
//! compared. A handle (the text style and the line type, groups 340) is not
//! decoded and is passed over.

#![allow(dead_code)]

use super::twin::{Group, Record, Tally};
use dwg::objects::color::ObjectColor;
use dwg::objects::table_cell_style::{NamedCellStyle, TableCellStyle, TableContentFormat};
use dwg::objects::table_content::{
    TableCellContentKind, TableContent, TableCustomData, TableValue, TableValueData,
};

/// The DXF colour index a `TC` stands for: 256 by layer, 0 by block, 257
/// none, or the index a method `0xC3` carries.
pub fn dxf_color_index(c: &ObjectColor) -> Option<i64> {
    match c.method() {
        0xC0 => Some(256),
        0xC1 => Some(0),
        0xC3 => Some(i64::from(c.payload())),
        0xC8 => Some(257),
        _ => None,
    }
}

pub struct Seq<'a> {
    groups: &'a [Group],
    at: usize,
    broken: bool,
}

impl<'a> Seq<'a> {
    pub fn new(groups: &'a [Group]) -> Self {
        Seq {
            groups,
            at: 0,
            broken: false,
        }
    }

    /// The next group, when it has `code`; otherwise the walk is broken here.
    fn take(&mut self, t: &mut Tally, what: &str, code: u16) -> Option<&'a Group> {
        if self.broken {
            return None;
        }
        match self.groups.get(self.at) {
            Some(g) if g.code == code => {
                self.at += 1;
                Some(g)
            }
            other => {
                self.broken = true;
                t.compared += 1;
                t.differ += 1;
                println!(
                    "    {what}: DXF has {} where group {code} was expected",
                    other.map_or("nothing".to_string(), |g| format!(
                        "group {} = {:?}",
                        g.code,
                        g.value.trim()
                    ))
                );
                None
            }
        }
    }

    pub fn marker(&mut self, t: &mut Tally, code: u16, text: &str) {
        if let Some(g) = self.take(t, text, code) {
            t.text(text, text, Some(g.value.trim()));
        }
    }

    pub fn int(&mut self, t: &mut Tally, what: &str, ours: i64, code: u16) {
        if let Some(g) = self.take(t, what, code) {
            t.int(what, ours, g.int());
        }
    }

    pub fn real(&mut self, t: &mut Tally, what: &str, ours: f64, code: u16) {
        if let Some(g) = self.take(t, what, code) {
            t.real(what, ours, g.real());
        }
    }

    pub fn text(&mut self, t: &mut Tally, what: &str, ours: &str, code: u16) {
        if let Some(g) = self.take(t, what, code) {
            t.text(what, ours, Some(g.value.as_str()));
        }
    }

    pub fn color(&mut self, t: &mut Tally, what: &str, ours: &ObjectColor) {
        if let Some(g) = self.take(t, what, 62) {
            match dxf_color_index(ours) {
                Some(i) => t.int(what, i, g.int()),
                None => {
                    t.compared += 1;
                    t.differ += 1;
                    println!(
                        "    {what}: dwg colour 0x{:08X} has no DXF index form",
                        ours.rgb
                    );
                }
            }
        }
    }

    /// A handle group the decoder does not read.
    pub fn handle(&mut self, t: &mut Tally, what: &str, code: u16) {
        if self.take(t, what, code).is_some() {
            t.omitted += 1;
        }
    }

    /// How many groups follow the last one compared; 0 once the walk broke.
    pub fn leftover(&self) -> usize {
        if self.broken {
            0
        } else {
            self.groups.len() - self.at
        }
    }
}

/// Compare a content format with the groups from its `300 CONTENTFORMAT` to
/// its `309 CONTENTFORMAT_END`.
pub fn content_format(s: &mut Seq, t: &mut Tally, f: &TableContentFormat) {
    s.marker(t, 300, "CONTENTFORMAT");
    s.marker(t, 1, "CONTENTFORMAT_BEGIN");
    s.int(
        t,
        "content override flags",
        i64::from(f.property_override_flags),
        90,
    );
    s.int(t, "content property flags", i64::from(f.property_flags), 91);
    s.int(t, "value data type", i64::from(f.value_data_type), 92);
    s.int(t, "value unit type", i64::from(f.value_unit_type), 93);
    s.text(t, "value format", &f.value_format, 300);
    s.real(t, "rotation", f.rotation, 40);
    s.real(t, "block scale", f.block_scale, 140);
    s.int(t, "cell alignment", i64::from(f.cell_alignment), 94);
    s.color(t, "content colour", &f.content_color);
    s.handle(t, "text style", 340);
    s.real(t, "text height", f.text_height, 144);
    s.marker(t, 309, "CONTENTFORMAT_END");
}

/// Compare one decoded cell style with the groups from its `1
/// TABLEFORMAT_BEGIN` to its `309 TABLEFORMAT_END`.
pub fn cell_style(s: &mut Seq, t: &mut Tally, style: &TableCellStyle) {
    s.marker(t, 1, "TABLEFORMAT_BEGIN");
    s.int(t, "cell style type", i64::from(style.cell_style_type), 90);
    s.int(t, "data flag", i64::from(style.data.is_some()), 170);
    if let Some(d) = &style.data {
        s.int(
            t,
            "override flags",
            i64::from(d.property_override_flags),
            91,
        );
        s.int(t, "merge flags", i64::from(d.merge_flags), 92);
        s.color(t, "background colour", &d.background_color);
        s.int(t, "layout flags", i64::from(d.content_layout_flags), 93);
        content_format(s, t, &d.content_format);
        s.int(t, "margin flags", i64::from(d.margin_override_flags), 171);
        if let Some(m) = &d.margins {
            s.marker(t, 301, "MARGIN");
            s.marker(t, 1, "CELLMARGIN_BEGIN");
            for (i, v) in m.iter().enumerate() {
                s.real(t, &format!("margin {i}"), *v, 40);
            }
            s.marker(t, 309, "CELLMARGIN_END");
        }
        s.int(t, "border count", d.borders.len() as i64, 94);
        for (i, b) in d.borders.iter().enumerate() {
            s.int(
                t,
                &format!("border {i} edge flags"),
                i64::from(b.edge_flags),
                95,
            );
            if let Some(p) = &b.properties {
                s.marker(t, 302, "GRIDFORMAT");
                s.marker(t, 1, "GRIDFORMAT_BEGIN");
                s.int(
                    t,
                    &format!("border {i} overrides"),
                    i64::from(p.override_flags),
                    90,
                );
                s.int(t, &format!("border {i} type"), i64::from(p.border_type), 91);
                s.color(t, &format!("border {i} colour"), &p.color);
                s.int(
                    t,
                    &format!("border {i} line weight"),
                    i64::from(p.line_weight),
                    92,
                );
                s.handle(t, &format!("border {i} line type"), 340);
                s.int(
                    t,
                    &format!("border {i} invisible"),
                    i64::from(p.invisible),
                    93,
                );
                s.real(t, &format!("border {i} spacing"), p.double_line_spacing, 40);
                s.marker(t, 309, "GRIDFORMAT_END");
            }
        }
    }
    s.marker(t, 309, "TABLEFORMAT_END");
}

/// Compare a named cell style with the groups of its entry, `300 CELLSTYLE`
/// to `309 CELLSTYLE_END`.
pub fn named_cell_style(s: &mut Seq, t: &mut Tally, n: &NamedCellStyle) {
    s.marker(t, 300, "CELLSTYLE");
    cell_style(s, t, &n.style);
    s.marker(t, 1, "CELLSTYLE_BEGIN");
    s.int(t, "cell style id", i64::from(n.id), 90);
    s.int(t, "cell style class", i64::from(n.class), 91);
    s.text(t, "cell style name", &n.name, 300);
    s.marker(t, 309, "CELLSTYLE_END");
}

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

/// Compare a decoded table content with the groups of the `AcDbTableContent`
/// object (or of a table's embedded copy) walked in the order §20.4.97 prints
/// the fields. The DXF's object is the table's round-trip data and holds no
/// laid-out cell geometry, which the entity's embedded copy does (`embedded`):
/// there the cell's geometry is passed over, not compared.
pub fn compare_content(ours: &TableContent, rec: &Record, t: &mut Tally, embedded: bool) {
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
        cell_style(s, t, &col.cell_style);
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
                for a in &content.attributes {
                    s.handle(t, "attribute handle", 330);
                    s.text(t, "attribute value", &a.value, 301);
                    s.int(t, "attribute index", i64::from(a.index), 92);
                }
                s.marker(t, 309, "CELLCONTENT_END");
                s.marker(t, 1, "FORMATTEDCELLCONTENT_BEGIN");
                s.int(
                    t,
                    "format flag",
                    i64::from(content.format_override_flag),
                    170,
                );
                if let Some(f) = &content.format {
                    content_format(s, t, f);
                }
                s.marker(t, 309, "FORMATTEDCELLCONTENT_END");
            }
            s.marker(t, 309, "LINKEDTABLEDATACELL_END");
            s.marker(t, 1, "FORMATTEDTABLEDATACELL_BEGIN");
            s.marker(t, 300, "CELLTABLEFORMAT");
            cell_style(s, t, &cell.cell_style);
            s.marker(t, 309, "FORMATTEDTABLEDATACELL_END");
            s.marker(t, 1, "TABLECELL_BEGIN");
            s.int(t, "cell style id", i64::from(cell.cell_style_id), 90);
            if embedded {
                s.handle(t, "unknown flag", 91);
            } else {
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
            }
            s.marker(t, 309, "TABLECELL_END");
        }
        custom_data(s, t, &row.custom_data);
        s.marker(t, 309, "LINKEDTABLEDATAROW_END");
        s.marker(t, 1, "FORMATTEDTABLEDATAROW_BEGIN");
        s.marker(t, 300, "ROWTABLEFORMAT");
        cell_style(s, t, &row.cell_style);
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
    cell_style(s, t, &ours.cell_style);
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
