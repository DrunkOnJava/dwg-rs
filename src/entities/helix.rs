//! HELIX entity — ODA Open Design Specification v5.4.1 §19.4.76
//! (L4-41 in the entity inventory).
//!
//! Unlike the SURFACE family, HELIX is purely parametric — there is
//! no cached ACIS body. The curve is fully defined by its axis,
//! radius, turns, height, handedness, and a constraint flag that
//! picks which of the five parameters {height, radius, turns,
//! turn_height, slope} is the dependent one.
//!
//! # Stream shape
//!
//! ```text
//! BD3 axis_base_point   -- WCS origin of the helix axis
//! BD3 start_point       -- WCS starting point of the curve (on cylinder)
//! BD3 axis_vector       -- WCS axis direction (magnitude does not matter)
//! BD  radius
//! BD  turns             -- real-valued turn count; may be fractional
//! BD  turn_height       -- pitch: distance along axis per turn
//! B   right_handed      -- 1 ⇒ right-hand; 0 ⇒ left-hand
//! BS  constrain_type    -- 0..2; see [`ConstrainType`]
//! ```
//!
//! The five radial / axial / temporal parameters {radius, turns,
//! turn_height, axis_length, slope} are over-determined; the spec
//! designates one as the "driven" variable. `constrain_type` tells
//! us which.

use crate::bitcursor::BitCursor;
use crate::entities::read_bd3;
use crate::entities::{Point3D, Vec3D};
use crate::error::{Error, Result};

/// Which helix parameter is the driven (dependent) one. A helix is
/// fully specified by any three of {radius, turns, turn_height,
/// axis_length, slope}; the constraint flag picks which of the five
/// is computed from the others. Only values 0..=2 are valid per
/// §19.4.76 — AutoCAD exposes the same trichotomy in its UI as
/// "Turn Height / Turns / Height" in the HELIX dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstrainType {
    /// 0 — turn height is driven by height + turns.
    TurnHeight,
    /// 1 — turns is driven by height + turn height.
    Turns,
    /// 2 — overall height is driven by turns + turn height.
    Height,
}

impl ConstrainType {
    /// Parses the HELIX constrain type from its `BS` code (0 turn height, 1 turns, 2 height).
    pub fn from_bs(v: i16) -> Result<Self> {
        match v {
            0 => Ok(Self::TurnHeight),
            1 => Ok(Self::Turns),
            2 => Ok(Self::Height),
            other => Err(Error::SectionMap(format!(
                "HELIX constrain_type {other} not in 0..=2"
            ))),
        }
    }

    /// Encodes the constrain type back to its `BS` code.
    pub fn to_bs(self) -> i16 {
        match self {
            Self::TurnHeight => 0,
            Self::Turns => 1,
            Self::Height => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Helix {
    pub axis_base_point: Point3D,
    pub start_point: Point3D,
    pub axis_vector: Vec3D,
    pub radius: f64,
    pub turns: f64,
    pub turn_height: f64,
    /// `true` for a right-handed helix; `false` for left-handed.
    pub right_handed: bool,
    pub constrain_type: ConstrainType,
}

/// Decodes the `Helix` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Helix> {
    let axis_base_point = read_bd3(c)?;
    let start_point = read_bd3(c)?;
    let axis_vector = read_bd3(c)?;
    let radius = c.read_bd()?;
    let turns = c.read_bd()?;
    let turn_height = c.read_bd()?;
    let right_handed = c.read_b()?;
    let constrain_type = ConstrainType::from_bs(c.read_bs()?)?;
    Ok(Helix {
        axis_base_point,
        start_point,
        axis_vector,
        radius,
        turns,
        turn_height,
        right_handed,
        constrain_type,
    })
}
