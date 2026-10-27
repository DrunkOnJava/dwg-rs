//! The DXF `TABLES` section from the decoded symbol tables.
//!
//! [`build`] turns the decoded VPORT, LTYPE, LAYER, STYLE, VIEW, APPID, DIMSTYLE
//! and BLOCK_RECORD records of a drawing into the `TABLE` blocks AutoCAD's own DXF export writes for it, and
//! [`write()`] emits them. Each record carries its handle and its owner (the
//! table's control object, from `AcDb:Header`), then the groups of its
//! `AcDb...TableRecord` subclass that the decoder surfaces, under the codes and
//! in the order of the exports of the public corpus.
//!
//! # What is written, and what is not
//!
//! - LAYER: name (2), flags (70: frozen 1, frozen in new viewports 2, locked 4,
//!   Xdep 16), colour (62, negative when the layer is off), linetype name (6,
//!   from the handle stream), plot flag (290, only when it is off: AutoCAD omits
//!   the default), lineweight (370) and the plot style handle (390). A layer
//!   whose colour is a true colour (the DWG stores no index for it) has no 62
//!   written; its material and visual style handles (347, 348) are not written.
//! - STYLE: name, flags (70: shape file 1, vertical 4, Xdep 16), 40, 41, 50
//!   (degrees), 71, 42, 3, 4.
//! - LTYPE: name, flags (70), description (3), alignment (72), dash count (73),
//!   pattern length (40) and each dash length (49) with its flag (74); a text or
//!   shape dash also its shape number (75), style (340), scale (46), rotation
//!   (50), offsets (44, 45) and text (9, from the linetype's text area).
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
//! - VIEW: name, flags (70: paper space 1, Xdep 16), the view as VPORT's (40 to
//!   44, 50, 71, 281, 10 to 12), the associated UCS (72, 110 to 112, 79, 146) and,
//!   from R2007, whether the camera plots (73) and the visual style (348).
//! - UCS: an empty table when the drawing has no UCS record, as AutoCAD writes
//!   it (every drawing of the public corpus). A drawing that has UCS records
//!   gets no UCS table: no corpus drawing holds one, so the UCS decoder has
//!   never read a real record, and §20.4.62 lists a second orthographic `BS` it
//!   does not read.
//! - DIMSTYLE: every dimension variable of the record (`tables::dimstyle`'s
//!   `decode_with_vars`) whose value is not the imperial default AutoCAD leaves
//!   out, the colours as indexes, the text style and linetypes by handle (the
//!   arrowhead blocks only when BLOCKS writes them), and the table header's `71`
//!   and `340` list.
//! - BLOCK_RECORD: the name the record's `BLOCK` entity gives (anonymous blocks
//!   carry their number there), the layout handle (340), insert units,
//!   explodability and scaling (70, 280, 281; R2007) and the preview bitmap
//!   (310, 127 bytes to a group).
//!
//! Every record also carries, from its handle stream, its reactors and its
//! extension dictionary (`102 {ACAD_REACTORS`, `102 {ACAD_XDICTIONARY`). A
//! handle is written only when the output defines the object it names: the
//! OBJECTS section writes dictionaries and plot style placeholders under their own
//! type, so those are named (a layout, material or visual style is written as an
//! untyped record and is not); the
//! entities that react to a record or insert a block are not written with their
//! handles yet, so a record whose reactors or insert handles name one has no
//! such bracket.
//!
//! Records the export itself creates while saving (their handles are at or
//! above the DWG's `$HANDSEED`) are not in the DWG and not written.
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
/// preamble (which [`write()`] emits from the table's own data).
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
    /// `VPORT`, `LTYPE`, `LAYER`, `STYLE`, `UCS` (empty), `APPID`, `DIMSTYLE` or `BLOCK_RECORD`.
    pub name: &'static str,
    /// The table's control object handle.
    pub handle: u64,
    /// Groups after the table's `70` count (a DIMSTYLE table's `AcDbDimStyleTable`
    /// subclass, its `71` count and `340` handles).
    pub header_extra: Vec<HeaderGroup>,
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
    /// DIMSTYLE names.
    pub dimstyles: Vec<String>,
    /// BLOCK_RECORD names.
    pub blocks: Vec<String>,
}

