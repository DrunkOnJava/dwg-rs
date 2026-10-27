//! The dynamic-block evaluation objects: BLOCKGRIPLOCATIONCOMPONENT,
//! BLOCKLINEARGRIP, BLOCKVISIBILITYGRIP, BLOCKLINEARPARAMETER,
//! BLOCKVISIBILITYPARAMETER and BLOCKSCALEACTION.
//!
//! ODA spec v5.4.1 prints no layout for any of them. The field lists are
//! derived from AutoCAD's own output under the method in `CLEANROOM.md`
//! (2026-10-26): the data bits of the 84 corpus records (`sample_base` saved as
//! R14 to R2018) against the `AcDbEvalExpr` family AutoCAD exported to DXF for
//! the same handles. ARCHITECTURE.md §7aw holds the evidence.
//!
//! The classes share a chain of base classes, written base first:
//!
//! ```text
//! AcDbEvalExpr:      BL unknown (-1), BS group 98, BL group 99,
//!                    BS value type (group 70: 40 = a real, -9999 = none),
//!                    BD group 140 when the type is 40, BS group 90 (node id)
//! AcDbBlockElement:  TV group 300, BS group 98, BL group 99, BS group 1071
//!                    (the expression-valued GRIPLOCATIONCOMPONENT has BS 91 and
//!                    TV 300 instead)
//! AcDbBlockGrip:     BL group 91, BL group 92, BD x3 (1010), B group 280,
//!                    BL group 93 (-1)
//! AcDbBlockParameter: B group 280, B group 281
//! ```
//!
//! then each class's own tail, listed on its struct. The second pair of group
//! 98 and 99 is 33 and 329 from R2007 and 25 and 104 on R14 to R2004, as the
//! DXF twins write them. Handles (groups 330, 331, 332, 333) live in the handle
//! stream and are not decoded, and a list whose length the stream does not
//! hold (the linear parameter's four properties, group 170) is read at the
//! length every corpus record has.
//!
//! Every small integer is read in the two short `BS` forms the corpus holds; a
//! wider form, an expression type other than 40 or -9999, and a count above
//! 4096 are refused with [`crate::error::Error::Unsupported`] or
//! [`crate::error::Error::SectionMap`], never read on a guess.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::objects::small_classes::{open_and_finish, read_small, unsupported};
use crate::string_stream::StringReader;
use crate::version::Version;

const MAX_STATES: u32 = 4096;

type Strings<'a> = Option<StringReader<'a>>;

/// The `AcDbEvalExpr` base: every class starts with it.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalExpr {
    /// `BL`, -1 in every record: unverified by value, no DXF group shows it.
    pub unknown_a: i32,
    /// `BS`, DXF group 98 of `AcDbEvalExpr` (33).
    pub dxf_98: u8,
    /// `BL`, DXF group 99 of `AcDbEvalExpr` (329).
    pub dxf_99: u32,
    /// `BS` value type: 40 for a real (DXF group 70), -9999 for none.
    pub value_type: i16,
    /// `BD` DXF group 140, present only when the value type is 40.
    pub value: Option<f64>,
    /// `BS`, DXF group 90 (the node id).
    pub node_id: u8,
}

/// The `AcDbBlockElement` base of every class but the expression component.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockElement {
    /// `TV`, DXF group 300.
    pub name: String,
    /// `BS`, DXF group 98 of the element: 33 from R2007, 25 before.
    pub dxf_98: u8,
    /// `BL`, DXF group 99 of the element: 329 from R2007, 104 before.
    pub dxf_99: u32,
    /// `BS`, DXF group 1071 (0).
    pub dxf_1071: u8,
}

/// The `AcDbBlockGrip` part of a grip.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockGrip {
    /// `BL`, DXF group 91.
    pub dxf_91: u32,
    /// `BL`, DXF group 92.
    pub dxf_92: u32,
    /// `BD` x3, DXF groups 1010, 1020, 1030.
    pub position: [f64; 3],
    /// `B`, DXF group 280.
    pub dxf_280: bool,
    /// `BL`, DXF group 93 (-1).
    pub dxf_93: i32,
}

/// BLOCKGRIPLOCATIONCOMPONENT: an expression naming a grip's updated
/// coordinate.
#[derive(Debug, Clone, PartialEq)]
pub struct GripLocationComponent {
    pub eval: EvalExpr,
    /// `BS`, DXF group 91.
    pub dxf_91: u8,
    /// `TV`, DXF group 300.
    pub name: String,
}

