//! DWG object type enumeration (spec §20.3).
//!
//! Every object in the `AcDb:AcDbObjects` stream is prefixed with a type
//! code. Below 0x1F2 the codes are fixed; 0x1F2/0x1F3 are proxies; from
//! 500 upward the codes are *dynamic* — they index into the `AcDb:Classes`
//! table for custom or late-introduced entity types (IMAGE, TABLE,
//! MLEADER, etc., all get assigned on a per-file basis).

use std::fmt;

/// All fixed object type codes plus the two dynamic ranges (proxies +
/// class-indexed). `Custom(N)` is used for type codes ≥ 500; resolve
/// against the `AcDb:Classes` table to get the DXF class name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectType {
    Unused,
    Text,
    Attrib,
    Attdef,
    Block,
    EndBlk,
    SeqEnd,
    Insert,
    MInsert,
    /// 0x09 — unassigned (reserved).
    Unassigned09,
    Vertex2d,
    Vertex3d,
    VertexMesh,
    VertexPface,
    VertexPfaceFace,
    Polyline2d,
    Polyline3d,
    Arc,
    Circle,
    Line,
    DimensionOrdinate,
    DimensionLinear,
    DimensionAligned,
    DimensionAng3Pt,
    DimensionAng2Ln,
    DimensionRadius,
    DimensionDiameter,
    Point,
    Face3D,
    PolylinePface,
    PolylineMesh,
    Solid,
    Trace,
    Shape,
    Viewport,
    Ellipse,
    Spline,
    Region,
    Solid3d,
    Body,
    Ray,
    XLine,
    Dictionary,
    OleFrame,
    MText,
    Leader,
    Tolerance,
    MLine,
    BlockControl,
    BlockHeader,
    LayerControl,
    Layer,
    StyleControl,
    Style,
    /// 0x36 — unassigned (reserved).
    Unassigned36,
    /// 0x37 — unassigned (reserved).
    Unassigned37,
    LtypeControl,
    Ltype,
    /// 0x3A — unassigned (reserved).
    Unassigned3A,
    /// 0x3B — unassigned (reserved).
    Unassigned3B,
    ViewControl,
    View,
    UcsControl,
    Ucs,
    VportControl,
    Vport,
    AppIdControl,
    AppId,
    DimStyleControl,
    DimStyle,
    VpEntHdrCtrl,
    VpEntHdr,
    Group,
    MLineStyle,
    Ole2Frame,
    /// 0x4B — DUMMY (internal).
    Dummy,
    LongTransaction,
    LwPolyline,
    Hatch,
    XRecord,
    AcDbPlaceholder,
    VbaProject,
    Layout,
    /// 0x1F2 — proxy entity (carries unknown-class data as opaque bytes).
    AcadProxyEntity,
    /// 0x1F3 — proxy object (non-graphical variant of proxy entity).
    AcadProxyObject,
    /// 0x4F8 — CAMERA (R2007+). Technically a custom class whose
    /// real type code is assigned per-file via `AcDb:Classes`; the
    /// fixed code used here matches what the bundled dispatch path
    /// expects and keeps CAMERA out of the generic [`Self::Custom`]
    /// bucket when the file happens to use this value.
    Camera,
    /// 0x4F9 — SUN (R2007+). See [`Self::Camera`] on the fixed-code
    /// convention for these late-introduced entities.
    Sun,
    /// 0x4FA — LIGHT (R2007+). See [`Self::Camera`].
    Light,
    /// 0x4FB — GEODATA (R2010+). See [`Self::Camera`].
    GeoData,
    /// Dynamic class — type code ≥ 500 (other than the fixed
    /// visual/scene codes above). Resolve via `AcDb:Classes`:
    /// `class_index = type_code - 500`.
    Custom(u16),
    /// Type code that fell in a reserved range (shouldn't happen on
    /// valid files, but keeps the reader non-panicking on corrupt input).
    Unknown(u16),
}