/// The blocks the BLOCKS section writes: only the two it writes empty.
///
/// A name or handle that selects any other block (`$DIMBLK`, a DIMSTYLE's arrowhead
/// blocks) points at a definition that never reaches the DXF, so it is not written.
/// Lift this when BLOCKS writes each block's contents.
pub const BLOCKS_EXPORTED: [&str; 2] = ["*Model_Space", "*Paper_Space"];

/// The layer the primitive exporter's entities go on while they carry no layer of
/// their own: `0` when it is visible and plottable (not frozen, not off, and no
/// group 290 of 0), else the first layer that is, so that geometry is not hidden
/// or kept out of a plot by a layer 0 the drawing froze, switched off or made
/// non-plotting. `None` when no layer qualifies (or there is no LAYER table).
pub fn visible_layer(tables: &[Table]) -> Option<String> {
    let layers = tables.iter().find(|t| t.name == "LAYER")?;
    let visible = |r: &&Record| {
        let int = |code: i32| {
            r.groups.iter().find_map(|g| match g {
                HeaderGroup::Int(c, v) if *c == code => Some(*v),
                _ => None,
            })
        };
        int(70).unwrap_or(0) & 1 == 0 && int(62).unwrap_or(0) >= 0 && int(290) != Some(0)
    };
    layers
        .records
        .iter()
        .filter(visible)
        .find(|r| r.name == "0")
        .or_else(|| layers.records.iter().find(visible))
        .map(|r| r.name.clone())
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
        dimstyles: names("DIMSTYLE"),
        blocks: names("BLOCK_RECORD")
            .into_iter()
            .filter(|n| BLOCKS_EXPORTED.contains(&n.as_str()))
            .collect(),
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
    file_version: crate::version::Version,
    version: DxfVersion,
    defined: &HashSet<u64>,
) -> Vec<HeaderGroup> {
    // R13 and R14 store no plot flag and no lineweight: the decoder leaves them
    // clear, which is not the drawing saying the layer does not plot.
    let stored = !matches!(file_version, crate::version::Version::R14);
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
    if stored && !l.plot_flag {
        g.push(HeaderGroup::Int(290, 0));
    }
    if let Some(w) = u8::try_from(l.lineweight)
        .ok()
        .and_then(lineweight_hundredths)
        .filter(|_| stored)
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

/// The text of a text dash: the NUL-terminated string at byte `offset` of the
/// linetype's text area (one byte a character up to R2004, UTF-16LE from R2007).
fn dash_text(area: &[u8], offset: usize, wide: bool) -> Option<String> {
    let tail = area.get(offset..)?;
    if wide {
        let units: Vec<u16> = tail
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|u| *u != 0)
            .collect();
        String::from_utf16(&units).ok()
    } else {
        let bytes: Vec<u8> = tail.iter().copied().take_while(|b| *b != 0).collect();
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// The groups of a LTYPE: per dash its length (49) and flags (74), and for a
/// text or shape dash its shape number (75), style (340), scale (46), rotation
/// (50), offsets (44, 45) and text (9). The style is the handle after the xref
/// block's, one per dash; a list that is not 1 + the dash count long is not read.
/// Bit 8 of a dash's flags (the DWG sets it on a text dash) is written from R2013
/// and cleared before, as AutoCAD's exports do.
fn ltype_groups(
    l: &crate::tables::ltype::LtypeEntry,
    refs: Option<&EntryRefs>,
    file_version: crate::version::Version,
    version: DxfVersion,
    defined: &HashSet<u64>,
) -> Option<Vec<HeaderGroup>> {
    if !l.pattern_length.is_finite() || l.dashes.iter().any(|d| !d.length.is_finite()) {
        return None;
    }
    let complex = l.dashes.iter().any(|d| d.shape_flag != 0);
    let styles = match refs {
        Some(r) if r.rest.len() == 1 + l.dashes.len() => Some(&r.rest[1..]),
        _ if complex => return None,
        _ => None,
    };
    let mut g = vec![
        HeaderGroup::Text(100, "AcDbLinetypeTableRecord".into()),
        HeaderGroup::Text(2, l.header.name.clone()),
        HeaderGroup::Int(70, xdep(&l.header)),
        HeaderGroup::Text(3, l.description.clone()),
        HeaderGroup::Int(72, i64::from(l.alignment)),
        HeaderGroup::Int(73, l.dashes.len() as i64),
        HeaderGroup::Real(40, l.pattern_length),
    ];
    for (i, d) in l.dashes.iter().enumerate() {
        g.push(HeaderGroup::Real(49, d.length));
        let flag = if matches!(version, DxfVersion::R2013 | DxfVersion::R2018) {
            d.shape_flag
        } else {
            d.shape_flag & !0x08
        };
        g.push(HeaderGroup::Int(74, i64::from(flag)));
        if d.shape_flag == 0 {
            continue;
        }
        if !finite(&[d.scale, d.rotation, d.x_offset, d.y_offset]) {
            return None;
        }
        g.push(HeaderGroup::Int(75, i64::from(d.shape_number)));
        if let Some(h) = styles
            .and_then(|s| s.get(i))
            .copied()
            .filter(|h| *h != 0 && defined.contains(h))
        {
            g.push(HeaderGroup::Handle(340, h));
        }
        g.push(HeaderGroup::Real(46, d.scale));
        g.push(HeaderGroup::Real(50, d.rotation.to_degrees()));
        g.push(HeaderGroup::Real(44, d.x_offset));
        g.push(HeaderGroup::Real(45, d.y_offset));
        if d.has_text() {
            let text = dash_text(
                &l.text_area,
                usize::try_from(d.shape_number).ok()?,
                file_version.is_r2007_plus(),
            )?;
            g.push(HeaderGroup::Text(9, text));
        }
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

pub(crate) fn from_r2007(version: DxfVersion) -> bool {
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

/// The groups of a VIEW record: the view as VPORT's, the associated UCS and, from
/// R2007, whether the camera plots and the visual style (the handle after the xref
/// block's and the background's).
fn view_groups(
    v: &crate::tables::view::ViewEntry,
    refs: Option<&EntryRefs>,
    file_version: crate::version::Version,
    version: DxfVersion,
    defined: &HashSet<u64>,
) -> Option<Vec<HeaderGroup>> {
    // R13 and R14 store neither a render mode nor an associated UCS.
    let r2000_plus = !matches!(file_version, crate::version::Version::R14);
    let reals = [
        v.view_height,
        v.view_width,
        v.view_center.x,
        v.view_center.y,
        v.target.x,
        v.target.y,
        v.target.z,
        v.view_direction.x,
        v.view_direction.y,
        v.view_direction.z,
        v.twist_angle,
        v.lens_length,
        v.front_clip,
        v.back_clip,
    ];
    if !finite(&reals) {
        return None;
    }
    let flags = xdep(&v.header) | i64::from(v.is_paperspace);
    let mut g = vec![
        HeaderGroup::Text(100, "AcDbViewTableRecord".into()),
        HeaderGroup::Text(2, v.header.name.clone()),
        HeaderGroup::Int(70, flags),
        HeaderGroup::Real(40, v.view_height),
    ];
    point2(&mut g, 10, v.view_center);
    g.push(HeaderGroup::Real(41, v.view_width));
    point3(&mut g, 11, v.view_direction);
    point3(&mut g, 12, v.target);
    g.push(HeaderGroup::Real(42, v.lens_length));
    g.push(HeaderGroup::Real(43, v.front_clip));
    g.push(HeaderGroup::Real(44, v.back_clip));
    g.push(HeaderGroup::Real(50, v.twist_angle.to_degrees()));
    g.push(HeaderGroup::Int(71, i64::from(v.view_mode)));
    if r2000_plus {
        g.push(HeaderGroup::Int(281, i64::from(v.render_mode)));
        g.push(HeaderGroup::Int(72, flag(v.is_associated_ucs)));
    }
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
        point3(&mut g, 110, ucs.origin);
        point3(&mut g, 111, ucs.x_direction);
        point3(&mut g, 112, ucs.y_direction);
        g.push(HeaderGroup::Int(79, i64::from(ucs.ortho_view_type)));
        g.push(HeaderGroup::Real(146, ucs.elevation));
    }
    if from_r2007(version) {
        if let Some(plottable) = v.camera_plottable {
            g.push(HeaderGroup::Int(73, flag(plottable)));
        }
        if let Some(r) = refs.filter(|r| r.rest.len() == 6) {
            handle_group(&mut g, 348, r.rest.get(2).copied(), defined);
        }
    }
    Some(g)
}

/// Where a dimension variable lands in a DIMSTYLE record: its DXF group, the
/// value AutoCAD leaves the group out for, and the first target that has it.
struct DimGroup {
    name: &'static str,
    code: i32,
    default: DimDefault,
    from: DimFrom,
}

enum DimDefault {
    Real(f64),
    Int(i64),
    Text(&'static str),
}

#[derive(Clone, Copy, PartialEq)]
enum DimFrom {
    R2000,
    R2007,
    R2010,
}

use DimDefault::{Int as DInt, Real as DReal, Text as DText};
use DimFrom::{R2000, R2007, R2010};

const fn dg(name: &'static str, code: i32, default: DimDefault, from: DimFrom) -> DimGroup {
    DimGroup {
        name,
        code,
        default,
        from,
    }
}

/// The dimension variables a DIMSTYLE record carries as groups, in the order of
/// AutoCAD's export. AutoCAD writes a group only when its value differs from the
/// imperial default listed here, measured on every DIMSTYLE of the corpus.
const DIM_GROUPS: &[DimGroup] = &[
    dg("DIMPOST", 3, DText(""), R2000),
    dg("DIMAPOST", 4, DText(""), R2000),
    dg("DIMSCALE", 40, DReal(1.0), R2000),
    dg("DIMASZ", 41, DReal(0.18), R2000),
    dg("DIMEXO", 42, DReal(0.0625), R2000),
    dg("DIMDLI", 43, DReal(0.38), R2000),
    dg("DIMEXE", 44, DReal(0.18), R2000),
    dg("DIMRND", 45, DReal(0.0), R2000),
    dg("DIMDLE", 46, DReal(0.0), R2000),
    dg("DIMTP", 47, DReal(0.0), R2000),
    dg("DIMTM", 48, DReal(0.0), R2000),
    dg("DIMFXL", 49, DReal(1.0), R2007),
    dg("DIMJOGANG", 50, DReal(std::f64::consts::FRAC_PI_4), R2007),
    dg("DIMTFILL", 69, DInt(0), R2007),
    dg("DIMTOL", 71, DInt(0), R2000),
    dg("DIMLIM", 72, DInt(0), R2000),
    dg("DIMTIH", 73, DInt(1), R2000),
    dg("DIMTOH", 74, DInt(1), R2000),
    dg("DIMSE1", 75, DInt(0), R2000),
    dg("DIMSE2", 76, DInt(0), R2000),
    dg("DIMTAD", 77, DInt(0), R2000),
    dg("DIMZIN", 78, DInt(0), R2000),
    dg("DIMAZIN", 79, DInt(0), R2000),
    dg("DIMARCSYM", 90, DInt(0), R2007),
    dg("DIMTXT", 140, DReal(0.18), R2000),
    dg("DIMCEN", 141, DReal(0.09), R2000),
    dg("DIMTSZ", 142, DReal(0.0), R2000),
    dg("DIMALTF", 143, DReal(25.4), R2000),
    dg("DIMLFAC", 144, DReal(1.0), R2000),
    dg("DIMTVP", 145, DReal(0.0), R2000),
    dg("DIMTFAC", 146, DReal(1.0), R2000),
    dg("DIMGAP", 147, DReal(0.09), R2000),
    dg("DIMALTRND", 148, DReal(0.0), R2000),
    dg("DIMALT", 170, DInt(0), R2000),
    dg("DIMALTD", 171, DInt(2), R2000),
    dg("DIMTOFL", 172, DInt(0), R2000),
    dg("DIMSAH", 173, DInt(0), R2000),
    dg("DIMTIX", 174, DInt(0), R2000),
    dg("DIMSOXD", 175, DInt(0), R2000),
    dg("DIMADEC", 179, DInt(0), R2000),
    dg("DIMDEC", 271, DInt(4), R2000),
    dg("DIMTDEC", 272, DInt(4), R2000),
    dg("DIMALTU", 273, DInt(2), R2000),
    dg("DIMALTTD", 274, DInt(2), R2000),
    dg("DIMAUNIT", 275, DInt(0), R2000),
    dg("DIMFRAC", 276, DInt(0), R2000),
    dg("DIMLUNIT", 277, DInt(2), R2000),
    dg("DIMDSEP", 278, DInt(46), R2000),
    dg("DIMTMOVE", 279, DInt(0), R2000),
    dg("DIMJUST", 280, DInt(0), R2000),
    dg("DIMSD1", 281, DInt(0), R2000),
    dg("DIMSD2", 282, DInt(0), R2000),
    dg("DIMTOLJ", 283, DInt(1), R2000),
    dg("DIMTZIN", 284, DInt(0), R2000),
    dg("DIMALTZ", 285, DInt(0), R2000),
    dg("DIMALTTZ", 286, DInt(0), R2000),
    dg("DIMUPT", 288, DInt(0), R2000),
    dg("DIMATFIT", 289, DInt(3), R2000),
    dg("DIMFXLON", 290, DInt(0), R2007),
    dg("DIMTXTDIRECTION", 295, DInt(0), R2010),
    dg("DIMLWD", 371, DInt(-2), R2000),
    dg("DIMLWE", 372, DInt(-2), R2000),
];

/// The dimension colours and their groups, written as the colour's index and
/// left out for ByBlock (0).
const DIM_COLORS: &[(&str, i32)] = &[
    ("DIMCLRD", 176),
    ("DIMCLRE", 177),
    ("DIMCLRT", 178),
    ("DIMTFILLCLR", 70),
];

fn dim_allowed(from: DimFrom, version: DxfVersion) -> bool {
    match from {
        R2000 => true,
        R2007 => from_r2007(version),
        R2010 => matches!(
            version,
            DxfVersion::R2010 | DxfVersion::R2013 | DxfVersion::R2018
        ),
    }
}

/// The groups of a DIMSTYLE record.
///
/// `refs` holds the record's handle stream: the xref block, the text style, the
/// leader, first and second arrowhead blocks and, from R2007, the dimension
/// linetype and the two extension line linetypes. A handle is written only when
/// it is not NULL and the output defines what it names.
fn dimstyle_groups(
    e: &crate::tables::dimstyle::DimStyleEntry,
    vars: &[crate::tables::dimstyle::DimVar],
    refs: Option<&EntryRefs>,
    file_version: crate::version::Version,
    version: DxfVersion,
    defined: &HashSet<u64>,
    exported_blocks: &HashSet<u64>,
) -> Option<Vec<HeaderGroup>> {
    use crate::tables::dimstyle::DimValue;
    let mut g = vec![
        HeaderGroup::Text(100, "AcDbDimStyleTableRecord".into()),
        HeaderGroup::Text(2, e.header.name.clone()),
        HeaderGroup::Int(70, xdep(&e.header)),
    ];
    for d in DIM_GROUPS {
        if !dim_allowed(d.from, version) {
            continue;
        }
        let Some(v) = vars.iter().find(|v| v.name == d.name) else {
            continue;
        };
        match (&v.value, &d.default) {
            (DimValue::Real(x), DReal(def)) => {
                if !x.is_finite() {
                    return None;
                }
                if x != def {
                    let out = if d.name == "DIMJOGANG" {
                        x.to_degrees()
                    } else {
                        *x
                    };
                    g.push(HeaderGroup::Real(d.code, out));
                }
            }
            (DimValue::Int(x), DInt(def)) if x != def => {
                g.push(HeaderGroup::Int(d.code, *x));
            }
            (DimValue::Flag(x), DInt(def)) if i64::from(*x) != *def => {
                g.push(HeaderGroup::Int(d.code, i64::from(*x)));
            }
            (DimValue::Text(t), DText(def)) if t != def => {
                g.push(HeaderGroup::Text(d.code, t.clone()));
            }
            _ => {}
        }
    }
    for (name, code) in DIM_COLORS {
        if *name == "DIMTFILLCLR" && !from_r2007(version) {
            continue;
        }
        if let Some(DimValue::Color(c)) = vars.iter().find(|v| v.name == *name).map(|v| &v.value) {
            if let Some(aci) = c.aci().filter(|a| *a != 0) {
                g.push(HeaderGroup::Int(*code, i64::from(aci)));
            }
        }
    }
    let want = if file_version.is_r2007_plus() { 9 } else { 6 };
    if let Some(r) = refs.filter(|r| r.rest.len() == want) {
        for (i, code) in [340, 341, 342, 343, 344, 345, 346, 347].iter().enumerate() {
            if *code >= 345 && !from_r2007(version) {
                continue;
            }
            if let Some(h) = r.rest.get(i + 1).copied().filter(|h| *h != 0) {
                let block = (341..=344).contains(code);
                if defined.contains(&h) && (!block || exported_blocks.contains(&h)) {
                    g.push(HeaderGroup::Handle(*code, h));
                }
            }
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

/// The handles after the dimstyles in the DIMSTYLE control object's list: the
/// `340` handles of the DXF table header, the non-NULL ones the output defines.
///
/// The control object (§20.4.67) holds `BL` numentries, then NULL, the
/// extension dictionary and the dimstyles; the handles that follow the
/// `numentries` of them are the table header's.
fn dimstyle_extras(
    raws: &[crate::object::RawObject],
    file_version: crate::version::Version,
    defined: &HashSet<u64>,
) -> Option<Vec<u64>> {
    let raw = raws
        .iter()
        .find(|r| r.kind == ObjectType::DimStyleControl)?;
    let mut c = crate::object::body_cursor(raw, file_version).ok()?;
    crate::common_entity::read_common_object_data_full(&mut c, file_version).ok()?;
    let n = c.read_bl().ok()? as usize;
    let refs = crate::tables::handles::refs(raw, file_version).ok()?;
    let extras = refs.rest.get(n..)?;
    Some(
        extras
            .iter()
            .copied()
            .filter(|h| *h != 0 && defined.contains(h))
            .collect(),
    )
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
        ("VIEW", "VIEW_CONTROL_OBJECT"),
        ("UCS", "UCS_CONTROL_OBJECT"),
        ("APPID", "APPID_CONTROL_OBJECT"),
        ("DIMSTYLE", "DIMSTYLE_CONTROL_OBJECT"),
        ("BLOCK_RECORD", "BLOCK_CONTROL_OBJECT"),
    ]
    .into_iter()
    .filter_map(|(name, ctl)| {
        Some(Table {
            name,
            handle: control(ctl)?,
            header_extra: Vec::new(),
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
    let exported_blocks: HashSet<u64> = block_names
        .iter()
        .filter(|(_, n)| BLOCKS_EXPORTED.contains(&n.as_str()))
        .map(|(h, _)| *h)
        .collect();
    let has_ucs = raws.iter().any(|r| r.kind == ObjectType::Ucs);
    let has_view = raws.iter().any(|r| r.kind == ObjectType::View);
    // Linetype handle -> (name, whether the LTYPE table writes it).
    let mut ltypes: BTreeMap<u64, (String, bool)> = BTreeMap::new();
    for raw in raws.iter().filter(|r| r.kind == ObjectType::Ltype) {
        if let DecodedEntity::Ltype(l) = crate::entities::decode_from_raw(raw, file_version) {
            let lt_refs = crate::tables::handles::refs(raw, file_version).ok();
            let written = raw.handle.value < seed
                && ltype_groups(&l, lt_refs.as_ref(), file_version, version, &defined).is_some();
            ltypes.insert(raw.handle.value, (l.header.name.clone(), written));
            if !written {
                defined.remove(&raw.handle.value);
            }
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
                | ObjectType::View
                | ObjectType::DimStyle
                | ObjectType::BlockHeader
        ) {
            continue;
        }
        let refs = crate::tables::handles::refs(raw, file_version).ok();
        let (table, name, groups) = if raw.kind == ObjectType::DimStyle {
            let Ok((e, vars)) = crate::tables::dimstyle::decode_with_vars(raw, file_version) else {
                continue;
            };
            let groups = dimstyle_groups(
                &e,
                &vars,
                refs.as_ref(),
                file_version,
                version,
                &defined,
                &exported_blocks,
            );
            ("DIMSTYLE", e.header.name.clone(), groups)
        } else {
            match crate::entities::decode_from_raw(raw, file_version) {
                DecodedEntity::Layer(l) => {
                    let lr = layer_refs(refs.as_ref(), file_version, &ltypes);
                    (
                        "LAYER",
                        l.header.name.clone(),
                        Some(layer_groups(&l, &lr, file_version, version, &defined)),
                    )
                }
                DecodedEntity::Style(s) => ("STYLE", s.header.name.clone(), style_groups(&s)),
                DecodedEntity::Ltype(l) => (
                    "LTYPE",
                    l.header.name.clone(),
                    ltype_groups(&l, refs.as_ref(), file_version, version, &defined),
                ),
                // AutoCAD's export leaves this internal application out of the
                // APPID table on all four drawings of the corpus that hold it.
                DecodedEntity::AppId(a) if a.header.name == "ACAD_SPLINE_DATA" => continue,
                DecodedEntity::AppId(a) => ("APPID", a.header.name.clone(), Some(appid_groups(&a))),
                DecodedEntity::View(v) => (
                    "VIEW",
                    v.header.name.clone(),
                    view_groups(&v, refs.as_ref(), file_version, version, &defined),
                ),
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
                    let groups = block_record_groups(
                        &b,
                        &name,
                        refs.as_ref(),
                        file_version,
                        version,
                        &defined,
                    );
                    ("BLOCK_RECORD", name, Some(groups))
                }
                _ => continue,
            }
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
    if let Some(extras) = dimstyle_extras(&raws, file_version, &defined) {
        if let Some(t) = tables.iter_mut().find(|t| t.name == "DIMSTYLE") {
            t.header_extra = vec![HeaderGroup::Text(100, "AcDbDimStyleTable".into())];
            t.header_extra
                .push(HeaderGroup::Int(71, extras.len() as i64));
            t.header_extra
                .extend(extras.into_iter().map(|h| HeaderGroup::Handle(340, h)));
        }
    }
    if !tables
        .iter()
        .any(|t| t.name == "LAYER" && !t.records.is_empty())
    {
        return Vec::new();
    }
    // An empty UCS table is the drawing's own: none of its UCS records exists.
    tables.retain(|t| {
        !t.records.is_empty() || (t.name == "UCS" && !has_ucs) || (t.name == "VIEW" && !has_view)
    });
    tables
}

/// `groups` with their text escaped for the release `w` writes (see
/// `dxf_header::escape_text`): the markers, the preview hex and the
/// structural groups are plain ASCII already.
fn escaped(groups: &[HeaderGroup], version: DxfVersion) -> Vec<HeaderGroup> {
    groups
        .iter()
        .map(|g| match g {
            HeaderGroup::Text(code, t) if !matches!(code, 0 | 100 | 102 | 310) => {
                HeaderGroup::Text(*code, crate::dxf_header::escape_text(version, t))
            }
            other => other.clone(),
        })
        .collect()
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
        write_groups(w, &t.header_extra);
        for r in &t.records {
            let handle_code = if t.name == "DIMSTYLE" { 105 } else { 5 };
            write_groups(
                w,
                &[
                    HeaderGroup::Text(0, t.name.into()),
                    HeaderGroup::Handle(handle_code, r.handle),
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
            write_groups(w, &escaped(&r.groups, w.version()));
        }
        w.write_string(0, "ENDTAB");
    }
    w.end_section();
}
