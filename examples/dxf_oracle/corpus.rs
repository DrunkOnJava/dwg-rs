//! Finding the drawing pairs in a corpus directory and loading both sides.

use dwg::classes::ClassMap;
use dwg::dxf_reader::{DxfDocument, DxfRecord};
use dwg::entities::{DecodedEntity, decode_from_raw, decode_from_raw_with_class_map};
use dwg::{DwgFile, ObjectType, Version};
use std::path::{Path, PathBuf};

/// One DWG and AutoCAD's DXF export of the same drawing.
pub struct DrawingPair {
    pub dwg: PathBuf,
    pub dxf: PathBuf,
}

/// Every `.dwg` under `dir` (subdirectories included, as the corpus fetch
/// lays sources out) with its twin, the first that exists of `<stem>.dxf`,
/// `<stem>_ascii.dxf` (the ACadSharp samples' name), and for an R14 save
/// `<name>_R14.dwg`, which nextgis exported only to R12 DXF, `<name>_R12.dxf`.
/// Returns the pairs and the DWGs that have no twin.
pub fn find_pairs(dir: &Path) -> std::io::Result<(Vec<DrawingPair>, Vec<PathBuf>)> {
    let mut dwgs = Vec::new();
    collect_dwgs(dir, 0, &mut dwgs)?;
    dwgs.sort();
    let mut pairs = Vec::new();
    let mut alone = Vec::new();
    for dwg in dwgs {
        let stem = dwg
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let parent = dwg.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let mut candidates = vec![
            parent.join(format!("{stem}.dxf")),
            parent.join(format!("{stem}_ascii.dxf")),
        ];
        if let Some(name) = stem.strip_suffix("_R14") {
            candidates.push(parent.join(format!("{name}_R12.dxf")));
        }
        match candidates.into_iter().find(|p| p.exists()) {
            Some(dxf) => pairs.push(DrawingPair { dwg, dxf }),
            None => alone.push(dwg),
        }
    }
    Ok((pairs, alone))
}

fn collect_dwgs(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() && depth < 4 {
            collect_dwgs(&path, depth + 1, out)?;
        } else if path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("dwg"))
        {
            out.push(path);
        }
    }
    Ok(())
}

/// One DWG object record and its decode.
pub struct DwgRecord {
    pub handle: u64,
    /// The DWG type: the spec label, or the class's DXF name for a custom class.
    pub label: String,
    /// The DXF record type AutoCAD exports it as (see [`dxf_key`]).
    pub dxf_key: Option<String>,
    pub decoded: DecodedEntity,
}

/// A loaded pair.
pub struct LoadedPair {
    pub name: String,
    pub version: Version,
    pub records: Vec<DwgRecord>,
    pub dxf: DxfDocument,
}

/// Opens both sides of `pair`. The DWG is walked by handle and every record
/// dispatched exactly as `DwgFile::decoded_entities` does, keeping the
/// handle next to the decode.
pub fn load(pair: &DrawingPair) -> Result<LoadedPair, String> {
    let name = pair
        .dwg
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let file = DwgFile::open(&pair.dwg).map_err(|e| format!("{name}: open: {e}"))?;
    let version = file.version();
    let raws = match file.all_objects() {
        Some(Ok(r)) => r,
        Some(Err(e)) => return Err(format!("{name}: object walk: {e}")),
        None => return Err(format!("{name}: no object stream")),
    };
    let class_map = file.class_map().and_then(Result::ok);
    let records = raws
        .iter()
        .map(|raw| {
            let decoded = match (class_map.as_ref(), raw.kind) {
                (Some(cm), ObjectType::Custom(code)) => {
                    decode_from_raw_with_class_map(raw, version, cm, code)
                }
                _ => decode_from_raw(raw, version),
            };
            let class_name = class_name(class_map.as_ref(), raw.kind);
            DwgRecord {
                handle: raw.handle.value,
                label: class_name
                    .clone()
                    .unwrap_or_else(|| raw.kind.short_label().to_string()),
                dxf_key: class_name.or_else(|| dxf_key(raw.kind).map(str::to_string)),
                decoded,
            }
        })
        .collect();
    let bytes = std::fs::read(&pair.dxf).map_err(|e| format!("{name}: read DXF: {e}"))?;
    let dxf = DxfDocument::parse(&bytes).map_err(|e| format!("{name}: DXF: {e}"))?;
    Ok(LoadedPair {
        name,
        version,
        records,
        dxf,
    })
}

