//! The DXF `TABLES` section from the decoded symbol tables.
//!
//! [`build`] turns the decoded LAYER, STYLE, LTYPE and APPID records of a
//! drawing into the `TABLE` blocks AutoCAD's own DXF export writes for it, and
//! [`write`] emits them. Each record carries its handle and its owner (the
//! table's control object, from `AcDb:Header`), then the groups of its
//! `AcDb...TableRecord` subclass that the decoder surfaces, under the codes and
//! in the order of the exports of the public corpus.
//!
//! # What is written, and what is not
//!
//! - LAYER: name (2), flags (70: frozen 1, frozen in new viewports 2, locked 4,
//!   Xdep 16), colour (62, negative when the layer is off), plot flag (290, only
//!   when it is off: AutoCAD omits the default) and lineweight (370). The linetype
//!   name (6), plot-style handle (390) and, from R2013, the material and visual
//!   style handles (347, 348) are not written: the decoder does not read the
//!   handle stream of a layer. A layer whose colour is a true colour (the DWG
//!   stores no index for it) has no 62 written either.
//! - STYLE: name, flags (70: shape file 1, vertical 4, Xdep 16), 40, 41, 50
//!   (degrees), 71, 42, 3, 4.
//! - LTYPE: name, flags (70), description (3), alignment (72), dash count (73),
//!   pattern length (40) and each dash length (49) with its flag (74). A
//!   linetype with a shape or text dash is not written.
//! - APPID: name and flags (70). `ACAD_SPLINE_DATA` is left out: AutoCAD's export
//!   omits it on every corpus drawing that holds it.
//!
//! Records the export itself creates while saving (their handles are at or
//! above the DWG's `$HANDSEED`) are not in the DWG and not written. Every other
//! table (VPORT, VIEW, UCS, DIMSTYLE, BLOCK_RECORD) is not written yet.
//!
//! Targets before R2000 have no subclass markers and are not written: the
//! converter keeps its default layer for them.

use crate::dxf::DxfVersion;
use crate::dxf_sections::{HeaderGroup, write_groups};
use crate::entities::DecodedEntity;
use crate::header_vars::{HeaderVars, lineweight_hundredths};
use crate::object_type::ObjectType;
use crate::reader::DwgFile;
use crate::tables::TableEntryHeader;

/// One record of a table: its handle and the groups after the `5`/`330`/`100`
/// preamble (which [`write`] emits from the table's own data).
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// The record's handle.
    pub handle: u64,
    /// Groups from the record's `AcDb...TableRecord` marker on, in file order
    /// (the marker is the first).
    pub groups: Vec<HeaderGroup>,
    /// The record's name (group 2).
    pub name: String,
}

/// One `TABLE` block.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// `LAYER`, `STYLE`, `LTYPE` or `APPID`.
    pub name: &'static str,
    /// The table's control object handle.
    pub handle: u64,
    /// The records written.
    pub records: Vec<Record>,
}

/// The names the written tables define, for the header's references.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Defined {
    /// LAYER record names.
    pub layers: Vec<String>,
    /// STYLE record names.
    pub styles: Vec<String>,
    /// LTYPE record names.
    pub ltypes: Vec<String>,
}

/// The names `tables` define.
pub fn defined(tables: &[Table]) -> Defined {
    let names = |t: &str| {
        tables
            .iter()
            .find(|x| x.name == t)
            .map(|x| x.records.iter().map(|r| r.name.clone()).collect())
            .unwrap_or_default()
    };
    Defined {
        layers: names("LAYER"),
        styles: names("STYLE"),
        ltypes: names("LTYPE"),
    }
}

fn xdep(h: &TableEntryHeader) -> i64 {
    if h.is_xref_dependent { 16 } else { 0 }
}

fn layer_groups(l: &crate::tables::layer::Layer) -> Vec<HeaderGroup> {
    let mut flags = xdep(&l.header);
    if l.flags & 1 != 0 {
        flags |= 1;
    }
    if l.flags & 4 != 0 {
        flags |= 2;
    }
    if l.flags & 8 != 0 {
        flags |= 4;
    }
    let mut g = vec![
        HeaderGroup::Text(100, "AcDbLayerTableRecord".into()),
        HeaderGroup::Text(2, l.header.name.clone()),
        HeaderGroup::Int(70, flags),
    ];
    if l.color_index != 0 {
        let c = i64::from(l.color_index);
        g.push(HeaderGroup::Int(62, if l.flags & 2 != 0 { -c } else { c }));
    }
    if !l.plot_flag {
        g.push(HeaderGroup::Int(290, 0));
    }
    if let Some(w) = u8::try_from(l.lineweight)
        .ok()
        .and_then(lineweight_hundredths)
    {
        g.push(HeaderGroup::Int(370, i64::from(w)));
    }
    g
}

fn style_groups(s: &crate::tables::style::StyleEntry) -> Option<Vec<HeaderGroup>> {
    let mut flags = xdep(&s.header);
    if s.flags & 1 != 0 {
        flags |= 1;
    }
    if s.flags & 4 != 0 {
        flags |= 4;
    }
    let reals = [
        s.fixed_height,
        s.width_factor,
        s.oblique_angle,
        s.last_height,
    ];
    if reals.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(vec![
        HeaderGroup::Text(100, "AcDbTextStyleTableRecord".into()),
        HeaderGroup::Text(2, s.header.name.clone()),
        HeaderGroup::Int(70, flags),
        HeaderGroup::Real(40, s.fixed_height),
        HeaderGroup::Real(41, s.width_factor),
        HeaderGroup::Real(50, s.oblique_angle.to_degrees()),
        HeaderGroup::Int(71, i64::from(s.generation)),
        HeaderGroup::Real(42, s.last_height),
        HeaderGroup::Text(3, s.font_filename.clone()),
        HeaderGroup::Text(4, s.bigfont_filename.clone()),
    ])
}

