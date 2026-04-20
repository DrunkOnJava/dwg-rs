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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_right_handed_helix() {
        let mut w = BitWriter::new();
        // axis base at origin
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // start at (radius, 0, 0)
        w.write_bd(5.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // axis +Z
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        // radius 5
        w.write_bd(5.0);
        // 3 turns
        w.write_bd(3.0);
        // turn height 2 (total height 6)
        w.write_bd(2.0);
        // right-handed
        w.write_b(true);
        // constrain by turn-height
        w.write_bs(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c).unwrap();
        assert_eq!(h.axis_base_point, Point3D::default());
        assert_eq!(
            h.start_point,
            Point3D {
                x: 5.0,
                y: 0.0,
                z: 0.0
            }
        );
        assert!((h.radius - 5.0).abs() < 1e-12);
        assert!((h.turns - 3.0).abs() < 1e-12);
        assert!(h.right_handed);
        assert_eq!(h.constrain_type, ConstrainType::TurnHeight);
    }

    #[test]
    fn roundtrip_left_handed_fractional_turns() {
        let mut w = BitWriter::new();
        for _ in 0..3 {
            // three points, no origin shift for simplicity
            w.write_bd(1.0);
            w.write_bd(2.0);
            w.write_bd(3.0);
        }
        w.write_bd(2.5); // radius
        w.write_bd(1.25); // turns — fractional
        w.write_bd(1.0); // turn height
        w.write_b(false); // left-handed
        w.write_bs(2); // constrain by height
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c).unwrap();
        assert!(!h.right_handed);
        assert!((h.turns - 1.25).abs() < 1e-12);
        assert_eq!(h.constrain_type, ConstrainType::Height);
    }

    #[test]
    fn rejects_bad_constrain_enum() {
        assert!(ConstrainType::from_bs(3).is_err());
        assert!(matches!(
            ConstrainType::from_bs(-1).unwrap_err(),
            Error::SectionMap(_)
        ));
    }

    #[test]
    fn constrain_type_roundtrip() {
        for ct in [
            ConstrainType::TurnHeight,
            ConstrainType::Turns,
            ConstrainType::Height,
        ] {
            assert_eq!(ConstrainType::from_bs(ct.to_bs()).unwrap(), ct);
        }
    }
}
