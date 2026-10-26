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
//! - VPORT: name, flags (70: Xdep 16), the screen rectangle (10, 11), view
//!   centre (12), snap and grid (13 to 15, 50, 75 to 78), view direction and
//!   target (16, 17), height and aspect ratio (40, 41), lens, clipping, twist
//!   (42 to 44, 51), view mode (71), circle zoom and fast zoom (72, 73), UCS icon
//!   (74), render mode (281), the viewport's own UCS (65, 110 to 112, 79, 146)
//!   and, from R2007, the lighting and grid block (60, 61, 63, 141, 142, 282,
//!   292, 421).
//! - UCS: an empty table when the drawing has no UCS record, as AutoCAD writes
//!   it (every drawing of the public corpus). A drawing that has UCS records
//!   gets no UCS table: no corpus drawing holds one, so the UCS decoder has
//!   never read a real record, and §20.4.62 lists a second orthographic `BS` it
//!   does not read.
//! - BLOCK_RECORD: the name the record's `BLOCK` entity gives (anonymous blocks
//!   carry their number there), the layout handle (340), insert units,
//!   explodability and scaling (70, 280, 281; R2007) and the preview bitmap
//!   (310, 127 bytes to a group).
//!
//! Every record also carries, from its handle stream, its reactors and its
//! extension dictionary (`102 {ACAD_REACTORS`, `102 {ACAD_XDICTIONARY`). A
//! handle is written only when the output defines the object it names: the
//! OBJECTS section writes the dictionaries and layouts, so those are named; the
//! entities that react to a record or insert a block are not written with their
//! handles yet, so a record whose reactors or insert handles name one has no
//! such bracket.
//!
//! Records the export itself creates while saving (their handles are at or
//! above the DWG's `$HANDSEED`) are not in the DWG and not written. VIEW and
//! DIMSTYLE are not written yet.
//!
//! Targets before R2000 have no subclass markers and are not written: the
//! converter keeps its default layer for them.

use std::collections::{BTreeMap, HashSet};

use crate::dxf::DxfVersion;
use crate::dxf_sections::{HeaderGroup, write_groups};
use crate::entities::DecodedEntity;
use crate::header_vars::{HeaderVars, lineweight_hundredths};
use crate::object_type::ObjectType;
use crate::reader::DwgFile;
use crate::tables::TableEntryHeader;
use crate::tables::handles::EntryRefs;

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
    /// The persistent reactors written, in a `102 {ACAD_REACTORS` bracket.
    pub reactors: Vec<u64>,
    /// The extension dictionary written, in a `102 {ACAD_XDICTIONARY` bracket.
    pub xdictionary: Option<u64>,
}

/// One `TABLE` block.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// `VPORT`, `LTYPE`, `LAYER`, `STYLE`, `UCS` (empty), `APPID` or `BLOCK_RECORD`.
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
    /// BLOCK_RECORD names.
    pub blocks: Vec<String>,
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
        blocks: names("BLOCK_RECORD"),
    }
}

fn xdep(h: &TableEntryHeader) -> i64 {
    if h.is_xref_dependent { 16 } else { 0 }
}

/// A layer's handle references, from §20.4.54's list: the xref block, the
/// plot style, from R2007 the material, always the linetype and from R2013 a
/// visual style.
#[derive(Default)]
struct LayerRefs {
    /// The linetype's name, when the output writes that linetype.
    linetype: Option<String>,
    plotstyle: Option<u64>,
    material: Option<u64>,
    visual_style: Option<u64>,
}

fn layer_refs(
    refs: Option<&EntryRefs>,
    file_version: crate::version::Version,
    ltypes: &BTreeMap<u64, (String, bool)>,
) -> LayerRefs {
    let Some(r) = refs else {
        return LayerRefs::default();
    };
    let want = if matches!(
        file_version,
        crate::version::Version::R2013 | crate::version::Version::R2018
    ) {
        5
    } else if file_version.is_r2007_plus() {
        4
    } else {
        3
    };
    if r.rest.len() != want {
        return LayerRefs::default();
    }
    let (material, linetype) = if want >= 4 {
        (Some(r.rest[2]), r.rest[3])
    } else {
        (None, r.rest[2])
    };
    LayerRefs {
        linetype: ltypes
            .get(&linetype)
            .filter(|(_, written)| *written)
            .map(|(n, _)| n.clone()),
        plotstyle: Some(r.rest[1]),
        material,
        visual_style: (want == 5).then(|| r.rest[4]),
    }
}