fn ltype_groups(l: &crate::tables::ltype::LtypeEntry) -> Option<Vec<HeaderGroup>> {
    if l.dashes.iter().any(|d| d.shape_flag != 0) || !l.pattern_length.is_finite() {
        return None;
    }
    if l.dashes.iter().any(|d| !d.length.is_finite()) {
        return None;
    }
    let mut g = vec![
        HeaderGroup::Text(100, "AcDbLinetypeTableRecord".into()),
        HeaderGroup::Text(2, l.header.name.clone()),
        HeaderGroup::Int(70, xdep(&l.header)),
        HeaderGroup::Text(3, l.description.clone()),
        HeaderGroup::Int(72, i64::from(l.alignment)),
        HeaderGroup::Int(73, l.dashes.len() as i64),
        HeaderGroup::Real(40, l.pattern_length),
    ];
    for d in &l.dashes {
        g.push(HeaderGroup::Real(49, d.length));
        g.push(HeaderGroup::Int(74, 0));
    }
    Some(g)
}

fn appid_groups(a: &crate::tables::appid::AppId) -> Vec<HeaderGroup> {
    vec![
        HeaderGroup::Text(100, "AcDbRegAppTableRecord".into()),
        HeaderGroup::Text(2, a.header.name.clone()),
        HeaderGroup::Int(70, xdep(&a.header)),
    ]
}

/// The tables to write for `file` when the output targets `version`.
///
/// Empty for targets before R2000, for a file with no readable header or
/// object stream, and when no LAYER record decodes (the converter then keeps
/// its default layer).
pub fn build(file: &DwgFile, version: DxfVersion, header: &HeaderVars) -> Vec<Table> {
    if !version.supports_subclass_markers() || version == DxfVersion::R14 {
        return Vec::new();
    }
    let Some(Ok(objects)) = file.all_objects() else {
        return Vec::new();
    };
    let seed = header.get_handle("$HANDSEED").map_or(u64::MAX, |h| h.value);
    let control = |name: &str| header.get_handle(name).map(|h| h.value);
    let mut tables: Vec<Table> = [
        ("LTYPE", "LINETYPE_CONTROL_OBJECT"),
        ("LAYER", "LAYER_CONTROL_OBJECT"),
        ("STYLE", "STYLE_CONTROL_OBJECT"),
        ("APPID", "APPID_CONTROL_OBJECT"),
    ]
    .into_iter()
    .filter_map(|(name, ctl)| {
        Some(Table {
            name,
            handle: control(ctl)?,
            records: Vec::new(),
        })
    })
    .collect();
    for raw in &objects {
        let handle = raw.handle.value;
        if handle >= seed {
            continue;
        }
        let (table, name, groups) = match raw.kind {
            ObjectType::Layer | ObjectType::Style | ObjectType::Ltype | ObjectType::AppId => {
                match crate::entities::decode_from_raw(raw, file.version()) {
                    DecodedEntity::Layer(l) => {
                        ("LAYER", l.header.name.clone(), Some(layer_groups(&l)))
                    }
                    DecodedEntity::Style(s) => ("STYLE", s.header.name.clone(), style_groups(&s)),
                    DecodedEntity::Ltype(l) => ("LTYPE", l.header.name.clone(), ltype_groups(&l)),
                    // AutoCAD's export leaves this internal application out of the
                    // APPID table on all four drawings of the corpus that hold it.
                    DecodedEntity::AppId(a) if a.header.name == "ACAD_SPLINE_DATA" => continue,
                    DecodedEntity::AppId(a) => {
                        ("APPID", a.header.name.clone(), Some(appid_groups(&a)))
                    }
                    _ => continue,
                }
            }
            _ => continue,
        };
        let Some(groups) = groups else { continue };
        if let Some(t) = tables.iter_mut().find(|t| t.name == table) {
            t.records.push(Record {
                handle,
                groups,
                name,
            });
        }
    }
    if !tables
        .iter()
        .any(|t| t.name == "LAYER" && !t.records.is_empty())
    {
        return Vec::new();
    }
    tables.retain(|t| !t.records.is_empty());
    tables
}

/// Write `tables` as a `TABLES` section.
pub fn write(w: &mut crate::dxf::DxfWriter, tables: &[Table]) {
    w.begin_section("TABLES");
    for t in tables {
        write_groups(
            w,
            &[
                HeaderGroup::Text(0, "TABLE".into()),
                HeaderGroup::Text(2, t.name.into()),
                HeaderGroup::Handle(5, t.handle),
                HeaderGroup::Handle(330, 0),
                HeaderGroup::Text(100, "AcDbSymbolTable".into()),
                HeaderGroup::Int(70, t.records.len() as i64),
            ],
        );
        for r in &t.records {
            write_groups(
                w,
                &[
                    HeaderGroup::Text(0, t.name.into()),
                    HeaderGroup::Handle(5, r.handle),
                    HeaderGroup::Handle(330, t.handle),
                    HeaderGroup::Text(100, "AcDbSymbolTableRecord".into()),
                ],
            );
            write_groups(w, &r.groups);
        }
        w.write_string(0, "ENDTAB");
    }
    w.end_section();
}