fn class_name(class_map: Option<&ClassMap>, kind: ObjectType) -> Option<String> {
    match kind {
        ObjectType::Custom(code) => class_map?
            .by_type_code(code)
            .map(|c| c.dxf_class_name.clone()),
        _ => None,
    }
}

/// The DXF record type AutoCAD writes for a fixed DWG type (DXF Reference
/// entity, table and object names). A `*_CONTROL` object is the `TABLE`
/// record of its table, keyed `TABLE(<name>)` after the table's group 2.
/// `None` for types with no DXF record.
pub fn dxf_key(kind: ObjectType) -> Option<&'static str> {
    use ObjectType::*;
    Some(match kind {
        Text => "TEXT",
        Attrib => "ATTRIB",
        Attdef => "ATTDEF",
        Block => "BLOCK",
        EndBlk => "ENDBLK",
        SeqEnd => "SEQEND",
        Insert | MInsert => "INSERT",
        Vertex2d | Vertex3d | VertexMesh | VertexPface | VertexPfaceFace => "VERTEX",
        Polyline2d | Polyline3d | PolylinePface | PolylineMesh => "POLYLINE",
        Arc => "ARC",
        Circle => "CIRCLE",
        Line => "LINE",
        DimensionOrdinate | DimensionLinear | DimensionAligned | DimensionAng3Pt
        | DimensionAng2Ln | DimensionRadius | DimensionDiameter => "DIMENSION",
        Point => "POINT",
        Face3D => "3DFACE",
        Solid => "SOLID",
        Trace => "TRACE",
        Shape => "SHAPE",
        Viewport => "VIEWPORT",
        Ellipse => "ELLIPSE",
        Spline => "SPLINE",
        Region => "REGION",
        Solid3d => "3DSOLID",
        Body => "BODY",
        Ray => "RAY",
        XLine => "XLINE",
        Dictionary => "DICTIONARY",
        OleFrame => "OLEFRAME",
        MText => "MTEXT",
        Leader => "LEADER",
        Tolerance => "TOLERANCE",
        MLine => "MLINE",
        BlockControl => "TABLE(BLOCK_RECORD)",
        BlockHeader => "BLOCK_RECORD",
        LayerControl => "TABLE(LAYER)",
        Layer => "LAYER",
        StyleControl => "TABLE(STYLE)",
        Style => "STYLE",
        LtypeControl => "TABLE(LTYPE)",
        Ltype => "LTYPE",
        ViewControl => "TABLE(VIEW)",
        View => "VIEW",
        UcsControl => "TABLE(UCS)",
        Ucs => "UCS",
        VportControl => "TABLE(VPORT)",
        Vport => "VPORT",
        AppIdControl => "TABLE(APPID)",
        AppId => "APPID",
        DimStyleControl => "TABLE(DIMSTYLE)",
        DimStyle => "DIMSTYLE",
        Group => "GROUP",
        MLineStyle => "MLINESTYLE",
        Ole2Frame => "OLE2FRAME",
        LwPolyline => "LWPOLYLINE",
        Hatch => "HATCH",
        XRecord => "XRECORD",
        AcDbPlaceholder => "ACDBPLACEHOLDER",
        Layout => "LAYOUT",
        AcadProxyEntity => "ACAD_PROXY_ENTITY",
        AcadProxyObject => "ACAD_PROXY_OBJECT",
        Camera => "CAMERA",
        Sun => "SUN",
        Light => "LIGHT",
        GeoData => "GEODATA",
        _ => return None,
    })
}

/// A DXF record's key in the same vocabulary as [`dxf_key`].
pub fn record_key(r: &DxfRecord) -> String {
    if r.kind == "TABLE" {
        let name = r.first(2).and_then(|v| v.as_str()).unwrap_or("?");
        format!("TABLE({name})")
    } else {
        r.kind.clone()
    }
}

/// `R2013` for `Version::R2013`.
pub fn release(v: Version) -> String {
    format!("{v:?}")
}
