//! SPLINE entity (§19.4.44) — NURBS curve (non-uniform rational
//! B-spline).
//!
//! SPLINE stores either a *fit* form (a list of points the curve
//! interpolates) or a *control* form (degree + knots + control-point
//! list + weights). The `scenario` byte selects between them.
//!
//! # Stream shape
//!
//! ```text
//! BL   scenario         -- 1 = control-based, 2 = fit-based
//! (R2013+)
//!   BL    spline_flag1  -- planar/linear/rational/closed/periodic bits
//!   BL    knot_param    -- 0=Chord, 1=SquareRoot, 2=Uniform, 3=Custom
//! BD   degree
//! // --- fit-based branch (scenario == 2) ---
//! BD   fit_tolerance
//! BD3  begin_tangent
//! BD3  end_tangent
//! BL   num_fit_pts
//! BD3 × num_fit_pts   fit_points
//! // --- control-based branch (scenario == 1) ---
//! B    rational
//! B    closed
//! B    periodic
//! BD   knot_tolerance
//! BD   control_tolerance
//! BL   num_knots
//! BD × num_knots    knots
//! BL   num_control_points
//! BD3 × num_control_points  control_points
//! BD × num_control_points   weights        -- only if rational
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Spline {
    pub scenario: u32,
    pub flag1: Option<u32>,
    pub knot_param: Option<u32>,
    pub degree: f64,
    pub fit: Option<FitForm>,
    pub control: Option<ControlForm>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FitForm {
    pub tolerance: f64,
    pub begin_tangent: Vec3D,
    pub end_tangent: Vec3D,
    pub fit_points: Vec<Point3D>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControlForm {
    pub rational: bool,
    pub closed: bool,
    pub periodic: bool,
    pub knot_tolerance: f64,
    pub control_tolerance: f64,
    pub knots: Vec<f64>,
    pub control_points: Vec<Point3D>,
    pub weights: Vec<f64>,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Spline> {
    let scenario = c.read_bl()? as u32;
    let (flag1, knot_param) = if matches!(version, Version::R2013 | Version::R2018) {
        (Some(c.read_bl()? as u32), Some(c.read_bl()? as u32))
    } else {
        (None, None)
    };
    let degree = c.read_bd()?;

    let (fit, control) = match scenario {
        2 => {
            let tolerance = c.read_bd()?;
            let begin_tangent = read_bd3(c)?;
            let end_tangent = read_bd3(c)?;
            let n = c.read_bl()? as usize;
            bounds_check(n, "fit_points")?;
            let mut fit_points = Vec::with_capacity(n);
            for _ in 0..n {
                fit_points.push(read_bd3(c)?);
            }
            (
                Some(FitForm {
                    tolerance,
                    begin_tangent,
                    end_tangent,
                    fit_points,
                }),
                None,
            )
        }
        1 => {
            let rational = c.read_b()?;
            let closed = c.read_b()?;
            let periodic = c.read_b()?;
            let knot_tolerance = c.read_bd()?;
            let control_tolerance = c.read_bd()?;
            let num_knots = c.read_bl()? as usize;
            bounds_check(num_knots, "knots")?;
            let mut knots = Vec::with_capacity(num_knots);
            for _ in 0..num_knots {
                knots.push(c.read_bd()?);
            }
            let num_control = c.read_bl()? as usize;
            bounds_check(num_control, "control_points")?;
            let mut control_points = Vec::with_capacity(num_control);
            let mut weights = Vec::new();
            for _ in 0..num_control {
                control_points.push(read_bd3(c)?);
            }
            if rational {
                weights.reserve(num_control);
                for _ in 0..num_control {
                    weights.push(c.read_bd()?);
                }
            }
            (
                None,
                Some(ControlForm {
                    rational,
                    closed,
                    periodic,
                    knot_tolerance,
                    control_tolerance,
                    knots,
                    control_points,
                    weights,
                }),
            )
        }
        _ => {
            return Err(Error::SectionMap(format!(
                "SPLINE scenario {scenario} not in {{1, 2}}"
            )));
        }
    };

    Ok(Spline {
        scenario,
        flag1,
        knot_param,
        degree,
        fit,
        control,
    })
}

fn bounds_check(n: usize, field: &'static str) -> Result<()> {
    if n > 1_000_000 {
        Err(Error::SectionMap(format!(
            "SPLINE {field} count {n} exceeds 1M sanity bound"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_fit_spline() {
        let mut w = BitWriter::new();
        w.write_bl(2); // scenario = fit
        w.write_bd(3.0); // degree
        w.write_bd(0.01); // tolerance
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // begin_tangent
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0); // end_tangent
        w.write_bl(3); // 3 fit points
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(2.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        let fit = s.fit.unwrap();
        assert_eq!(fit.fit_points.len(), 3);
        assert_eq!(fit.tolerance, 0.01);
        assert!(s.control.is_none());
    }

    #[test]
    fn roundtrip_control_spline() {
        let mut w = BitWriter::new();
        w.write_bl(1); // control-based
        w.write_bd(3.0);
        w.write_b(false); // not rational
        w.write_b(false); // not closed
        w.write_b(false); // not periodic
        w.write_bd(1e-6); // knot tolerance
        w.write_bd(1e-6); // control tolerance
        w.write_bl(5); // 5 knots
        for k in [0.0, 0.0, 0.5, 1.0, 1.0] {
            w.write_bd(k);
        }
        w.write_bl(3); // 3 control points
        for (x, y, z) in [(0.0, 0.0, 0.0), (1.0, 2.0, 0.0), (2.0, 0.0, 0.0)] {
            w.write_bd(x);
            w.write_bd(y);
            w.write_bd(z);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        let ctl = s.control.unwrap();
        assert_eq!(ctl.knots.len(), 5);
        assert_eq!(ctl.control_points.len(), 3);
        assert!(ctl.weights.is_empty());
    }
}
