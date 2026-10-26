//! Comparing decoded cell styles with the groups AutoCAD's DXF export writes
//! for them in a CELLSTYLEMAP, shared by `probe_cellstylemap` and the R2010
//! half of `probe_tablestyle`.
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

use super::twin::{Group, Tally};
use dwg::objects::color::ObjectColor;
use dwg::objects::table_cell_style::{NamedCellStyle, TableCellStyle, TableContentFormat};

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