impl ObjectType {
    pub fn from_code(code: u16) -> Self {
        match code {
            0x00 => Self::Unused,
            0x01 => Self::Text,
            0x02 => Self::Attrib,
            0x03 => Self::Attdef,
            0x04 => Self::Block,
            0x05 => Self::EndBlk,
            0x06 => Self::SeqEnd,
            0x07 => Self::Insert,
            0x08 => Self::MInsert,
            0x09 => Self::Unassigned09,
            0x0A => Self::Vertex2d,
            0x0B => Self::Vertex3d,
            0x0C => Self::VertexMesh,
            0x0D => Self::VertexPface,
            0x0E => Self::VertexPfaceFace,
            0x0F => Self::Polyline2d,
            0x10 => Self::Polyline3d,
            0x11 => Self::Arc,
            0x12 => Self::Circle,
            0x13 => Self::Line,
            0x14 => Self::DimensionOrdinate,
            0x15 => Self::DimensionLinear,
            0x16 => Self::DimensionAligned,
            0x17 => Self::DimensionAng3Pt,
            0x18 => Self::DimensionAng2Ln,
            0x19 => Self::DimensionRadius,
            0x1A => Self::DimensionDiameter,
            0x1B => Self::Point,
            0x1C => Self::Face3D,
            0x1D => Self::PolylinePface,
            0x1E => Self::PolylineMesh,
            0x1F => Self::Solid,
            0x20 => Self::Trace,
            0x21 => Self::Shape,
            0x22 => Self::Viewport,
            0x23 => Self::Ellipse,
            0x24 => Self::Spline,
            0x25 => Self::Region,
            0x26 => Self::Solid3d,
            0x27 => Self::Body,
            0x28 => Self::Ray,
            0x29 => Self::XLine,
            0x2A => Self::Dictionary,
            0x2B => Self::OleFrame,
            0x2C => Self::MText,
            0x2D => Self::Leader,
            0x2E => Self::Tolerance,
            0x2F => Self::MLine,
            0x30 => Self::BlockControl,
            0x31 => Self::BlockHeader,
            0x32 => Self::LayerControl,
            0x33 => Self::Layer,
            0x34 => Self::StyleControl,
            0x35 => Self::Style,
            0x36 => Self::Unassigned36,
            0x37 => Self::Unassigned37,
            0x38 => Self::LtypeControl,
            0x39 => Self::Ltype,
            0x3A => Self::Unassigned3A,
            0x3B => Self::Unassigned3B,
            0x3C => Self::ViewControl,
            0x3D => Self::View,
            0x3E => Self::UcsControl,
            0x3F => Self::Ucs,
            0x40 => Self::VportControl,
            0x41 => Self::Vport,
            0x42 => Self::AppIdControl,
            0x43 => Self::AppId,
            0x44 => Self::DimStyleControl,
            0x45 => Self::DimStyle,
            0x46 => Self::VpEntHdrCtrl,
            0x47 => Self::VpEntHdr,
            0x48 => Self::Group,
            0x49 => Self::MLineStyle,
            0x4A => Self::Ole2Frame,
            0x4B => Self::Dummy,
            0x4C => Self::LongTransaction,
            0x4D => Self::LwPolyline,
            0x4E => Self::Hatch,
            0x4F => Self::XRecord,
            0x50 => Self::AcDbPlaceholder,
            0x51 => Self::VbaProject,
            0x52 => Self::Layout,
            0x1F2 => Self::AcadProxyEntity,
            0x1F3 => Self::AcadProxyObject,
            0x4F8 => Self::Camera,
            0x4F9 => Self::Sun,
            0x4FA => Self::Light,
            0x4FB => Self::GeoData,
            code if code >= 500 => Self::Custom(code),
            code => Self::Unknown(code),
        }
    }

    /// True for entity types — things drawable on the canvas (LINE, CIRCLE,
    /// etc.) as opposed to objects (DICTIONARY, XRECORD, control objects).
    pub fn is_entity(self) -> bool {
        matches!(
            self,
            Self::Text
                | Self::Attrib
                | Self::Attdef
                | Self::Block
                | Self::EndBlk
                | Self::SeqEnd
                | Self::Insert
                | Self::MInsert
                | Self::Vertex2d
                | Self::Vertex3d
                | Self::VertexMesh
                | Self::VertexPface
                | Self::VertexPfaceFace
                | Self::Polyline2d
                | Self::Polyline3d
                | Self::Arc
                | Self::Circle
                | Self::Line
                | Self::DimensionOrdinate
                | Self::DimensionLinear
                | Self::DimensionAligned
                | Self::DimensionAng3Pt
                | Self::DimensionAng2Ln
                | Self::DimensionRadius
                | Self::DimensionDiameter
                | Self::Point
                | Self::Face3D
                | Self::PolylinePface
                | Self::PolylineMesh
                | Self::Solid
                | Self::Trace
                | Self::Shape
                | Self::Viewport
                | Self::Ellipse
                | Self::Spline
                | Self::Region
                | Self::Solid3d
                | Self::Body
                | Self::Ray
                | Self::XLine
                | Self::OleFrame
                | Self::MText
                | Self::Leader
                | Self::Tolerance
                | Self::MLine
                | Self::Ole2Frame
                | Self::LwPolyline
                | Self::Hatch
                | Self::AcadProxyEntity
                | Self::Camera
                | Self::Sun
                | Self::Light
                | Self::GeoData
        )
    }