/// BLOCKLINEARGRIP.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearGrip {
    pub eval: EvalExpr,
    pub element: BlockElement,
    pub grip: BlockGrip,
    /// `BD` x3, DXF groups 140, 141, 142.
    pub offset: [f64; 3],
}

/// BLOCKVISIBILITYGRIP.
#[derive(Debug, Clone, PartialEq)]
pub struct VisibilityGrip {
    pub eval: EvalExpr,
    pub element: BlockElement,
    pub grip: BlockGrip,
}

/// One of the four property entries of a linear parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearProperty {
    /// `BL`, DXF group 171 to 174.
    pub flag: u8,
    /// `BL`, DXF group 92 to 95.
    pub value: u8,
    /// `TV`, DXF group 301 to 304.
    pub name: String,
}

/// BLOCKLINEARPARAMETER.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearParameter {
    pub eval: EvalExpr,
    pub element: BlockElement,
    /// `B`, DXF group 280 of `AcDbBlockParameter`.
    pub dxf_280: bool,
    /// `B`, DXF group 281 of `AcDbBlockParameter`.
    pub dxf_281: bool,
    /// `BD` x3, DXF groups 1010, 1020, 1030.
    pub point1: [f64; 3],
    /// `BD` x3, DXF groups 1011, 1021, 1031.
    pub point2: [f64; 3],
    /// Four entries; DXF group 170 (4) is not stored and is read as 4.
    pub properties: Vec<LinearProperty>,
    /// `BL` x4, DXF group 91 (listed in the DXF before the entries).
    pub dxf_91: [u8; 4],
    /// `BL`, DXF group 177.
    pub dxf_177: u8,
    /// `TV`, DXF group 305.
    pub name: String,
    /// `TV`, DXF group 306.
    pub description: String,
    /// `BD`, DXF group 140.
    pub dxf_140: f64,
    /// `BL`, DXF group 96.
    pub dxf_96: u8,
    /// `BD` x3, DXF groups 141, 142, 143.
    pub dxf_141_to_143: [f64; 3],
    /// `BL`, DXF group 175.
    pub dxf_175: u8,
}

/// One state of a visibility parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct VisibilityState {
    /// `TV`, DXF group 303.
    pub name: String,
    /// `BL`, DXF group 94 (the count of group 332 handles).
    pub dxf_94: u8,
    /// `BL`, DXF group 95 (the count of group 333 handles).
    pub dxf_95: u8,
}

/// BLOCKVISIBILITYPARAMETER.
#[derive(Debug, Clone, PartialEq)]
pub struct VisibilityParameter {
    pub eval: EvalExpr,
    pub element: BlockElement,
    /// `B`, DXF group 280 of `AcDbBlockParameter`.
    pub dxf_280: bool,
    /// `B`, DXF group 281 of `AcDbBlockParameter`.
    pub dxf_281: bool,
    /// `BD` x3, DXF groups 1010, 1020, 1030.
    pub position: [f64; 3],
    /// `BS` x2 after the position, both 0: DXF groups 170 and 171, in an order
    /// the corpus cannot tell apart.
    pub dxf_170_171: [u8; 2],
    /// `BL`, DXF group 93 (the first one).
    pub dxf_93: u8,
    /// `B`, DXF group 281 of the visibility parameter.
    pub dxf_281_visibility: bool,
    /// `TV`, DXF group 301.
    pub name: String,
    /// `TV`, DXF group 302.
    pub description: String,
    /// `B`, DXF group 91.
    pub dxf_91: bool,
    /// `BL`, DXF group 93 (the second one).
    pub dxf_93_second: u8,
    /// The states; their count is DXF group 92.
    pub states: Vec<VisibilityState>,
}

/// BLOCKSCALEACTION.
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleAction {
    pub eval: EvalExpr,
    pub element: BlockElement,
    /// `BD` x3, DXF groups 1010, 1020, 1030.
    pub position: [f64; 3],
    /// `BS`, DXF group 71.
    pub dxf_71: u8,
    /// `BS`, DXF group 70.
    pub dxf_70: u8,
    /// `BD` x3, DXF groups 1011, 1021, 1031.
    pub base_point: [f64; 3],
    /// `BL`, DXF group 92.
    pub dxf_92: u8,
    /// `TV`, DXF group 301.
    pub name_x: String,
    /// `BL`, DXF group 93.
    pub dxf_93: u8,
    /// `TV`, DXF group 302.
    pub name_y: String,
    /// `B`, DXF group 280.
    pub dxf_280: bool,
    /// `BD` x3, DXF groups 1012, 1022, 1032.
    pub second_point: [f64; 3],
    /// `BL`, DXF group 94.
    pub dxf_94: u8,
    /// `TV`, DXF group 303.
    pub name_scale: String,
    /// `BL`, DXF group 95.
    pub dxf_95: u8,
    /// `TV`, DXF group 304.
    pub name_x_scale: String,
    /// `BL`, DXF group 96.
    pub dxf_96: u8,
    /// `TV`, DXF group 305.
    pub name_y_scale: String,
    /// `RC` after the last string, 0 in every record: unverified by value.
    pub unknown_trailing: u8,
}

