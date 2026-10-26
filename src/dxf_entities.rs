//! The DXF `ENTITIES` section from the decoded drawing entities.
//!
//! [`build`] turns each decoded entity of model space and of the active paper
//! space into the record AutoCAD's own DXF export writes for it, and [`write`]
//! emits them. A record is the handle, the reactors and extension dictionary
//! the handle stream names, the owner (the block record), the `AcDbEntity`
//! subclass (layer, linetype, colour, lineweight, linetype scale, visibility,
//! transparency, material) and the groups of the entity's own subclasses, under
//! the codes and with the defaults AutoCAD leaves out that its exports show.
//!
//! An entity whose decoder surfaces no DXF mapping yet is not written, and
//! [`Entities::skipped`] counts it by kind.

use std::collections::{BTreeMap, HashSet};

use crate::dxf::DxfVersion;
use crate::dxf_sections::{HeaderGroup, write_groups};
use crate::dxf_tables::{Table, from_r2007};
use crate::entities::{DecodedEntity, Point2D, Point3D};
use crate::entity_refs::{EntityRefs, entity_refs};
use crate::header_vars::{HeaderVars, lineweight_hundredths};
use crate::object::RawObject;
use crate::reader::DwgFile;
use crate::version::Version;

/// One entity record.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRecord {
    /// The DXF entity name.
    pub kind: &'static str,
    /// The entity's handle.
    pub handle: u64,
    /// The owner: the model space or paper space block record, or the block the
    /// entity is in.
    pub owner: u64,
    /// The reactors written.
    pub reactors: Vec<u64>,
    /// The extension dictionary written.
    pub xdictionary: Option<u64>,
    /// The groups from `100 AcDbEntity` on, in order.
    pub groups: Vec<HeaderGroup>,
}

/// The entities of a drawing as the converter writes them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entities {
    /// Entities of model space and of the active paper space, in file order.
    pub records: Vec<EntityRecord>,
    /// Entities left out, by kind.
    pub skipped: BTreeMap<String, usize>,
}

struct Ctx<'a> {
    version: DxfVersion,
    layers: BTreeMap<u64, &'a str>,
    linetypes: BTreeMap<u64, &'a str>,
    styles: BTreeMap<u64, &'a str>,
    blocks: BTreeMap<u64, &'a str>,
    model_space: u64,
    paper_space: u64,
    material_by_block: Option<u64>,
}

fn from_r2010(version: DxfVersion) -> bool {
    matches!(
        version,
        DxfVersion::R2010 | DxfVersion::R2013 | DxfVersion::R2018
    )
}