/// `h` as group `code`: a NULL as 0, a handle the output defines as itself,
/// any other not at all.
fn handle_or_null(g: &mut Vec<HeaderGroup>, code: i32, h: Option<u64>, defined: &HashSet<u64>) {
    match h {
        Some(0) => g.push(HeaderGroup::Handle(code, 0)),
        Some(h) if defined.contains(&h) => g.push(HeaderGroup::Handle(code, h)),
        _ => {}
    }
}

fn layer_groups(
    l: &crate::tables::layer::Layer,
    refs: &LayerRefs,
    version: DxfVersion,
    defined: &HashSet<u64>,
) -> Vec<HeaderGroup> {
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
    if let Some(name) = &refs.linetype {
        g.push(HeaderGroup::Text(6, name.clone()));
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
    handle_or_null(&mut g, 390, refs.plotstyle, defined);
    if from_r2007(version) {
        handle_or_null(&mut g, 347, refs.material, defined);
    }
    if matches!(version, DxfVersion::R2013 | DxfVersion::R2018) {
        handle_or_null(&mut g, 348, refs.visual_style, defined);
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

fn finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn point2(g: &mut Vec<HeaderGroup>, code: i32, p: crate::entities::Point2D) {
    g.push(HeaderGroup::Real(code, p.x));
    g.push(HeaderGroup::Real(code + 10, p.y));
}

fn point3(g: &mut Vec<HeaderGroup>, code: i32, p: crate::entities::Point3D) {
    g.push(HeaderGroup::Real(code, p.x));
    g.push(HeaderGroup::Real(code + 10, p.y));
    g.push(HeaderGroup::Real(code + 20, p.z));
}

fn flag(on: bool) -> i64 {
    i64::from(on)
}

fn from_r2007(version: DxfVersion) -> bool {
    matches!(
        version,
        DxfVersion::R2007 | DxfVersion::R2010 | DxfVersion::R2013 | DxfVersion::R2018
    )
}

/// The handle `h` as a group when the output defines it.
fn handle_group(g: &mut Vec<HeaderGroup>, code: i32, h: Option<u64>, defined: &HashSet<u64>) {
    if let Some(h) = h.filter(|h| *h != 0 && defined.contains(h)) {
        g.push(HeaderGroup::Handle(code, h));
    }
}

fn vport_groups(
    v: &crate::tables::vport::VportEntry,
    refs: Option<&EntryRefs>,
    version: DxfVersion,
    defined: &HashSet<u64>,
) -> Option<Vec<HeaderGroup>> {
    let reals = [
        v.view_height,
        v.view_width,
        v.view_center.x,
        v.view_center.y,
        v.view_target.x,
        v.view_target.y,
        v.view_target.z,
        v.view_direction.x,
        v.view_direction.y,
        v.view_direction.z,
        v.view_twist,
        v.lens_length,
        v.front_clip,
        v.back_clip,
        v.lower_left.x,
        v.lower_left.y,
        v.upper_right.x,
        v.upper_right.y,
        v.snap_base.x,
        v.snap_base.y,
        v.snap_spacing.x,
        v.snap_spacing.y,
        v.grid_spacing.x,
        v.grid_spacing.y,
        v.snap_rotation,
    ];
    if !finite(&reals) {
        return None;
    }
    let mut g = vec![
        HeaderGroup::Text(100, "AcDbViewportTableRecord".into()),
        HeaderGroup::Text(2, v.header.name.clone()),
        HeaderGroup::Int(70, xdep(&v.header)),
    ];
    point2(&mut g, 10, v.lower_left);
    point2(&mut g, 11, v.upper_right);
    point2(&mut g, 12, v.view_center);
    point2(&mut g, 13, v.snap_base);
    point2(&mut g, 14, v.snap_spacing);
    point2(&mut g, 15, v.grid_spacing);
    point3(&mut g, 16, v.view_direction);
    point3(&mut g, 17, v.view_target);
    g.push(HeaderGroup::Real(40, v.view_height));
    if let Some(aspect) = v.aspect_ratio().filter(|a| a.is_finite()) {
        g.push(HeaderGroup::Real(41, aspect));
    }
    g.push(HeaderGroup::Real(42, v.lens_length));
    g.push(HeaderGroup::Real(43, v.front_clip));
    g.push(HeaderGroup::Real(44, v.back_clip));
    g.push(HeaderGroup::Real(50, v.snap_rotation.to_degrees()));
    g.push(HeaderGroup::Real(51, v.view_twist.to_degrees()));
    g.push(HeaderGroup::Int(71, i64::from(v.view_mode)));
    g.push(HeaderGroup::Int(72, i64::from(v.circle_zoom)));
    g.push(HeaderGroup::Int(73, flag(v.fast_zoom)));
    g.push(HeaderGroup::Int(74, i64::from(v.ucs_icon)));
    g.push(HeaderGroup::Int(75, flag(v.snap_on)));
    g.push(HeaderGroup::Int(76, flag(v.grid_on)));
    g.push(HeaderGroup::Int(77, flag(v.snap_style)));
    g.push(HeaderGroup::Int(78, i64::from(v.snap_isopair)));
    if let Some(ucs) = &v.ucs {
        let ucs_reals = [
            ucs.origin.x,
            ucs.origin.y,
            ucs.origin.z,
            ucs.x_direction.x,
            ucs.x_direction.y,
            ucs.x_direction.z,
            ucs.y_direction.x,
            ucs.y_direction.y,
            ucs.y_direction.z,
            ucs.elevation,
        ];
        if !finite(&ucs_reals) {
            return None;
        }
        g.push(HeaderGroup::Int(281, i64::from(v.render_mode)));
        g.push(HeaderGroup::Int(65, flag(v.ucs_per_vport)));
        point3(&mut g, 110, ucs.origin);
        point3(&mut g, 111, ucs.x_direction);
        point3(&mut g, 112, ucs.y_direction);
        g.push(HeaderGroup::Int(79, i64::from(ucs.ortho_view_type)));
        g.push(HeaderGroup::Real(146, ucs.elevation));
    }
    if from_r2007(version) {
        if let Some(l) = &v.lighting {
            if !finite(&[l.brightness, l.contrast]) {
                return None;
            }
            // VPORT: Vport control, reactors, xdictionary, xref block, then
            // background (332), visual style (348), sun (361), named UCS (345)
            // and base UCS (346).
            let at = |i: usize| refs.and_then(|r| r.rest.get(i).copied());
            handle_group(&mut g, 348, at(2), defined);
            g.push(HeaderGroup::Int(60, i64::from(l.grid_flags)));
            g.push(HeaderGroup::Int(61, i64::from(l.grid_major)));
            g.push(HeaderGroup::Int(292, flag(l.use_default_lights)));
            g.push(HeaderGroup::Int(282, i64::from(l.default_lighting_type)));
            g.push(HeaderGroup::Real(141, l.brightness));
            g.push(HeaderGroup::Real(142, l.contrast));
            match l.ambient_color_rgb >> 24 {
                0xC2 => g.push(HeaderGroup::Int(
                    421,
                    i64::from(l.ambient_color_rgb & 0xFF_FFFF),
                )),
                0xC3 => g.push(HeaderGroup::Int(63, i64::from(l.ambient_color_rgb & 0xFF))),
                0xC0 => g.push(HeaderGroup::Int(63, 256)),
                0xC1 => g.push(HeaderGroup::Int(63, 0)),
                _ => g.push(HeaderGroup::Int(63, i64::from(l.ambient_color_index))),
            }
            handle_group(&mut g, 332, at(1), defined);
            handle_group(&mut g, 361, at(3), defined);
        }
    }
    Some(g)
}

/// The groups of a BLOCK_RECORD for the block `name`.
///
/// `refs` holds the record's handle stream. Its entry-specific handles are, in
/// §20.4.52's order, a NULL, the `BLOCK` entity, on R2000 the first and last
/// entity (unless the block is an xref), on R2004 and later the owned entities,
/// the `ENDBLK` entity, the insert handles and the layout. A list that does not
/// have the length that order implies is not read.
fn block_record_groups(
    b: &crate::tables::block_record::BlockRecord,
    name: &str,
    refs: Option<&EntryRefs>,
    file_version: crate::version::Version,
    version: DxfVersion,
    defined: &HashSet<u64>,
) -> Vec<HeaderGroup> {
    let mut g = vec![
        HeaderGroup::Text(100, "AcDbBlockTableRecord".into()),
        HeaderGroup::Text(2, name.to_string()),
    ];
    let expected = refs.map(|_| {
        let mut n = 2;
        if matches!(file_version, crate::version::Version::R2000) && !b.is_xref && !b.xref_overlay {
            n += 2;
        }
        if file_version.is_r2004_plus() {
            n += b.num_owned_objects.unwrap_or(0) as usize;
        }
        n + 1 + b.insert_count + 1
    });
    if let (Some(r), Some(n)) = (refs, expected) {
        if r.rest.len() == n {
            let layout = r.rest[r.rest.len() - 1];
            if layout == 0 {
                g.push(HeaderGroup::Handle(340, 0));
            } else if defined.contains(&layout) {
                g.push(HeaderGroup::Handle(340, layout));
            }
        }
    }
    if from_r2007(version) {
        if let (Some(units), Some(explodable), Some(scaling)) =
            (b.insert_units, b.explodable, b.block_scaling)
        {
            g.push(HeaderGroup::Int(70, i64::from(units)));
            g.push(HeaderGroup::Int(280, flag(explodable)));
            g.push(HeaderGroup::Int(281, i64::from(scaling)));
        }
    }
    for chunk in b.preview.chunks(127) {
        let hex: String = chunk.iter().map(|x| format!("{x:02X}")).collect();
        g.push(HeaderGroup::Text(310, hex));
    }
    g
}

/// The tables to write for `file` when the output targets `version`.
///
/// `objects` holds the handles the OBJECTS section writes: a handle group that
/// names one of them is written, any other is left out.
///
/// Empty for targets before R2000, for a file with no readable header or
/// object stream, and when no LAYER record decodes (the converter then keeps
/// its default layer).
pub fn build(
    file: &DwgFile,
    version: DxfVersion,
    header: &HeaderVars,
    objects: &HashSet<u64>,
) -> Vec<Table> {
    if !version.supports_subclass_markers() || version == DxfVersion::R14 {
        return Vec::new();
    }
    let Some(Ok(raws)) = file.all_objects() else {
        return Vec::new();
    };
    let file_version = file.version();
    let seed = header.get_handle("$HANDSEED").map_or(u64::MAX, |h| h.value);
    let control = |name: &str| header.get_handle(name).map(|h| h.value);
    let mut tables: Vec<Table> = [
        ("VPORT", "VPORT_CONTROL_OBJECT"),
        ("LTYPE", "LINETYPE_CONTROL_OBJECT"),
        ("LAYER", "LAYER_CONTROL_OBJECT"),
        ("STYLE", "STYLE_CONTROL_OBJECT"),
        ("UCS", "UCS_CONTROL_OBJECT"),
        ("APPID", "APPID_CONTROL_OBJECT"),
        ("BLOCK_RECORD", "BLOCK_CONTROL_OBJECT"),
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
    let block_names: BTreeMap<u64, String> = crate::graph::resolve_block_names(&raws, file_version);
    // Handles the output defines: the objects it writes and every table record
    // it can write. A record that writes none still names a defined handle,
    // which only matters for UCS references, left out here.
    let mut defined: HashSet<u64> = objects.clone();
    defined.extend(
        raws.iter()
            .filter(|r| r.kind.is_table_entry())
            .map(|r| r.handle.value),
    );
    let has_ucs = raws.iter().any(|r| r.kind == ObjectType::Ucs);
    // Linetype handle -> (name, whether the LTYPE table writes it).
    let mut ltypes: BTreeMap<u64, (String, bool)> = BTreeMap::new();
    for raw in raws.iter().filter(|r| r.kind == ObjectType::Ltype) {
        if let DecodedEntity::Ltype(l) = crate::entities::decode_from_raw(raw, file_version) {
            let written = raw.handle.value < seed && ltype_groups(&l).is_some();
            ltypes.insert(raw.handle.value, (l.header.name.clone(), written));
        }
    }
    for raw in &raws {
        let handle = raw.handle.value;
        if handle >= seed {
            continue;
        }
        if !matches!(
            raw.kind,
            ObjectType::Layer
                | ObjectType::Style
                | ObjectType::Ltype
                | ObjectType::AppId
                | ObjectType::Vport
                | ObjectType::BlockHeader
        ) {
            continue;
        }
        let refs = crate::tables::handles::refs(raw, file_version).ok();
        let (table, name, groups) = match crate::entities::decode_from_raw(raw, file_version) {
            DecodedEntity::Layer(l) => {
                let lr = layer_refs(refs.as_ref(), file_version, &ltypes);
                (
                    "LAYER",
                    l.header.name.clone(),
                    Some(layer_groups(&l, &lr, version, &defined)),
                )
            }
            DecodedEntity::Style(s) => ("STYLE", s.header.name.clone(), style_groups(&s)),
            DecodedEntity::Ltype(l) => ("LTYPE", l.header.name.clone(), ltype_groups(&l)),
            // AutoCAD's export leaves this internal application out of the
            // APPID table on all four drawings of the corpus that hold it.
            DecodedEntity::AppId(a) if a.header.name == "ACAD_SPLINE_DATA" => continue,
            DecodedEntity::AppId(a) => ("APPID", a.header.name.clone(), Some(appid_groups(&a))),
            DecodedEntity::VPort(v) => (
                "VPORT",
                v.header.name.clone(),
                vport_groups(&v, refs.as_ref(), version, &defined),
            ),
            DecodedEntity::BlockRecord(b) => {
                let name = block_names
                    .get(&handle)
                    .cloned()
                    .unwrap_or_else(|| b.header.name.clone());
                let groups =
                    block_record_groups(&b, &name, refs.as_ref(), file_version, version, &defined);
                ("BLOCK_RECORD", name, Some(groups))
            }
            _ => continue,
        };
        let Some(groups) = groups else { continue };
        let (reactors, xdictionary) = match &refs {
            Some(r) => (
                if r.reactors.iter().all(|h| defined.contains(h)) {
                    r.reactors.clone()
                } else {
                    Vec::new()
                },
                r.xdictionary.filter(|h| defined.contains(h)),
            ),
            None => (Vec::new(), None),
        };
        if let Some(t) = tables.iter_mut().find(|t| t.name == table) {
            t.records.push(Record {
                handle,
                groups,
                name,
                reactors,
                xdictionary,
            });
        }
    }
    if !tables
        .iter()
        .any(|t| t.name == "LAYER" && !t.records.is_empty())
    {
        return Vec::new();
    }
    // An empty UCS table is the drawing's own: none of its UCS records exists.
    tables.retain(|t| !t.records.is_empty() || (t.name == "UCS" && !has_ucs));
    tables
}

fn bracket(w: &mut crate::dxf::DxfWriter, name: &str, groups: &[HeaderGroup]) {
    w.write_string(102, &format!("{{{name}"));
    write_groups(w, groups);
    w.write_string(102, "}");
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
                ],
            );
            if !r.reactors.is_empty() {
                let rs: Vec<HeaderGroup> = r
                    .reactors
                    .iter()
                    .map(|h| HeaderGroup::Handle(330, *h))
                    .collect();
                bracket(w, "ACAD_REACTORS", &rs);
            }
            if let Some(x) = r.xdictionary {
                bracket(w, "ACAD_XDICTIONARY", &[HeaderGroup::Handle(360, x)]);
            }
            write_groups(
                w,
                &[
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