/// A decoded dynamic-block evaluation object.
#[derive(Debug, Clone, PartialEq)]
pub enum DynamicBlockObject {
    GripLocationComponent(GripLocationComponent),
    LinearGrip(LinearGrip),
    VisibilityGrip(VisibilityGrip),
    LinearParameter(Box<LinearParameter>),
    VisibilityParameter(Box<VisibilityParameter>),
    ScaleAction(Box<ScaleAction>),
}

fn read_point(c: &mut BitCursor<'_>) -> Result<[f64; 3]> {
    Ok([c.read_bd()?, c.read_bd()?, c.read_bd()?])
}

fn read_eval(c: &mut BitCursor<'_>) -> Result<EvalExpr> {
    let unknown_a = c.read_bl()?;
    let dxf_98 = read_small(c, "evaluation group 98")?;
    let dxf_99 = c.read_bl_u()?;
    let value_type = c.read_bs()?;
    let value = match value_type {
        40 => Some(c.read_bd()?),
        -9999 => None,
        _ => return Err(unsupported("evaluation expression value type")),
    };
    let node_id = read_small(c, "evaluation node id")?;
    Ok(EvalExpr {
        unknown_a,
        dxf_98,
        dxf_99,
        value_type,
        value,
        node_id,
    })
}

fn read_element(
    c: &mut BitCursor<'_>,
    strings: &mut Strings<'_>,
    version: Version,
) -> Result<BlockElement> {
    let name = modern::read_tv(c, strings, version)?;
    let dxf_98 = read_small(c, "element group 98")?;
    let dxf_99 = c.read_bl_u()?;
    let dxf_1071 = read_small(c, "element group 1071")?;
    Ok(BlockElement {
        name,
        dxf_98,
        dxf_99,
        dxf_1071,
    })
}

fn read_grip(c: &mut BitCursor<'_>) -> Result<BlockGrip> {
    let dxf_91 = c.read_bl_u()?;
    let dxf_92 = c.read_bl_u()?;
    let position = read_point(c)?;
    let dxf_280 = c.read_b()?;
    let dxf_93 = c.read_bl()?;
    Ok(BlockGrip {
        dxf_91,
        dxf_92,
        position,
        dxf_280,
        dxf_93,
    })
}

