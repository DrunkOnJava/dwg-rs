//! Does the §20.4.101.1 TABLESTYLE field list close on every corpus record
//! it covers, and do its values equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every `TABLESTYLE` record of every drawing under the corpus directory
//! this reports whether the crate's decoder closed exactly on the record's
//! data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus holds
//! AutoCAD's DXF export of the drawing, compares each decoded value with the
//! group of the `AcDbTableStyle` object whose handle (group 5) matches:
//!
//! - header: description (3), flow direction (70), flags (71), margins (40,
//!   41), suppress title (280, the second of the two 280s) and suppress
//!   header (281);
//! - each of the three row styles, which the DXF writes from its group 7
//!   on: text height (140), alignment (170), text and fill colours (62, 63,
//!   as the colour index a `TC` of method `0xC0` ByLayer, `0xC1` ByBlock or
//!   `0xC3` index stands for), background enabled (283), the six border
//!   line weights (274-279), visibilities (284-289) and colours (64-69),
//!   and from R2007 the data type (90), data unit type (91) and format
//!   string (1).
//!
//! The text style (group 7) is a handle in the record's handle stream, which
//! the decoder does not read, so it is not compared. A group the DXF leaves
//! out is counted as omitted, not compared against an assumed default. R14
//! has no twin in the corpus (nextgis exported it to R12 DXF, ACadSharp has
//! no AC1014 export), so its records are checked against their boundary
//! alone, and R2010 and later use §20.4.101.2's layout, which is not decoded
//! here and is listed rather than counted.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_tablestyle -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record in a decoded release errors, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use dwg::objects::acad_table_style::TableStyleRow;
use dwg::objects::color::ObjectColor;
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

fn first<'a>(groups: &'a [&Group], code: u16) -> Option<&'a Group> {
    groups.iter().copied().find(|g| g.code == code)
}

fn int_of(groups: &[&Group], code: u16) -> Option<i64> {
    first(groups, code).and_then(Group::int)
}

fn color(t: &mut Tally, what: &str, ours: &ObjectColor, theirs: Option<i64>) {
    let index = match ours.method() {
        0xC0 => Some(256),
        0xC1 => Some(0),
        0xC3 => Some(i64::from(ours.payload())),
        _ => None,
    };
    match (index, theirs) {
        (Some(i), _) => t.int(what, i, theirs),
        (None, Some(_)) => {
            t.compared += 1;
            t.differ += 1;
            println!(
                "    {what}: dwg colour 0x{:08X} has no DXF index form",
                ours.rgb
            );
        }
        (None, None) => t.omitted += 1,
    }
}

fn compare_row(t: &mut Tally, label: &str, ours: &TableStyleRow, dxf: &[&Group]) {
    let w = |what: &str| format!("{label} {what}");
    t.real(
        &w("text height"),
        ours.text_height,
        first(dxf, 140).and_then(Group::real),
    );
    t.int(
        &w("alignment"),
        i64::from(ours.text_alignment),
        int_of(dxf, 170),
    );
    color(t, &w("text colour"), &ours.text_color, int_of(dxf, 62));
    color(t, &w("fill colour"), &ours.fill_color, int_of(dxf, 63));
    t.int(
        &w("background"),
        i64::from(ours.background_enabled),
        int_of(dxf, 283),
    );
    for (i, b) in ours.borders.iter().enumerate() {
        let n = i as u16;
        t.int(
            &w(&format!("border {i} line weight")),
            i64::from(b.line_weight),
            int_of(dxf, 274 + n),
        );
        t.int(
            &w(&format!("border {i} visible")),
            i64::from(b.visible),
            int_of(dxf, 284 + n),
        );
        color(
            t,
            &w(&format!("border {i} colour")),
            &b.color,
            int_of(dxf, 64 + n),
        );
    }
    if let Some(v) = ours.data_type {
        t.int(&w("data type"), i64::from(v), int_of(dxf, 90));
    }
    if let Some(v) = ours.data_unit_type {
        t.int(&w("data unit type"), i64::from(v), int_of(dxf, 91));
    }
    if let Some(v) = &ours.format_string {
        t.text(
            &w("format string"),
            v,
            first(dxf, 1).map(|g| g.value.as_str()),
        );
    }
}

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::TableStyle(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let groups: Vec<&Group> = rec.groups.iter().collect();
    let start = groups
        .iter()
        .position(|g| g.code == 100 && g.value.trim() == "AcDbTableStyle")
        .map_or(0, |i| i + 1);
    let body = &groups[start..];
    let first_row = body.iter().position(|g| g.code == 7).unwrap_or(body.len());
    let head = &body[..first_row];
    t.text(
        "description",
        &ours.description,
        first(head, 3).map(|g| g.value.as_str()),
    );
    t.int(
        "flow direction",
        i64::from(ours.flow_direction),
        int_of(head, 70),
    );
    t.int("flags", i64::from(ours.bit_flags), int_of(head, 71));
    t.real(
        "horizontal margin",
        ours.horizontal_margin,
        first(head, 40).and_then(Group::real),
    );
    t.real(
        "vertical margin",
        ours.vertical_margin,
        first(head, 41).and_then(Group::real),
    );
    let suppress: Vec<i64> = head
        .iter()
        .filter(|g| g.code == 280)
        .filter_map(|g| g.int())
        .collect();
    t.int(
        "suppress title",
        i64::from(ours.suppress_title),
        suppress.last().copied(),
    );
    t.int(
        "suppress header",
        i64::from(ours.suppress_header),
        int_of(head, 281),
    );
    let mut starts: Vec<usize> = (first_row..body.len())
        .filter(|&i| body[i].code == 7)
        .collect();
    starts.push(body.len());
    if starts.len() != 4 {
        t.compared += 1;
        t.differ += 1;
        println!("    DXF holds {} row styles, not 3", starts.len() - 1);
        return true;
    }
    let rows = [
        ("data", &ours.data),
        ("title", &ours.title),
        ("header", &ours.header),
    ];
    for (k, (label, row)) in rows.iter().enumerate() {
        compare_row(t, label, row, &body[starts[k]..starts[k + 1]]);
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "TABLESTYLE",
            class: Some("TABLESTYLE"),
            kind: None,
            dxf_kind: "TABLESTYLE",
            decodes_on: |v| !v.is_r2010_plus(),
            note: "; the text style handles (group 7) are not decoded",
        },
        compare,
    )
}
