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
    /// `BL` degree of the spline.
    pub degree: i32,
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

/// Decodes the `Spline` payload that follows the common entity header
/// (§20.4.40).
///
/// # Measured: `degree` is a `BL` and the branch is derived
///
/// §20.4.40 types `Degree` as a `BL`, not a `BD` — the two SPLINE
/// records of `sample_AC1032.dwg` read `3` as a `BL` and land their
/// tolerances on plausible `1e-9` / `1e-10` values.
///
/// The branch is **not** taken on the stored `Scenario` word on R2013+.
/// Both records store `1`, yet one is a fit spline: §20.4.40 says "the
/// scenario flag becomes 1 if the knot parameter is Custom or has no
/// fit data, otherwise 2", and that derivation is what the bytes
/// follow.
///
/// | handle | scenario | flags1 | knot param | branch taken | ends | boundary |
/// |--------|----------|--------|------------|--------------|------|----------|
/// | `0x433` | 1 | 0 | 15 (Custom) | control: 8 knots `0,0,0,0,1,1,1,1`, 4 control points | 822 | 822 |
/// | `0x434` | 1 | 9 (fit points + use knot param) | 2 (Uniform) | fit: tol `1e-10`, 2 fit points | 734 | 734 |
///
/// Neither record carries a string stream, so the boundary each closes
/// on is the bit before the `strings present` trailer flag — see
/// [`crate::string_stream::data_field_end`].
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Spline> {
    let scenario = c.read_bl()? as u32;
    let r2013_plus = matches!(version, Version::R2013 | Version::R2018);
    let (flag1, knot_param) = if r2013_plus {
        (Some(c.read_bl()? as u32), Some(c.read_bl()? as u32))
    } else {
        (None, None)
    };
    let degree = c.read_bl()?;

    // §20.4.40: on R2013+ the effective scenario is derived from the
    // spline flags and the knot parameter, not read from `Scenario`.
    const KNOT_PARAM_CUSTOM: u32 = 15;
    const SPLINE_FLAG_METHOD_FIT_POINTS: u32 = 1;
    let effective_scenario = match (flag1, knot_param) {
        (Some(flags), Some(param)) => {
            if param == KNOT_PARAM_CUSTOM || flags & SPLINE_FLAG_METHOD_FIT_POINTS == 0 {
                1
            } else {
                2
            }
        }
        _ => scenario,
    };

    let mut fit_tolerance = 0.0;
    let mut begin_tangent = Vec3D::default();
    let mut end_tangent = Vec3D::default();
    let mut num_fit = 0usize;
    let mut rational = false;
    let mut closed = false;
    let mut periodic = false;
    let mut knot_tolerance = 0.0;
    let mut control_tolerance = 0.0;
    let mut num_knots = 0usize;
    let mut num_control = 0usize;
    let mut has_weights = false;
    match effective_scenario {
        2 => {
            fit_tolerance = c.read_bd()?;
            begin_tangent = read_bd3(c)?;
            end_tangent = read_bd3(c)?;
            num_fit = c.read_bl()? as usize;
            bounds_check(num_fit, "fit_points", c.remaining_bits())?;
        }
        1 => {
            rational = c.read_b()?;
            closed = c.read_b()?;
            periodic = c.read_b()?;
            knot_tolerance = c.read_bd()?;
            control_tolerance = c.read_bd()?;
            num_knots = c.read_bl()? as usize;
            bounds_check(num_knots, "knots", c.remaining_bits())?;
            num_control = c.read_bl()? as usize;
            bounds_check(num_control, "control_points", c.remaining_bits())?;
            has_weights = c.read_b()?;
        }
        other => {
            return Err(Error::SectionMap(format!(
                "SPLINE scenario {other} not in {{1, 2}}"
            )));
        }
    }

    let mut knots = Vec::with_capacity(num_knots);
    for _ in 0..num_knots {
        knots.push(c.read_bd()?);
    }
    let mut control_points = Vec::with_capacity(num_control);
    let mut weights = Vec::new();
    for _ in 0..num_control {
        control_points.push(read_bd3(c)?);
        if has_weights {
            weights.push(c.read_bd()?);
        }
    }
    let mut fit_points = Vec::with_capacity(num_fit);
    for _ in 0..num_fit {
        fit_points.push(read_bd3(c)?);
    }

    let (fit, control) = if effective_scenario == 2 {
        (
            Some(FitForm {
                tolerance: fit_tolerance,
                begin_tangent,
                end_tangent,
                fit_points,
            }),
            None,
        )
    } else {
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

/// Decode an R2010+ SPLINE and check it ends exactly on the record's
/// data-stream boundary (§19.1). SPLINE carries no `TV`, so the check
/// is the only thing the split streams contribute — but it is what
/// turns a wrong field list into an error instead of plausible
/// geometry.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<Spline> {
    let (_strings, string_start) = crate::tables::modern::open_entity(payload, version)?;
    let mut c = BitCursor::new(payload);
    crate::string_stream::seek(&mut c, object_body_start)?;
    crate::common_entity::read_common_entity_data(&mut c, version)?;
    let spline = decode(&mut c, version)?;
    let at = c.position_bits();
    if at != string_start {
        return Err(crate::tables::modern::misaligned(
            "SPLINE",
            at,
            string_start,
        ));
    }
    Ok(spline)
}

/// Ceiling on any per-spline collection size. 100K is already far
/// beyond any real-world NURBS; the remaining-bits derivation in
/// `bounds_check` catches inputs that are smaller than this ceiling
/// but still larger than the object payload could physically encode.
const SPLINE_MAX_COUNT: usize = 100_000;

fn bounds_check(n: usize, field: &'static str, remaining_bits: usize) -> Result<()> {
    if n > SPLINE_MAX_COUNT || n > remaining_bits {
        Err(Error::SectionMap(format!(
            "SPLINE {field} count {n} exceeds cap \
             ({SPLINE_MAX_COUNT} or remaining_bits {remaining_bits})"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::string_stream::tests::{bits_of, build_payload};

    fn write_preamble(w: &mut BitWriter) {
        w.write_bs_u(0);
        w.write_b(false);
        w.write_bb(0b10);
        w.write_bl(0);
        w.write_b(true);
        w.write_b(false);
        w.write_bs_u(0x0100);
        w.write_bd(1.0);
        w.write_bb(0b00);
        w.write_bb(0b00);
        w.write_bb(0b00);
        w.write_rc(0);
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
        w.write_bs(0);
        w.write_rc(0x1D);
    }

    #[test]
    fn roundtrip_fit_spline_pre_r2013() {
        let mut w = BitWriter::new();
        w.write_bl(2); // scenario = fit
        w.write_bl(3); // degree is a BL
        w.write_bd(0.01); // fit tolerance
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // begin tangent
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0); // end tangent
        w.write_bl(3); // 3 fit points
        for (x, y, z) in [(0.0, 0.0, 0.0), (1.0, 1.0, 0.0), (2.0, 0.0, 0.0)] {
            w.write_bd(x);
            w.write_bd(y);
            w.write_bd(z);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        let fit = s.fit.unwrap();
        assert_eq!(s.degree, 3);
        assert_eq!(fit.fit_points.len(), 3);
        assert_eq!(fit.tolerance, 0.01);
        assert!(s.control.is_none());
    }

    #[test]
    fn roundtrip_control_spline_pre_r2013() {
        let mut w = BitWriter::new();
        w.write_bl(1); // control-based
        w.write_bl(3); // degree
        w.write_b(false); // not rational
        w.write_b(false); // not closed
        w.write_b(false); // not periodic
        w.write_bd(1e-6); // knot tolerance
        w.write_bd(1e-6); // control tolerance
        w.write_bl(5); // 5 knots
        w.write_bl(3); // 3 control points
        w.write_b(false); // no weights
        for k in [0.0, 0.0, 0.5, 1.0, 1.0] {
            w.write_bd(k);
        }
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

    /// The R2018 shape measured on `sample_AC1032.dwg` handle `0x433`:
    /// stored scenario `1`, spline flags `0`, knot parameter `15`
    /// (Custom) — so the control branch is taken — with 8 knots and 4
    /// control points, and the whole record closing on its
    /// data-stream boundary.
    #[test]
    fn r2018_control_spline_closes_on_the_boundary() {
        let mut w = BitWriter::new();
        write_preamble(&mut w);
        w.write_bl(1); // scenario
        w.write_bl(0); // spline flags 1
        w.write_bl(15); // knot parameter = Custom
        w.write_bl(3); // degree
        w.write_b(false); // rational
        w.write_b(false); // closed
        w.write_b(false); // periodic
        w.write_bd(1e-9);
        w.write_bd(1e-10);
        w.write_bl(8); // num knots
        w.write_bl(4); // num control points
        w.write_b(false); // no weights
        for k in [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0] {
            w.write_bd(k);
        }
        for i in 0..4u8 {
            w.write_bd(f64::from(i));
            w.write_bd(f64::from(i) * 2.0);
            w.write_bd(0.0);
        }
        let body = bits_of(&w);
        let payload = build_payload(&body, &[]);
        let s = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(s.scenario, 1);
        assert_eq!(s.knot_param, Some(15));
        assert_eq!(s.degree, 3);
        let ctl = s.control.expect("control form");
        assert_eq!(ctl.knots.len(), 8);
        assert_eq!(ctl.control_points.len(), 4);
        assert!(s.fit.is_none());
    }

    /// The R2018 shape measured on handle `0x434`: the same stored
    /// scenario `1`, but spline flags `9` (method fit points + use knot
    /// parameter) and knot parameter `2` (Uniform), which §20.4.40's
    /// derivation turns into the *fit* branch.
    #[test]
    fn r2018_derived_scenario_takes_the_fit_branch() {
        let mut w = BitWriter::new();
        write_preamble(&mut w);
        w.write_bl(1); // stored scenario says control...
        w.write_bl(9); // ...but the flags say method-fit-points
        w.write_bl(2); // knot parameter = Uniform (not Custom)
        w.write_bl(3); // degree
        w.write_bd(1e-10); // fit tolerance
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0); // begin tangent
        w.write_bd(0.0);
        w.write_bd(-1.0);
        w.write_bd(0.0); // end tangent
        w.write_bl(2); // 2 fit points
        for (x, y) in [(250.0, 17.0), (259.0, 27.0)] {
            w.write_bd(x);
            w.write_bd(y);
            w.write_bd(0.0);
        }
        let body = bits_of(&w);
        let payload = build_payload(&body, &[]);
        let s = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(s.scenario, 1);
        assert_eq!(s.flag1, Some(9));
        let fit = s.fit.expect("fit form");
        assert_eq!(fit.fit_points.len(), 2);
        assert_eq!(fit.tolerance, 1e-10);
        assert!(s.control.is_none());
    }

    /// A field list one bit short of the boundary must be rejected,
    /// not returned.
    #[test]
    fn misaligned_field_list_errors() {
        let mut w = BitWriter::new();
        write_preamble(&mut w);
        w.write_bl(1);
        w.write_bl(0);
        w.write_bl(15);
        w.write_bl(3);
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
        w.write_bd(1e-9);
        w.write_bd(1e-10);
        w.write_bl(0); // num knots
        w.write_bl(0); // num control points
        w.write_b(false);
        let mut body = bits_of(&w);
        body.push(false); // one bit the field list will not consume
        let payload = build_payload(&body, &[]);
        let err = decode_modern_split_stream(&payload, 8, Version::R2018)
            .expect_err("an extra bit must be rejected");
        assert!(
            matches!(&err, Error::SectionMap(m) if m.contains("SPLINE data fields ended")),
            "err={err:?}"
        );
    }
}