    /// True for a symbol-table entry (Layer, LType, Style, ...).
    ///
    /// These objects are NOT drawing entities
    /// ([`Self::is_entity`] returns `false`) but they DO have a
    /// typed per-entry decoder in [`crate::tables`] and participate
    /// in dispatch via [`crate::entities::DecodedEntity::Layer`]
    /// and sibling variants.
    pub fn is_table_entry(self) -> bool {
        matches!(
            self,
            Self::Layer
                | Self::Style
                | Self::Ltype
                | Self::View
                | Self::Ucs
                | Self::Vport
                | Self::AppId
                | Self::DimStyle
                | Self::BlockHeader
        )
    }

    /// True for a *_CONTROL object (manages a symbol table).
    pub fn is_control(self) -> bool {
        matches!(
            self,
            Self::BlockControl
                | Self::LayerControl
                | Self::StyleControl
                | Self::LtypeControl
                | Self::ViewControl
                | Self::UcsControl
                | Self::VportControl
                | Self::AppIdControl
                | Self::DimStyleControl
                | Self::VpEntHdrCtrl
        )
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Unused => "UNUSED",
            Self::Text => "TEXT",
            Self::Attrib => "ATTRIB",
            Self::Attdef => "ATTDEF",
            Self::Block => "BLOCK",
            Self::EndBlk => "ENDBLK",
            Self::SeqEnd => "SEQEND",
            Self::Insert => "INSERT",
            Self::MInsert => "MINSERT",
            Self::Unassigned09 => "UNASSIGNED_09",
            Self::Vertex2d => "VERTEX_2D",
            Self::Vertex3d => "VERTEX_3D",
            Self::VertexMesh => "VERTEX_MESH",
            Self::VertexPface => "VERTEX_PFACE",
            Self::VertexPfaceFace => "VERTEX_PFACE_FACE",
            Self::Polyline2d => "POLYLINE_2D",
            Self::Polyline3d => "POLYLINE_3D",
            Self::Arc => "ARC",
            Self::Circle => "CIRCLE",
            Self::Line => "LINE",
            Self::DimensionOrdinate => "DIMENSION_ORDINATE",
            Self::DimensionLinear => "DIMENSION_LINEAR",
            Self::DimensionAligned => "DIMENSION_ALIGNED",
            Self::DimensionAng3Pt => "DIMENSION_ANG_3PT",
            Self::DimensionAng2Ln => "DIMENSION_ANG_2LN",
            Self::DimensionRadius => "DIMENSION_RADIUS",
            Self::DimensionDiameter => "DIMENSION_DIAMETER",
            Self::Point => "POINT",
            Self::Face3D => "3DFACE",
            Self::PolylinePface => "POLYLINE_PFACE",
            Self::PolylineMesh => "POLYLINE_MESH",
            Self::Solid => "SOLID",
            Self::Trace => "TRACE",
            Self::Shape => "SHAPE",
            Self::Viewport => "VIEWPORT",
            Self::Ellipse => "ELLIPSE",
            Self::Spline => "SPLINE",
            Self::Region => "REGION",
            Self::Solid3d => "3DSOLID",
            Self::Body => "BODY",
            Self::Ray => "RAY",
            Self::XLine => "XLINE",
            Self::Dictionary => "DICTIONARY",
            Self::OleFrame => "OLEFRAME",
            Self::MText => "MTEXT",
            Self::Leader => "LEADER",
            Self::Tolerance => "TOLERANCE",
            Self::MLine => "MLINE",
            Self::BlockControl => "BLOCK_CONTROL",
            Self::BlockHeader => "BLOCK_HEADER",
            Self::LayerControl => "LAYER_CONTROL",
            Self::Layer => "LAYER",
            Self::StyleControl => "STYLE_CONTROL",
            Self::Style => "STYLE",
            Self::Unassigned36 => "UNASSIGNED_36",
            Self::Unassigned37 => "UNASSIGNED_37",
            Self::LtypeControl => "LTYPE_CONTROL",
            Self::Ltype => "LTYPE",
            Self::Unassigned3A => "UNASSIGNED_3A",
            Self::Unassigned3B => "UNASSIGNED_3B",
            Self::ViewControl => "VIEW_CONTROL",
            Self::View => "VIEW",
            Self::UcsControl => "UCS_CONTROL",
            Self::Ucs => "UCS",
            Self::VportControl => "VPORT_CONTROL",
            Self::Vport => "VPORT",
            Self::AppIdControl => "APPID_CONTROL",
            Self::AppId => "APPID",
            Self::DimStyleControl => "DIMSTYLE_CONTROL",
            Self::DimStyle => "DIMSTYLE",
            Self::VpEntHdrCtrl => "VP_ENT_HDR_CTRL",
            Self::VpEntHdr => "VP_ENT_HDR",
            Self::Group => "GROUP",
            Self::MLineStyle => "MLINESTYLE",
            Self::Ole2Frame => "OLE2FRAME",
            Self::Dummy => "DUMMY",
            Self::LongTransaction => "LONG_TRANSACTION",
            Self::LwPolyline => "LWPOLYLINE",
            Self::Hatch => "HATCH",
            Self::XRecord => "XRECORD",
            Self::AcDbPlaceholder => "ACDB_PLACEHOLDER",
            Self::VbaProject => "VBA_PROJECT",
            Self::Layout => "LAYOUT",
            Self::AcadProxyEntity => "ACAD_PROXY_ENTITY",
            Self::AcadProxyObject => "ACAD_PROXY_OBJECT",
            Self::Camera => "CAMERA",
            Self::Sun => "SUN",
            Self::Light => "LIGHT",
            Self::GeoData => "GEODATA",
            Self::Custom(_) => "CUSTOM",
            Self::Unknown(_) => "UNKNOWN",
        }
    }
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Custom(n) => write!(f, "CUSTOM({n})"),
            Self::Unknown(n) => write!(f, "UNKNOWN(0x{n:x})"),
            other => f.write_str(other.short_label()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_codes_decode() {
        assert_eq!(ObjectType::from_code(0x13), ObjectType::Line);
        assert_eq!(ObjectType::from_code(0x12), ObjectType::Circle);
        assert_eq!(ObjectType::from_code(0x11), ObjectType::Arc);
        assert_eq!(ObjectType::from_code(0x4D), ObjectType::LwPolyline);
        assert_eq!(ObjectType::from_code(0x4E), ObjectType::Hatch);
    }

    #[test]
    fn proxy_codes_decode() {
        assert_eq!(ObjectType::from_code(0x1F2), ObjectType::AcadProxyEntity);
        assert_eq!(ObjectType::from_code(0x1F3), ObjectType::AcadProxyObject);
    }

    #[test]
    fn dynamic_codes_decode() {
        assert_eq!(ObjectType::from_code(500), ObjectType::Custom(500));
        assert_eq!(ObjectType::from_code(12345), ObjectType::Custom(12345));
    }

    #[test]
    fn entity_classification() {
        assert!(ObjectType::Line.is_entity());
        assert!(ObjectType::Circle.is_entity());
        assert!(ObjectType::LwPolyline.is_entity());
        assert!(!ObjectType::Dictionary.is_entity());
        assert!(!ObjectType::LayerControl.is_entity());
    }

    #[test]
    fn control_classification() {
        assert!(ObjectType::LayerControl.is_control());
        assert!(ObjectType::BlockControl.is_control());
        assert!(!ObjectType::Layer.is_control());
        assert!(!ObjectType::Line.is_control());
    }

    #[test]
    fn short_label_stable() {
        assert_eq!(ObjectType::Line.short_label(), "LINE");
        assert_eq!(ObjectType::LwPolyline.short_label(), "LWPOLYLINE");
        assert_eq!(ObjectType::MText.short_label(), "MTEXT");
    }
}