/// The eval and element bases, then the element-based tail of `class`.
fn read_body(
    class: &str,
    c: &mut BitCursor<'_>,
    strings: &mut Strings<'_>,
    version: Version,
) -> Result<DynamicBlockObject> {
    let eval = read_eval(c)?;
    if class == "BLOCKGRIPLOCATIONCOMPONENT" {
        let dxf_91 = read_small(c, "group 91")?;
        let name = modern::read_tv(c, strings, version)?;
        return Ok(DynamicBlockObject::GripLocationComponent(
            GripLocationComponent { eval, dxf_91, name },
        ));
    }
    let element = read_element(c, strings, version)?;
    Ok(match class {
        "BLOCKLINEARGRIP" => {
            let grip = read_grip(c)?;
            let offset = read_point(c)?;
            DynamicBlockObject::LinearGrip(LinearGrip {
                eval,
                element,
                grip,
                offset,
            })
        }
        "BLOCKVISIBILITYGRIP" => {
            let grip = read_grip(c)?;
            DynamicBlockObject::VisibilityGrip(VisibilityGrip {
                eval,
                element,
                grip,
            })
        }
        "BLOCKLINEARPARAMETER" => {
            let dxf_280 = c.read_b()?;
            let dxf_281 = c.read_b()?;
            let point1 = read_point(c)?;
            let point2 = read_point(c)?;
            let mut properties = Vec::with_capacity(4);
            for _ in 0..4 {
                let flag = read_small(c, "linear property flag")?;
                let value = read_small(c, "linear property value")?;
                let name = modern::read_tv(c, strings, version)?;
                properties.push(LinearProperty { flag, value, name });
            }
            let mut dxf_91 = [0u8; 4];
            for v in &mut dxf_91 {
                *v = read_small(c, "group 91")?;
            }
            let dxf_177 = read_small(c, "group 177")?;
            let name = modern::read_tv(c, strings, version)?;
            let description = modern::read_tv(c, strings, version)?;
            let dxf_140 = c.read_bd()?;
            let dxf_96 = read_small(c, "group 96")?;
            let dxf_141_to_143 = read_point(c)?;
            let dxf_175 = read_small(c, "group 175")?;
            DynamicBlockObject::LinearParameter(Box::new(LinearParameter {
                eval,
                element,
                dxf_280,
                dxf_281,
                point1,
                point2,
                properties,
                dxf_91,
                dxf_177,
                name,
                description,
                dxf_140,
                dxf_96,
                dxf_141_to_143,
                dxf_175,
            }))
        }
        "BLOCKVISIBILITYPARAMETER" => {
            let dxf_280 = c.read_b()?;
            let dxf_281 = c.read_b()?;
            let position = read_point(c)?;
            let first = read_small(c, "group 170")?;
            let second = read_small(c, "group 171")?;
            let dxf_93 = read_small(c, "group 93")?;
            let dxf_281_visibility = c.read_b()?;
            let name = modern::read_tv(c, strings, version)?;
            let description = modern::read_tv(c, strings, version)?;
            let dxf_91 = c.read_b()?;
            let dxf_93_second = read_small(c, "group 93")?;
            let count = c.read_bl_u()?;
            if count > MAX_STATES {
                return Err(Error::SectionMap(format!(
                    "BLOCKVISIBILITYPARAMETER state count {count} exceeds cap {MAX_STATES}"
                )));
            }
            let mut states = Vec::new();
            for _ in 0..count {
                let name = modern::read_tv(c, strings, version)?;
                let dxf_94 = read_small(c, "group 94")?;
                let dxf_95 = read_small(c, "group 95")?;
                states.push(VisibilityState {
                    name,
                    dxf_94,
                    dxf_95,
                });
            }
            DynamicBlockObject::VisibilityParameter(Box::new(VisibilityParameter {
                eval,
                element,
                dxf_280,
                dxf_281,
                position,
                dxf_170_171: [first, second],
                dxf_93,
                dxf_281_visibility,
                name,
                description,
                dxf_91,
                dxf_93_second,
                states,
            }))
        }
        "BLOCKSCALEACTION" => {
            let position = read_point(c)?;
            let dxf_71 = read_small(c, "group 71")?;
            let dxf_70 = read_small(c, "group 70")?;
            let base_point = read_point(c)?;
            let dxf_92 = read_small(c, "group 92")?;
            let name_x = modern::read_tv(c, strings, version)?;
            let dxf_93 = read_small(c, "group 93")?;
            let name_y = modern::read_tv(c, strings, version)?;
            let dxf_280 = c.read_b()?;
            let second_point = read_point(c)?;
            let dxf_94 = read_small(c, "group 94")?;
            let name_scale = modern::read_tv(c, strings, version)?;
            let dxf_95 = read_small(c, "group 95")?;
            let name_x_scale = modern::read_tv(c, strings, version)?;
            let dxf_96 = read_small(c, "group 96")?;
            let name_y_scale = modern::read_tv(c, strings, version)?;
            let unknown_trailing = c.read_rc()?;
            if unknown_trailing != 0 {
                return Err(unsupported("scale action trailing byte"));
            }
            DynamicBlockObject::ScaleAction(Box::new(ScaleAction {
                eval,
                element,
                position,
                dxf_71,
                dxf_70,
                base_point,
                dxf_92,
                name_x,
                dxf_93,
                name_y,
                dxf_280,
                second_point,
                dxf_94,
                name_scale,
                dxf_95,
                name_x_scale,
                dxf_96,
                name_y_scale,
                unknown_trailing,
            }))
        }
        _ => return Err(unsupported("dynamic block class")),
    })
}

/// Decode one dynamic-block evaluation object named by its DXF class `class`,
/// from the layout of the module docs (derived from AutoCAD's output,
/// ARCHITECTURE.md §7aw), ending exactly on the data-stream boundary and, from
/// R2007, with every string-stream entry consumed.
pub(crate) fn decode_object(
    class: &str,
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<DynamicBlockObject> {
    open_and_finish(
        class,
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| read_body(class, c, strings, version),
    )
}