fn names(tables: &[Table], table: &str) -> BTreeMap<u64, String> {
    tables
        .iter()
        .find(|t| t.name == table)
        .map(|t| {
            t.records
                .iter()
                .map(|r| (r.handle, r.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn real_ok(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn p3(g: &mut Vec<HeaderGroup>, code: i32, p: Point3D) {
    g.push(HeaderGroup::Real(code, p.x));
    g.push(HeaderGroup::Real(code + 10, p.y));
    g.push(HeaderGroup::Real(code + 20, p.z));
}

fn p2z(g: &mut Vec<HeaderGroup>, code: i32, p: Point2D, z: f64) {
    g.push(HeaderGroup::Real(code, p.x));
    g.push(HeaderGroup::Real(code + 10, p.y));
    g.push(HeaderGroup::Real(code + 20, z));
}

fn finite3(p: &Point3D) -> bool {
    real_ok(&[p.x, p.y, p.z])
}

const Z: Point3D = Point3D {
    x: 0.0,
    y: 0.0,
    z: 1.0,
};

fn thickness(g: &mut Vec<HeaderGroup>, t: f64) {
    if t != 0.0 {
        g.push(HeaderGroup::Real(39, t));
    }
}

fn extrusion(g: &mut Vec<HeaderGroup>, e: Point3D) {
    if e != Z {
        p3(g, 210, e);
    }
}

fn sub(name: &str) -> HeaderGroup {
    HeaderGroup::Text(100, name.into())
}

/// The `AcDbEntity` groups of an entity.
fn common_groups(
    c: &crate::common_entity::CommonEntityData,
    refs: &EntityRefs,
    paper_space: bool,
    ctx: &Ctx<'_>,
    defined: &HashSet<u64>,
) -> Option<Vec<HeaderGroup>> {
    let mut g = vec![sub("AcDbEntity")];
    if paper_space {
        g.push(HeaderGroup::Int(67, 1));
    }
    g.push(HeaderGroup::Text(
        8,
        ctx.layers.get(&refs.layer)?.to_string(),
    ));
    match c.ltype_flags {
        0 => {}
        1 => g.push(HeaderGroup::Text(6, "ByBlock".into())),
        2 => g.push(HeaderGroup::Text(6, "Continuous".into())),
        _ => g.push(HeaderGroup::Text(
            6,
            ctx.linetypes.get(&refs.linetype?)?.to_string(),
        )),
    }
    let index = i64::from(c.color_raw & 0x01FF);
    if index != 256 {
        g.push(HeaderGroup::Int(62, index));
    }
    if (c.ltype_scale - 1.0).abs() > 0.0 {
        if !c.ltype_scale.is_finite() {
            return None;
        }
        g.push(HeaderGroup::Real(48, c.ltype_scale));
    }
    if c.invisibility & 1 != 0 {
        g.push(HeaderGroup::Int(60, 1));
    }
    if let Some(w) = lineweight_hundredths(c.lineweight).filter(|w| *w != -1) {
        g.push(HeaderGroup::Int(370, i64::from(w)));
    }
    if let Some(rgb) = c.color_rgb {
        if rgb >> 24 == 0xC2 {
            g.push(HeaderGroup::Int(420, i64::from(rgb & 0xFF_FFFF)));
        }
    }
    if let Some(alpha) = c.color_alpha {
        g.push(HeaderGroup::Int(440, i64::from(alpha)));
    }
    if from_r2007(ctx.version) {
        let material = match refs.material_flags {
            1 => ctx.material_by_block,
            3 => refs.material,
            _ => None,
        };
        if let Some(m) = material.filter(|m| defined.contains(m)) {
            g.push(HeaderGroup::Handle(347, m));
        }
    }
    Some(g)
}

/// The groups of a TEXT or an ATTRIB's text, in `AcDbText` order; the vertical
/// alignment (73) goes in the second `AcDbText` of a TEXT only.
fn text_groups(
    t: &crate::entities::text::Text,
    style: Option<&str>,
    second: bool,
) -> Option<Vec<HeaderGroup>> {
    if !(real_ok(&[
        t.elevation,
        t.insertion_point.x,
        t.insertion_point.y,
        t.thickness,
        t.oblique_angle,
        t.rotation_angle,
        t.height,
        t.width_factor,
    ]) && finite3(&t.extrusion))
    {
        return None;
    }
    let mut g = Vec::new();
    thickness(&mut g, t.thickness);
    p2z(&mut g, 10, t.insertion_point, t.elevation);
    g.push(HeaderGroup::Real(40, t.height));
    g.push(HeaderGroup::Text(1, t.text.clone()));
    if t.rotation_angle != 0.0 {
        g.push(HeaderGroup::Real(50, t.rotation_angle.to_degrees()));
    }
    if t.width_factor != 1.0 {
        g.push(HeaderGroup::Real(41, t.width_factor));
    }
    if t.oblique_angle != 0.0 {
        g.push(HeaderGroup::Real(51, t.oblique_angle.to_degrees()));
    }
    if let Some(name) = style.filter(|n| !n.eq_ignore_ascii_case("Standard")) {
        g.push(HeaderGroup::Text(7, name.to_string()));
    }
    if t.generation != 0 {
        g.push(HeaderGroup::Int(71, i64::from(t.generation)));
    }
    if t.h_align != 0 {
        g.push(HeaderGroup::Int(72, i64::from(t.h_align)));
    }
    if let Some(a) = t.alignment_point {
        if !real_ok(&[a.x, a.y]) {
            return None;
        }
        p2z(&mut g, 11, a, t.elevation);
    }
    extrusion(&mut g, t.extrusion);
    if second {
        g.push(sub("AcDbText"));
        if t.v_align != 0 {
            g.push(HeaderGroup::Int(73, i64::from(t.v_align)));
        }
    }
    Some(g)
}

/// The groups of the entity's own subclasses, with its DXF name.
fn own_groups(
    decoded: &DecodedEntity,
    refs: &EntityRefs,
    ctx: &Ctx<'_>,
    parent: Option<&str>,
) -> Option<(&'static str, Vec<HeaderGroup>)> {
    let mut g = Vec::new();
    fn first_ref<'a>(refs: &EntityRefs, m: &BTreeMap<u64, &'a str>) -> Option<&'a str> {
        refs.rest.first().and_then(|h| m.get(h)).copied()
    }
    let kind = match decoded {
        DecodedEntity::Line(l) => {
            if !(finite3(&l.start)
                && finite3(&l.end)
                && finite3(&l.extrusion)
                && l.thickness.is_finite())
            {
                return None;
            }
            g.push(sub("AcDbLine"));
            thickness(&mut g, l.thickness);
            p3(&mut g, 10, l.start);
            p3(&mut g, 11, l.end);
            extrusion(&mut g, l.extrusion);
            "LINE"
        }
        DecodedEntity::Point(p) => {
            if !(finite3(&p.position)
                && finite3(&p.extrusion)
                && real_ok(&[p.thickness, p.x_axis_angle]))
            {
                return None;
            }
            g.push(sub("AcDbPoint"));
            p3(&mut g, 10, p.position);
            thickness(&mut g, p.thickness);
            extrusion(&mut g, p.extrusion);
            if p.x_axis_angle != 0.0 {
                g.push(HeaderGroup::Real(50, p.x_axis_angle.to_degrees()));
            }
            "POINT"
        }
        DecodedEntity::Circle(k) => {
            if !(finite3(&k.center) && finite3(&k.extrusion) && real_ok(&[k.radius, k.thickness])) {
                return None;
            }
            g.push(sub("AcDbCircle"));
            thickness(&mut g, k.thickness);
            p3(&mut g, 10, k.center);
            g.push(HeaderGroup::Real(40, k.radius));
            extrusion(&mut g, k.extrusion);
            "CIRCLE"
        }
        DecodedEntity::Arc(a) => {
            if !(finite3(&a.center)
                && finite3(&a.extrusion)
                && real_ok(&[a.radius, a.thickness, a.start_angle, a.end_angle]))
            {
                return None;
            }
            g.push(sub("AcDbCircle"));
            thickness(&mut g, a.thickness);
            p3(&mut g, 10, a.center);
            g.push(HeaderGroup::Real(40, a.radius));
            extrusion(&mut g, a.extrusion);
            g.push(sub("AcDbArc"));
            g.push(HeaderGroup::Real(50, a.start_angle.to_degrees()));
            g.push(HeaderGroup::Real(51, a.end_angle.to_degrees()));
            "ARC"
        }
        DecodedEntity::Ellipse(e) => {
            if !(finite3(&e.center)
                && finite3(&e.major_axis)
                && finite3(&e.extrusion)
                && real_ok(&[e.axis_ratio, e.start_param, e.end_param]))
            {
                return None;
            }
            g.push(sub("AcDbEllipse"));
            p3(&mut g, 10, e.center);
            p3(&mut g, 11, e.major_axis);
            p3(&mut g, 210, e.extrusion);
            g.push(HeaderGroup::Real(40, e.axis_ratio));
            g.push(HeaderGroup::Real(41, e.start_param));
            g.push(HeaderGroup::Real(42, e.end_param));
            "ELLIPSE"
        }
        DecodedEntity::Solid(k) => {
            if !(real_ok(&[k.thickness, k.elevation])
                && finite3(&k.extrusion)
                && k.corners.iter().all(|p| real_ok(&[p.x, p.y])))
            {
                return None;
            }
            g.push(sub("AcDbTrace"));
            thickness(&mut g, k.thickness);
            for (n, p) in k.corners.iter().enumerate() {
                p2z(&mut g, 10 + n as i32, *p, k.elevation);
            }
            extrusion(&mut g, k.extrusion);
            "SOLID"
        }
        DecodedEntity::Ray(r) => {
            if !(finite3(&r.start) && finite3(&r.direction)) {
                return None;
            }
            g.push(sub("AcDbRay"));
            p3(&mut g, 10, r.start);
            p3(&mut g, 11, r.direction);
            "RAY"
        }
        DecodedEntity::XLine(x) => {
            if !(finite3(&x.point) && finite3(&x.direction)) {
                return None;
            }
            g.push(sub("AcDbXline"));
            p3(&mut g, 10, x.point);
            p3(&mut g, 11, x.direction);
            "XLINE"
        }
        DecodedEntity::ThreeDFace(f) => {
            if !f.corners.iter().all(finite3) {
                return None;
            }
            g.push(sub("AcDbFace"));
            for (n, p) in f.corners.iter().enumerate() {
                p3(&mut g, 10 + n as i32, *p);
            }
            if f.invisible_edges != 0 {
                g.push(HeaderGroup::Int(70, i64::from(f.invisible_edges)));
            }
            "3DFACE"
        }
        DecodedEntity::Text(t) => {
            g.push(sub("AcDbText"));
            g.extend(text_groups(t, first_ref(refs, &ctx.styles), true)?);
            "TEXT"
        }
        DecodedEntity::Attrib(a) => {
            if a.mtext.is_some() {
                return None;
            }
            g.push(sub("AcDbText"));
            g.extend(text_groups(&a.text, first_ref(refs, &ctx.styles), false)?);
            g.push(sub("AcDbAttribute"));
            if from_r2010(ctx.version) {
                g.push(HeaderGroup::Int(280, 0));
            }
            g.push(HeaderGroup::Text(2, a.tag.clone()));
            g.push(HeaderGroup::Int(70, i64::from(a.flags)));
            if a.field_length != 0 {
                g.push(HeaderGroup::Int(73, i64::from(a.field_length)));
            }
            if a.text.v_align != 0 {
                g.push(HeaderGroup::Int(74, i64::from(a.text.v_align)));
            }
            if from_r2007(ctx.version) {
                g.push(HeaderGroup::Int(280, i64::from(a.lock_position)));
            }
            "ATTRIB"
        }
        DecodedEntity::Insert(i) => {
            if !(finite3(&i.insertion_point)
                && finite3(&i.scale)
                && finite3(&i.extrusion)
                && i.rotation.is_finite())
            {
                return None;
            }
            g.push(sub("AcDbBlockReference"));
            if i.has_attribs {
                g.push(HeaderGroup::Int(66, 1));
            }
            g.push(HeaderGroup::Text(
                2,
                first_ref(refs, &ctx.blocks)?.to_string(),
            ));
            p3(&mut g, 10, i.insertion_point);
            if i.scale.x != 1.0 {
                g.push(HeaderGroup::Real(41, i.scale.x));
            }
            if i.scale.y != 1.0 {
                g.push(HeaderGroup::Real(42, i.scale.y));
            }
            if i.scale.z != 1.0 {
                g.push(HeaderGroup::Real(43, i.scale.z));
            }
            if i.rotation != 0.0 {
                g.push(HeaderGroup::Real(50, i.rotation.to_degrees()));
            }
            extrusion(&mut g, i.extrusion);
            "INSERT"
        }
        DecodedEntity::MText(m) => {
            if matches!(ctx.version, DxfVersion::R2018)
                || m.background.is_some()
                || m.background_flags != 0
                || m.column_type != 0
                || !(finite3(&m.insertion_point)
                    && finite3(&m.extrusion)
                    && finite3(&m.x_axis_direction)
                    && real_ok(&[
                        m.rect_width,
                        m.nominal_text_height,
                        m.extents_height,
                        m.extents_width,
                        m.linespace_factor,
                        m.rect_height.unwrap_or(0.0),
                    ]))
            {
                return None;
            }
            g.push(sub("AcDbMText"));
            p3(&mut g, 10, m.insertion_point);
            g.push(HeaderGroup::Real(40, m.nominal_text_height));
            g.push(HeaderGroup::Real(41, m.rect_width));
            if let Some(h) = m.rect_height {
                g.push(HeaderGroup::Real(46, h));
            }
            g.push(HeaderGroup::Int(71, i64::from(m.attachment_point)));
            g.push(HeaderGroup::Int(72, i64::from(m.drawing_direction)));
            let chars: Vec<char> = m.text.chars().collect();
            let mut chunks: Vec<String> = chars.chunks(250).map(|c| c.iter().collect()).collect();
            let last = chunks.pop().unwrap_or_default();
            for c in chunks {
                g.push(HeaderGroup::Text(3, c));
            }
            g.push(HeaderGroup::Text(1, last));
            if let Some(name) =
                first_ref(refs, &ctx.styles).filter(|n| !n.eq_ignore_ascii_case("Standard"))
            {
                g.push(HeaderGroup::Text(7, name.to_string()));
            }
            extrusion(&mut g, m.extrusion);
            if m.x_axis_direction
                != (Point3D {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                })
            {
                p3(&mut g, 11, m.x_axis_direction);
            }
            g.push(HeaderGroup::Int(73, i64::from(m.linespace_style)));
            g.push(HeaderGroup::Real(44, m.linespace_factor));
            "MTEXT"
        }
        DecodedEntity::LwPolyline(p) => {
            let ok = p.vertices.iter().all(|v| real_ok(&[v.x, v.y]))
                && p.bulges.iter().all(|b| b.is_finite())
                && p.widths.iter().all(|(a, b)| real_ok(&[*a, *b]))
                && real_ok(&[
                    p.elevation.unwrap_or(0.0),
                    p.thickness.unwrap_or(0.0),
                    p.constant_width.unwrap_or(0.0),
                ])
                && p.extrusion.as_ref().is_none_or(finite3);
            if !ok {
                return None;
            }
            g.push(sub("AcDbPolyline"));
            g.push(HeaderGroup::Int(90, p.vertices.len() as i64));
            g.push(HeaderGroup::Int(70, i64::from(p.closed)));
            if p.widths.is_empty() || p.constant_width.is_some() {
                g.push(HeaderGroup::Real(43, p.constant_width.unwrap_or(0.0)));
            }
            if let Some(e) = p.elevation.filter(|e| *e != 0.0) {
                g.push(HeaderGroup::Real(38, e));
            }
            if let Some(t) = p.thickness.filter(|t| *t != 0.0) {
                g.push(HeaderGroup::Real(39, t));
            }
            if let Some(e) = p.extrusion {
                extrusion(&mut g, e);
            }
            for (i, v) in p.vertices.iter().enumerate() {
                g.push(HeaderGroup::Real(10, v.x));
                g.push(HeaderGroup::Real(20, v.y));
                if let Some((sw, ew)) = p.widths.get(i) {
                    g.push(HeaderGroup::Real(40, *sw));
                    g.push(HeaderGroup::Real(41, *ew));
                }
                if let Some(b) = p.bulges.get(i).filter(|b| **b != 0.0) {
                    g.push(HeaderGroup::Real(42, *b));
                }
                if let Some(id) = p.vertex_ids.get(i) {
                    g.push(HeaderGroup::Int(91, i64::from(*id)));
                }
            }
            "LWPOLYLINE"
        }
        DecodedEntity::Polyline(p) => {
            if !(real_ok(&[
                p.default_start_width,
                p.default_end_width,
                p.thickness,
                p.elevation,
            ]) && finite3(&p.extrusion))
            {
                return None;
            }
            g.push(sub("AcDb2dPolyline"));
            g.push(HeaderGroup::Int(66, 1));
            g.push(HeaderGroup::Real(10, 0.0));
            g.push(HeaderGroup::Real(20, 0.0));
            g.push(HeaderGroup::Real(30, p.elevation));
            g.push(HeaderGroup::Int(70, i64::from(p.flag)));
            if p.default_start_width != 0.0 {
                g.push(HeaderGroup::Real(40, p.default_start_width));
            }
            if p.default_end_width != 0.0 {
                g.push(HeaderGroup::Real(41, p.default_end_width));
            }
            thickness(&mut g, p.thickness);
            if p.curve_type != 0 {
                g.push(HeaderGroup::Int(75, i64::from(p.curve_type)));
            }
            extrusion(&mut g, p.extrusion);
            "POLYLINE"
        }
        DecodedEntity::Polyline3d(p) => {
            let curve = match p.flags[0] & 3 {
                1 => 5,
                2 => 6,
                _ => 0,
            };
            g.push(sub("AcDb3dPolyline"));
            g.push(HeaderGroup::Int(66, 1));
            p3(&mut g, 10, Point3D::default());
            g.push(HeaderGroup::Int(
                70,
                8 | i64::from(p.flags[1] & 1) | if curve != 0 { 4 } else { 0 },
            ));
            if curve != 0 {
                g.push(HeaderGroup::Int(75, curve));
            }
            "POLYLINE"
        }
        DecodedEntity::PolyfaceMesh(p) => {
            g.push(sub("AcDbPolyFaceMesh"));
            g.push(HeaderGroup::Int(66, 1));
            p3(&mut g, 10, Point3D::default());
            g.push(HeaderGroup::Int(70, 64));
            g.push(HeaderGroup::Int(71, i64::from(p.vertex_count)));
            g.push(HeaderGroup::Int(72, i64::from(p.face_count)));
            "POLYLINE"
        }
        DecodedEntity::Vertex(v) => {
            if !(finite3(&v.location)
                && real_ok(&[v.start_width, v.end_width, v.bulge])
                && v.tangent_direction.is_none_or(f64::is_finite))
            {
                return None;
            }
            g.push(sub("AcDbVertex"));
            g.push(sub("AcDb2dVertex"));
            p3(&mut g, 10, v.location);
            if v.start_width != 0.0 {
                g.push(HeaderGroup::Real(40, v.start_width));
            }
            if v.end_width != 0.0 {
                g.push(HeaderGroup::Real(41, v.end_width));
            }
            if v.bulge != 0.0 {
                g.push(HeaderGroup::Real(42, v.bulge));
            }
            g.push(HeaderGroup::Int(70, i64::from(v.flag)));
            if let Some(t) = v.tangent_direction {
                g.push(HeaderGroup::Real(50, t.to_degrees()));
            }
            if let Some(id) = v.vertex_id.filter(|id| *id != 0) {
                g.push(HeaderGroup::Int(91, i64::from(id)));
            }
            "VERTEX"
        }
        DecodedEntity::VertexPoint(v) => {
            if !finite3(&v.location) {
                return None;
            }
            g.push(sub("AcDbVertex"));
            g.push(sub(if parent == Some("POLYFACE") {
                "AcDbPolyFaceMeshVertex"
            } else {
                "AcDb3dPolylineVertex"
            }));
            p3(&mut g, 10, v.location);
            g.push(HeaderGroup::Int(70, i64::from(v.flag)));
            "VERTEX"
        }
        DecodedEntity::VertexPfaceFace(v) => {
            g.push(sub("AcDbVertex"));
            g.push(sub("AcDbFaceRecord"));
            g.push(HeaderGroup::Real(10, 0.0));
            g.push(HeaderGroup::Real(20, 0.0));
            g.push(HeaderGroup::Real(30, 0.0));
            g.push(HeaderGroup::Int(70, 128));
            for (n, i) in v.vertex_indices.iter().enumerate() {
                if *i != 0 {
                    g.push(HeaderGroup::Int(71 + n as i32, i64::from(*i)));
                }
            }
            "VERTEX"
        }
        DecodedEntity::SeqEnd(_) => "SEQEND",
        _ => return None,
    };
    Some((kind, g))
}

fn kind_label(decoded: &DecodedEntity, raw: &RawObject) -> String {
    match decoded {
        DecodedEntity::Unhandled { .. } | DecodedEntity::Error { .. } => {
            format!("{} (not decoded)", raw.kind.short_label())
        }
        _ => raw.kind.short_label().to_string(),
    }
}

/// The handle the dictionary `dictionary` stores under `key`.
fn dictionary_entry(
    raws: &[RawObject],
    version: Version,
    dictionary: u64,
    key: &str,
) -> Option<u64> {
    let raw = raws.iter().find(|r| r.handle.value == dictionary)?;
    let DecodedEntity::Dictionary(d) = crate::entities::decode_from_raw(raw, version) else {
        return None;
    };
    let refs = crate::tables::handles::refs(raw, version).ok()?;
    let at = d.keys.iter().position(|k| k == key)?;
    refs.rest.get(at).copied().filter(|h| *h != 0)
}

/// The entities to write for `file` when the output targets `version`.
///
/// `tables` are the tables the converter writes: their layers, linetypes, text
/// styles and blocks name what an entity refers to by handle. `objects` are the
/// handles OBJECTS writes. Empty for targets before R2000, for a file whose
/// object stream or header does not read, and when `tables` is empty.
pub fn build(
    file: &DwgFile,
    version: DxfVersion,
    header: &HeaderVars,
    tables: &[Table],
    objects: &HashSet<u64>,
) -> Entities {
    let mut out = Entities::default();
    if tables.is_empty() || !version.supports_subclass_markers() || version == DxfVersion::R14 {
        return out;
    }
    let Some(Ok(raws)) = file.all_objects() else {
        return out;
    };
    let file_version = file.version();
    if matches!(file_version, Version::R14) {
        return out;
    }
    let handle = |name: &str| header.get_handle(name).map_or(0, |h| h.value);
    let layer_names = names(tables, "LAYER");
    let linetype_names = names(tables, "LTYPE");
    let style_names = names(tables, "STYLE");
    let block_names = names(tables, "BLOCK_RECORD");
    let ctx = Ctx {
        version,
        layers: layer_names.iter().map(|(h, n)| (*h, n.as_str())).collect(),
        linetypes: linetype_names
            .iter()
            .map(|(h, n)| (*h, n.as_str()))
            .collect(),
        styles: style_names.iter().map(|(h, n)| (*h, n.as_str())).collect(),
        blocks: block_names.iter().map(|(h, n)| (*h, n.as_str())).collect(),
        model_space: handle("BLOCK_RECORD_MODEL_SPACE"),
        paper_space: handle("BLOCK_RECORD_PAPER_SPACE"),
        material_by_block: dictionary_entry(
            &raws,
            file_version,
            handle("DICTIONARY_MATERIALS"),
            "ByBlock",
        ),
    };
    let seed = header.get_handle("$HANDSEED").map_or(u64::MAX, |h| h.value);
    let mut defined: HashSet<u64> = objects.clone();
    for t in tables {
        defined.extend(t.records.iter().map(|r| r.handle));
    }
    // Entities written so far: handle -> (kind for a child's subclass, paper space).
    let mut written: BTreeMap<u64, (&'static str, bool)> = BTreeMap::new();
    for raw in raws
        .iter()
        .filter(|r| r.is_entity() && r.handle.value < seed)
    {
        let decoded = crate::entities::decode_from_raw(raw, file_version);
        let Ok((common, refs)) = entity_refs(raw, file_version) else {
            *out.skipped.entry(kind_label(&decoded, raw)).or_default() += 1;
            continue;
        };
        let owner = match (refs.owner, common.raw_mode) {
            (Some(o), _) => o,
            (None, 1) => ctx.paper_space,
            _ => ctx.model_space,
        };
        let child = matches!(
            decoded,
            DecodedEntity::Attrib(_)
                | DecodedEntity::Vertex(_)
                | DecodedEntity::VertexPoint(_)
                | DecodedEntity::VertexPfaceFace(_)
                | DecodedEntity::SeqEnd(_)
        );
        let (parent, paper_space) = if child {
            match written.get(&owner) {
                Some((kind, paper)) => (Some(*kind), *paper),
                None => continue,
            }
        } else if owner == ctx.model_space {
            (None, false)
        } else if owner == ctx.paper_space {
            (None, true)
        } else {
            continue;
        };
        let Some((kind, own)) = own_groups(&decoded, &refs, &ctx, parent) else {
            *out.skipped.entry(kind_label(&decoded, raw)).or_default() += 1;
            continue;
        };
        let Some(mut groups) = common_groups(&common, &refs, paper_space, &ctx, &defined) else {
            *out.skipped.entry(kind_label(&decoded, raw)).or_default() += 1;
            continue;
        };
        groups.extend(own);
        let label = if matches!(decoded, DecodedEntity::PolyfaceMesh(_)) {
            "POLYFACE"
        } else {
            kind
        };
        written.insert(raw.handle.value, (label, paper_space));
        out.records.push(EntityRecord {
            kind,
            handle: raw.handle.value,
            owner,
            reactors: if refs.reactors.iter().all(|h| defined.contains(h)) {
                refs.reactors.clone()
            } else {
                Vec::new()
            },
            xdictionary: refs.xdictionary.filter(|h| defined.contains(h)),
            groups,
        });
    }
    let _ = ctx.version;
    out
}

/// `groups` with their text escaped for the release `version` (see
/// `dxf_header::escape_text`); markers are plain ASCII already.
fn escaped(groups: &[HeaderGroup], version: DxfVersion) -> Vec<HeaderGroup> {
    groups
        .iter()
        .map(|g| match g {
            HeaderGroup::Text(code, t) if !matches!(code, 0 | 100 | 102) => {
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

/// Write `entities` as an `ENTITIES` section.
pub fn write(w: &mut crate::dxf::DxfWriter, entities: &Entities) {
    w.begin_section("ENTITIES");
    for (kind, n) in &entities.skipped {
        w.write_comment(&format!("skipped: {kind} x{n}"));
    }
    for r in &entities.records {
        write_groups(
            w,
            &[
                HeaderGroup::Text(0, r.kind.into()),
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
        write_groups(w, &[HeaderGroup::Handle(330, r.owner)]);
        write_groups(w, &escaped(&r.groups, w.version()));
    }
    w.end_section();
}
