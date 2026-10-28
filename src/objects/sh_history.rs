//! ACSH_HISTORY_CLASS, ACSH_BOX_CLASS and ACSH_EXTRUSION_CLASS: the solid-history
//! records AutoCAD writes for a primitive solid and for an extrusion.
//!
//! ODA spec v5.4.1 prints no layout for either. The field lists are derived from
//! AutoCAD's own output under the method in `CLEANROOM.md` (2026-10-26): the data
//! bits of the 21 corpus records (`sample_base` saved as R14 to R2018) against
//! the DXF object of the same handle. ARCHITECTURE.md §7bj holds the evidence.
//!
//! ```text
//! ACSH_HISTORY_CLASS   BL group 90 (33), BL group 91 (329), BL group 92 (1),
//!                      B group 280, B group 281
//! ACSH_BOX_CLASS       AcDbEvalExpr (as in `objects::dynamic_block`),
//!                      BL group 90, BL group 91,
//!                      BD x16 transform (groups 40 to 55),
//!                      colour (R2004+: BS, BL colour word, RC flags and the TVs
//!                      they name; before: one BS, 256 = ByLayer, group 62),
//!                      BL group 92,
//!                      BL group 90, BL group 91 of the box, BD x3 (40, 41, 42)
//! ACSH_EXTRUSION_CLASS the same evaluation base and node (BL 92 included), then the
//!                      sweep base: BL 90, BL 91, 3BD (10, 20, 30), BD x6 (42 to 45,
//!                      48, 49), B 290, BS 70, BS 71, B x5 (292 to 296), BD x3 (11, 21,
//!                      31), BD x16 (46), BD x16 (47), BL 92, BL 90 (a bit count) with
//!                      that many bits (group 310), BL 93
//! ```
//!
//! The DXF writes the last six of the 16 transform values (groups 50 to 55) in
//! degrees; the decoded values are as stored, so the probe converts. Handles
//! (groups 360 and 347) live in the handle stream and are not decoded. A colour
//! flag above 3, and an integer wider than the short forms the corpus holds, are
//! refused with [`crate::error::Error::Unsupported`]. ACSH_EXTRUSION_CLASS follows the
//! same evaluation base and node, then the sweep base with its profile data and two
//! more matrices (ARCHITECTURE.md §7bp).
//!
//! ACSH_CYLINDER_CLASS and ACSH_REVOLVE_CLASS follow the same evaluation base and node
//! (ARCHITECTURE.md §7bq). Their field lists are derived from the bits of the three
//! records of one R14 drawing and held to the record boundary alone: the corpus holds no
//! DXF export of that drawing, so no value is scored.
//!
//! ```text
//! ACSH_CYLINDER_CLASS  the evaluation base and node of ACSH_BOX_CLASS (BL 92 included),
//!                      BL 90, BL 91 of the cylinder, BD x4
//! ACSH_REVOLVE_CLASS   the same base and node, BL 90, BL 91, 3BD, 3BD, BD (2 pi), six
//!                      values stored in the zero form of BD, BL or BS, BL 92 (77),
//!                      BL bit count (1272) and that many bits
//! ```

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::dynamic_block::{EvalExpr, read_eval};
use crate::objects::modern;
use crate::objects::small_classes::{open_and_finish, unsupported};
use crate::string_stream::StringReader;
use crate::version::Version;

/// ACSH_HISTORY_CLASS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShHistory {
    /// `BL`, DXF group 90 (33).
    pub dxf_90: u32,
    /// `BL`, DXF group 91 (329).
    pub dxf_91: u32,
    /// `BL`, DXF group 92 (1).
    pub dxf_92: u32,
    /// `B`, DXF group 280.
    pub dxf_280: bool,
    /// `B`, DXF group 281.
    pub dxf_281: bool,
}

/// The colour of a history node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShColor {
    /// `BS`: before R2004 the whole colour (256 = ByLayer, DXF group 62), from
    /// R2004 the leading `BS` of the colour, 0 in the corpus.
    pub index: i16,
    /// `BL` colour word from R2004 (0xC0000000 in the corpus, where the DXF
    /// writes group 62 = 256).
    pub word: Option<u32>,
    /// `TV` colour name when flag bit 0 is set, from R2004.
    pub name: Option<String>,
    /// `TV` colour book when flag bit 1 is set, from R2004.
    pub book: Option<String>,
}

/// ACSH_BOX_CLASS.
#[derive(Debug, Clone, PartialEq)]
pub struct ShBox {
    pub eval: EvalExpr,
    /// `BL`, DXF group 90 of the history node (33).
    pub node_90: u32,
    /// `BL`, DXF group 91 of the history node (329).
    pub node_91: u32,
    /// `BD` x16, DXF groups 40 to 55 (the last six in degrees in the DXF).
    pub transform: [f64; 16],
    pub color: ShColor,
    /// `BL`, DXF group 92 (3).
    pub node_92: u32,
    /// `BL`, DXF group 90 of the box (33).
    pub box_90: u32,
    /// `BL`, DXF group 91 of the box (329).
    pub box_91: u32,
    /// `BD` x3, DXF groups 40, 41 and 42 of the box.
    pub size: [f64; 3],
}

pub(crate) fn read_color(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<ShColor> {
    let index = c.read_bs()?;
    if !version.is_r2004_plus() {
        return Ok(ShColor {
            index,
            word: None,
            name: None,
            book: None,
        });
    }
    let word = c.read_bl_u()?;
    let flags = c.read_rc()?;
    if flags > 3 {
        return Err(unsupported("ACSH colour flag"));
    }
    let name = if flags & 1 != 0 {
        Some(modern::read_tv(c, strings, version)?)
    } else {
        None
    };
    let book = if flags & 2 != 0 {
        Some(modern::read_tv(c, strings, version)?)
    } else {
        None
    };
    Ok(ShColor {
        index,
        word: Some(word),
        name,
        book,
    })
}

/// Decode an ACSH_HISTORY_CLASS record: three `BL` and two `B` (DXF groups 90, 91,
/// 92, 280, 281), derived from AutoCAD's output (ARCHITECTURE.md §7bj), ending on
/// the data-stream boundary.
pub(crate) fn decode_history(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<ShHistory> {
    open_and_finish(
        "ACSH_HISTORY_CLASS",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, _| {
            Ok(ShHistory {
                dxf_90: c.read_bl_u()?,
                dxf_91: c.read_bl_u()?,
                dxf_92: c.read_bl_u()?,
                dxf_280: c.read_b()?,
                dxf_281: c.read_b()?,
            })
        },
    )
}

/// Decode an ACSH_BOX_CLASS record from the layout of the module docs (derived
/// from AutoCAD's output, ARCHITECTURE.md §7bj), ending exactly on the
/// data-stream boundary and, from R2007, with the string stream consumed.
pub(crate) fn decode_box(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<ShBox> {
    open_and_finish(
        "ACSH_BOX_CLASS",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            let eval = read_eval(c)?;
            let node_90 = c.read_bl_u()?;
            let node_91 = c.read_bl_u()?;
            let mut transform = [0.0; 16];
            for v in &mut transform {
                *v = c.read_bd()?;
            }
            let color = read_color(c, strings, version)?;
            let node_92 = c.read_bl_u()?;
            let box_90 = c.read_bl_u()?;
            let box_91 = c.read_bl_u()?;
            let size = [c.read_bd()?, c.read_bd()?, c.read_bd()?];
            Ok(ShBox {
                eval,
                node_90,
                node_91,
                transform,
                color,
                node_92,
                box_90,
                box_91,
                size,
            })
        },
    )
}

/// ACSH_EXTRUSION_CLASS: a solid made by extruding a profile.
#[derive(Debug, Clone, PartialEq)]
pub struct ShExtrusion {
    pub eval: EvalExpr,
    /// `BL`, DXF group 90 of the history node (33).
    pub node_90: u32,
    /// `BL`, DXF group 91 of the history node (329).
    pub node_91: u32,
    /// `BD` x16, DXF groups 40 to 55 (the last six in degrees in the DXF).
    pub transform: [f64; 16],
    pub color: ShColor,
    /// `BL`, DXF group 92 of the history node (4).
    pub node_92: u32,
    /// `BL`, DXF group 90 of the sweep base (33).
    pub sweep_90: u32,
    /// `BL`, DXF group 91 of the sweep base (329).
    pub sweep_91: u32,
    /// `3BD`, DXF groups 10, 20 and 30.
    pub point: [f64; 3],
    /// `BD` x6, in the order of DXF groups 42, 43, 44, 45, 48 and 49: 0, 0, 0, 0, 1.0 and
    /// 0. Only the 1.0 (group 48) is told apart by value from the zeros.
    pub scalars: [f64; 6],
    /// `B`, DXF group 290.
    pub dxf_290: bool,
    /// `BS`, DXF group 70.
    pub dxf_70: i16,
    /// `BS`, DXF group 71.
    pub dxf_71: i16,
    /// `B` x5, in the order of DXF groups 292 to 296: 1, 0, 0, 1, 0.
    pub flags: [bool; 5],
    /// `BD` x3, DXF groups 11, 21 and 31 (0.0 in every record).
    pub vector: [f64; 3],
    /// `BD` x16, DXF groups 46.
    pub matrix_46: [f64; 16],
    /// `BD` x16, DXF groups 47.
    pub matrix_47: [f64; 16],
    /// `BL`, DXF group 92 of the sweep base (77).
    pub dxf_92: u32,
    /// The profile data, DXF groups 90 and 310: a `BL` bit count (672) and that many bits,
    /// read as bytes. A count that is not a multiple of 8 is refused.
    pub profile: Vec<u8>,
    /// `BL`, DXF group 93 (0).
    pub dxf_93: u32,
}

fn read_array<const N: usize>(c: &mut BitCursor<'_>) -> Result<[f64; N]> {
    let mut out = [0.0; N];
    for v in &mut out {
        *v = c.read_bd()?;
    }
    Ok(out)
}

/// Decode an ACSH_EXTRUSION_CLASS record from the layout of the module docs (derived
/// from AutoCAD's output, ARCHITECTURE.md §7bp), ending exactly on the data-stream
/// boundary and, from R2007, with the string stream consumed.
pub(crate) fn decode_extrusion(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<ShExtrusion> {
    open_and_finish(
        "ACSH_EXTRUSION_CLASS",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            let eval = read_eval(c)?;
            let node_90 = c.read_bl_u()?;
            let node_91 = c.read_bl_u()?;
            let transform = read_array::<16>(c)?;
            let color = read_color(c, strings, version)?;
            let node_92 = c.read_bl_u()?;
            let sweep_90 = c.read_bl_u()?;
            let sweep_91 = c.read_bl_u()?;
            let point = read_array::<3>(c)?;
            let scalars = read_array::<6>(c)?;
            let dxf_290 = c.read_b()?;
            let dxf_70 = c.read_bs()?;
            let dxf_71 = c.read_bs()?;
            let flags = [
                c.read_b()?,
                c.read_b()?,
                c.read_b()?,
                c.read_b()?,
                c.read_b()?,
            ];
            let vector = read_array::<3>(c)?;
            let matrix_46 = read_array::<16>(c)?;
            let matrix_47 = read_array::<16>(c)?;
            let dxf_92 = c.read_bl_u()?;
            let bits = c.read_bl_u()? as usize;
            if bits % 8 != 0 {
                return Err(Error::Unsupported {
                    feature: "ACSH_EXTRUSION_CLASS profile bit count that is not a multiple of 8"
                        .into(),
                });
            }
            if bits > c.remaining_bits() {
                return Err(Error::SectionMap(format!(
                    "ACSH_EXTRUSION_CLASS profile of {bits} bits exceeds the {} bits left in the record",
                    c.remaining_bits()
                )));
            }
            let mut profile = Vec::with_capacity(bits / 8);
            for _ in 0..bits / 8 {
                profile.push(c.read_rc()?);
            }
            let dxf_93 = c.read_bl_u()?;
            Ok(ShExtrusion {
                eval,
                node_90,
                node_91,
                transform,
                color,
                node_92,
                sweep_90,
                sweep_91,
                point,
                scalars,
                dxf_290,
                dxf_70,
                dxf_71,
                flags,
                vector,
                matrix_46,
                matrix_47,
                dxf_92,
                profile,
                dxf_93,
            })
        },
    )
}

/// ACSH_CYLINDER_CLASS: a solid cylinder. Held to its record boundary alone; no DXF twin
/// scores any value (ARCHITECTURE.md §7bq).
#[derive(Debug, Clone, PartialEq)]
pub struct ShCylinder {
    pub eval: EvalExpr,
    /// `BL`, group 90 of the history node as ACSH_BOX_CLASS has it (33).
    pub node_90: u32,
    /// `BL`, group 91 of the history node as ACSH_BOX_CLASS has it (73 in the corpus).
    pub node_91: u32,
    /// `BD` x16, the transform as ACSH_BOX_CLASS has it.
    pub transform: [f64; 16],
    pub color: ShColor,
    /// `BL`, group 92 of the history node as ACSH_BOX_CLASS has it (6 and 7 in the corpus).
    pub node_92: u32,
    /// `BL` after the node (33), the counterpart of the box's group 90.
    pub cylinder_90: u32,
    /// `BL` after the node (73 in the corpus), the counterpart of the box's group 91.
    pub cylinder_91: u32,
    /// `BD` x4: 0.4, 0.15, 0.15, 0.15 and 0.1, 0.25, 0.25, 0.25 in the two corpus records.
    /// The first differs from the other three, which are equal in each record, so the
    /// first reads as the height and the rest as radii; no value is scored, so the roles
    /// are unverified.
    pub size: [f64; 4],
}

/// Decode an ACSH_CYLINDER_CLASS record from the layout of the module docs (derived from
/// AutoCAD's output, ARCHITECTURE.md §7bq), ending exactly on the data-stream boundary and,
/// from R2007, with the string stream consumed.
pub(crate) fn decode_cylinder(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<ShCylinder> {
    open_and_finish(
        "ACSH_CYLINDER_CLASS",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            let eval = read_eval(c)?;
            let node_90 = c.read_bl_u()?;
            let node_91 = c.read_bl_u()?;
            let transform = read_array::<16>(c)?;
            let color = read_color(c, strings, version)?;
            let node_92 = c.read_bl_u()?;
            let cylinder_90 = c.read_bl_u()?;
            let cylinder_91 = c.read_bl_u()?;
            let size = read_array::<4>(c)?;
            Ok(ShCylinder {
                eval,
                node_90,
                node_91,
                transform,
                color,
                node_92,
                cylinder_90,
                cylinder_91,
                size,
            })
        },
    )
}

/// ACSH_REVOLVE_CLASS: a solid made by revolving a profile. Held to its record boundary
/// alone; no DXF twin scores any value (ARCHITECTURE.md §7bq).
#[derive(Debug, Clone, PartialEq)]
pub struct ShRevolve {
    pub eval: EvalExpr,
    /// `BL`, group 90 of the history node as ACSH_BOX_CLASS has it (33).
    pub node_90: u32,
    /// `BL`, group 91 of the history node as ACSH_BOX_CLASS has it (73 in the corpus).
    pub node_91: u32,
    /// `BD` x16, the transform as ACSH_BOX_CLASS has it.
    pub transform: [f64; 16],
    pub color: ShColor,
    /// `BL`, group 92 of the history node as ACSH_BOX_CLASS has it (8 in the corpus).
    pub node_92: u32,
    /// `BL` after the node (33), the counterpart of the box's group 90.
    pub revolve_90: u32,
    /// `BL` after the node (73 in the corpus), the counterpart of the box's group 91.
    pub revolve_91: u32,
    /// `BD` x3: 0, -0.525, 0 in the one corpus record, read as a point on the axis.
    pub point: [f64; 3],
    /// `BD` x3: 0, 1, 0 in the one corpus record, read as the axis direction.
    pub axis: [f64; 3],
    /// `BD`: 6.283185307179586 (2 pi) in the one corpus record, read as the angle swept.
    pub angle: f64,
    /// `BL` before the profile data (77), the value the extrusion's group 92 holds.
    pub dxf_92: u32,
    /// The profile data: a `BL` bit count (1272) and that many bits, read as bytes. A count
    /// that is not a multiple of 8 is refused.
    pub profile: Vec<u8>,
}

/// Decode an ACSH_REVOLVE_CLASS record from the layout of the module docs (derived from
/// AutoCAD's output, ARCHITECTURE.md §7bq), ending exactly on the data-stream boundary and,
/// from R2007, with the string stream consumed. The six slots between the angle and the
/// profile data are read only in the 2-bit zero form that `BD`, `BL` and `BS` share: the
/// corpus record holds six zeros there, and which of the three types each slot is cannot
/// be told from those bits, so any other form is refused as unsupported.
pub(crate) fn decode_revolve(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<ShRevolve> {
    open_and_finish(
        "ACSH_REVOLVE_CLASS",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            let eval = read_eval(c)?;
            let node_90 = c.read_bl_u()?;
            let node_91 = c.read_bl_u()?;
            let transform = read_array::<16>(c)?;
            let color = read_color(c, strings, version)?;
            let node_92 = c.read_bl_u()?;
            let revolve_90 = c.read_bl_u()?;
            let revolve_91 = c.read_bl_u()?;
            let point = read_array::<3>(c)?;
            let axis = read_array::<3>(c)?;
            let angle = c.read_bd()?;
            for _ in 0..6 {
                if c.read_bb()? != 0b10 {
                    return Err(Error::Unsupported {
                        feature: "ACSH_REVOLVE_CLASS slot that is not the zero form".into(),
                    });
                }
            }
            let dxf_92 = c.read_bl_u()?;
            let bits = c.read_bl_u()? as usize;
            if bits % 8 != 0 {
                return Err(Error::Unsupported {
                    feature: "ACSH_REVOLVE_CLASS profile bit count that is not a multiple of 8"
                        .into(),
                });
            }
            if bits > c.remaining_bits() {
                return Err(Error::SectionMap(format!(
                    "ACSH_REVOLVE_CLASS profile of {bits} bits exceeds the {} bits left in the record",
                    c.remaining_bits()
                )));
            }
            let mut profile = Vec::with_capacity(bits / 8);
            for _ in 0..bits / 8 {
                profile.push(c.read_rc()?);
            }
            Ok(ShRevolve {
                eval,
                node_90,
                node_91,
                transform,
                color,
                node_92,
                revolve_90,
                revolve_91,
                point,
                axis,
                angle,
                dxf_92,
                profile,
            })
        },
    )
}
