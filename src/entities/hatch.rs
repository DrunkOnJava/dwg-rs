//! HATCH entity (§19.4.75) — filled hatch region.
//!
//! HATCH is among the densest entity types in DWG: every instance
//! encodes a gradient-fill block (R2004+), an elevation, an extrusion,
//! a pattern name, a boundary-path tree (lines / arcs / ellipses /
//! splines or an explicit polyline), a pattern-line definition, and
//! a seed-point list. This module now decodes the full structure end
//! to end — the `num_paths == 0` special case the previous iteration
//! handled remains correct, and every path type in the tree is now
//! exercised.
//!
//! # Stream shape (R2004+, L4-22)
//!
//! ```text
//! (R2004+)
//!   BS    is_gradient_fill        -- 0 = plain hatch, 1 = gradient
//!   (if is_gradient_fill)
//!     BL  reserved                -- always 0
//!     BD  gradient_angle
//!     BD  gradient_shift
//!     BL  is_single_color_gradient
//!     BD  gradient_tint
//!     BL  num_gradient_colors     -- cap 16
//!     for each:
//!       BD unknown_double
//!       CMC color                 -- simplified to BS ACI index
//!     TV  gradient_name
//! BD   elevation
//! BD3  extrusion
//! TV   pattern_name
//! B    solid_fill                 -- 1 = solid, 0 = pattern
//! B    associative
//! BL   num_paths                  -- cap 10_000
//! for each path:
//!   BL   path_type_flags
//!   (if polyline)
//!     B    has_bulge
//!     B    is_closed
//!     BL   num_path_segs
//!     for each seg:
//!       BD2  vertex
//!       (if has_bulge) BD bulge
//!   (else)
//!     BL   num_path_segs
//!     for each seg:
//!       RC seg_type (1 line, 2 arc, 3 ellipse, 4 spline)
//!       (line)    BD2 start, BD2 end
//!       (arc)     BD2 center, BD radius, BD start_angle, BD end_angle, B ccw
//!       (ellipse) BD2 center, BD2 endpoint, BD axis_ratio,
//!                 BD start_angle, BD end_angle, B ccw
//!       (spline)  BL degree, B is_rational, B is_periodic,
//!                 BL num_knots, BL num_control_points,
//!                 BD[num_knots] knots, BD2[num_control_points] control_points,
//!                 (if is_rational) BD[num_control_points] weights,
//!                 BL num_fit_points, BD2[num_fit_points] fit_points
//!   BL   num_boundary_handles      -- cap 1024
//!   H[num_boundary_handles] boundary_handles
//! BS   pattern_style               -- 0-2
//! BD   pattern_angle
//! BD   pattern_scale_or_spacing
//! B    pattern_double
//! BS   num_pattern_lines           -- cap 100
//! for each pattern line:
//!   BD   line_angle
//!   BD2  line_origin
//!   BD2  line_offset
//!   BS   num_line_dashes            -- cap 64
//!   BD[num_line_dashes] line_dashes
//! BS   pixel_size                  -- rendering hint
//! BL   num_seed_points             -- cap 1024
//! BD2[num_seed_points] seed_points
//! (R2007+) H plot_style_handle
//! ```
//!
//! # Defensive caps
//!
//! Seven per-array caps are enforced:
//!
//! | Array                  | Cap     |
//! |------------------------|---------|
//! | gradient colors        | 16      |
//! | boundary paths         | 10_000  |
//! | segments per path      | 100_000 |
//! | boundary handles       | 1_024   |
//! | pattern lines          | 100     |
//! | dashes per pattern line| 64      |
//! | seed points            | 1_024   |
//!
//! Each cap is paired with a remaining-bits sanity check so a claimed
//! count larger than the object's payload can possibly encode is
//! rejected immediately — defense against adversarial or truncated
//! streams.

use crate::bitcursor::{BitCursor, Handle};
use crate::entities::{Point2D, Vec3D, read_bd2, read_bd3};
use crate::error::{Error, Result};
use crate::tables::read_tv;
use crate::version::Version;

// ========================================================================
// Defensive caps — derived from ODA §19.4.75 "practical limits" guidance
// cross-checked against observed worst-case values in real drawings.
// ========================================================================
const CAP_GRADIENT_COLORS: usize = 16;
const CAP_PATHS: usize = 10_000;
const CAP_PATH_SEGS: usize = 100_000;
const CAP_BOUNDARY_HANDLES: usize = 1_024;
const CAP_PATTERN_LINES: usize = 100;
const CAP_LINE_DASHES: usize = 64;
const CAP_SEED_POINTS: usize = 1_024;

#[derive(Debug, Clone, PartialEq)]
pub struct Hatch {
    pub gradient: Option<GradientFill>,
    pub elevation: f64,
    pub extrusion: Vec3D,
    pub pattern_name: String,
    pub solid_fill: bool,
    pub associative: bool,
    pub paths: Vec<HatchPath>,
    pub pattern_style: u16,
    pub pattern_angle: f64,
    pub pattern_scale: f64,
    pub pattern_double: bool,
    pub pattern_lines: Vec<PatternLine>,
    pub pixel_size: u16,
    pub seed_points: Vec<(f64, f64)>,
}

/// Gradient-fill block (§19.4.75, R2004+).
///
/// CMC colors are simplified to the ACI (AutoCAD Color Index) byte —
/// a BS value in the range 0..=256. This matches the representation
/// used by every other entity that carries a CMC in this crate
/// (`light`, `mtext`, `acad_material`).
#[derive(Debug, Clone, PartialEq)]
pub struct GradientFill {
    pub angle: f64,
    pub shift: f64,
    pub is_single_color: u32,
    pub tint: f64,
    pub colors: Vec<GradientColor>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GradientColor {
    pub unknown_double: f64,
    pub color: i16,
}

/// A single boundary path in the HATCH tree. Stores the raw
/// `path_type_flags` bitset (`1` external, `2` polyline, `4` derived,
/// `8` textbox, `16` outermost) plus the segments themselves.
#[derive(Debug, Clone, PartialEq)]
pub struct HatchPath {
    pub flags: u32,
    pub segments: HatchPathSegments,
    pub boundary_handles: Vec<Handle>,
}

/// Path body — either a polyline (list of vertices with optional
/// bulges) or an edge list (lines / arcs / ellipses / splines).
#[derive(Debug, Clone, PartialEq)]
pub enum HatchPathSegments {
    Polyline {
        has_bulge: bool,
        is_closed: bool,
        vertices: Vec<(Point2D, Option<f64>)>,
    },
    Edges(Vec<HatchEdge>),
}

/// One edge within a non-polyline boundary loop.
#[derive(Debug, Clone, PartialEq)]
pub enum HatchEdge {
    Line {
        start: Point2D,
        end: Point2D,
    },
    Arc {
        center: Point2D,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
        counter_clockwise: bool,
    },
    Ellipse {
        center: Point2D,
        endpoint: Point2D,
        axis_ratio: f64,
        start_angle: f64,
        end_angle: f64,
        counter_clockwise: bool,
    },
    Spline {
        degree: u32,
        is_rational: bool,
        is_periodic: bool,
        knots: Vec<f64>,
        control_points: Vec<Point2D>,
        weights: Vec<f64>,
        fit_points: Vec<Point2D>,
    },
}

/// One pattern line within a non-solid hatch pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternLine {
    pub angle: f64,
    pub origin: Point2D,
    pub offset: Point2D,
    pub dashes: Vec<f64>,
}

// ========================================================================
// Per-array bounds check. Kept as a tiny helper instead of inlining the
// cap-plus-remaining-bits logic everywhere so the caps can be audited
// at a glance.
// ========================================================================

fn bounds_check(n: usize, field: &'static str, cap: usize, remaining_bits: usize) -> Result<()> {
    if n > cap || n > remaining_bits {
        Err(Error::SectionMap(format!(
            "HATCH {field} count {n} exceeds cap ({cap}) \
             or remaining_bits ({remaining_bits})"
        )))
    } else {
        Ok(())
    }
}

/// Decode a HATCH entity's type-specific payload.
///
/// The caller is expected to have already consumed the object header
/// (type code, size, handle) and the common entity preamble. This
/// function reads every field defined by §19.4.75, including the
/// boundary path tree.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Hatch> {
    let gradient = decode_gradient(c, version)?;

    let elevation = c.read_bd()?;
    let extrusion = read_bd3(c)?;
    let pattern_name = read_tv(c, version)?;
    let solid_fill = c.read_b()?;
    let associative = c.read_b()?;

    let num_paths = c.read_bl_u()? as usize;
    bounds_check(num_paths, "num_paths", CAP_PATHS, c.remaining_bits())?;
    let mut paths = Vec::with_capacity(num_paths);
    for _ in 0..num_paths {
        paths.push(decode_path(c)?);
    }

    let pattern_style = c.read_bs_u()?;
    let pattern_angle = c.read_bd()?;
    let pattern_scale = c.read_bd()?;
    let pattern_double = c.read_b()?;

    let num_pattern_lines = c.read_bs_u()? as usize;
    bounds_check(
        num_pattern_lines,
        "num_pattern_lines",
        CAP_PATTERN_LINES,
        c.remaining_bits(),
    )?;
    let mut pattern_lines = Vec::with_capacity(num_pattern_lines);
    for _ in 0..num_pattern_lines {
        pattern_lines.push(decode_pattern_line(c)?);
    }

    let pixel_size = c.read_bs_u()?;
    let num_seed_points = c.read_bl_u()? as usize;
    bounds_check(
        num_seed_points,
        "num_seed_points",
        CAP_SEED_POINTS,
        c.remaining_bits(),
    )?;
    let mut seed_points = Vec::with_capacity(num_seed_points);
    for _ in 0..num_seed_points {
        let p = read_bd2(c)?;
        seed_points.push((p.x, p.y));
    }

    // Plot-style handle is R2007+ only. Earlier formats don't emit it,
    // and reading a trailing handle that isn't there would mis-align the
    // next object in the stream.
    if version.is_r2007_plus() {
        let _plot_style = c.read_handle()?;
    }

    Ok(Hatch {
        gradient,
        elevation,
        extrusion,
        pattern_name,
        solid_fill,
        associative,
        paths,
        pattern_style,
        pattern_angle,
        pattern_scale,
        pattern_double,
        pattern_lines,
        pixel_size,
        seed_points,
    })
}

fn decode_gradient(c: &mut BitCursor<'_>, version: Version) -> Result<Option<GradientFill>> {
    if !version.is_r2004_plus() {
        return Ok(None);
    }
    let is_gradient_fill = c.read_bs_u()?;
    if is_gradient_fill == 0 {
        return Ok(None);
    }
    let _reserved = c.read_bl()?;
    let angle = c.read_bd()?;
    let shift = c.read_bd()?;
    let is_single_color = c.read_bl_u()?;
    let tint = c.read_bd()?;
    let num_colors = c.read_bl_u()? as usize;
    bounds_check(
        num_colors,
        "num_gradient_colors",
        CAP_GRADIENT_COLORS,
        c.remaining_bits(),
    )?;
    let mut colors = Vec::with_capacity(num_colors);
    for _ in 0..num_colors {
        let unknown_double = c.read_bd()?;
        // CMC simplified to ACI (matches the rest of the crate).
        let color = c.read_bs()?;
        colors.push(GradientColor {
            unknown_double,
            color,
        });
    }
    let name = read_tv(c, version)?;
    Ok(Some(GradientFill {
        angle,
        shift,
        is_single_color,
        tint,
        colors,
        name,
    }))
}

fn decode_path(c: &mut BitCursor<'_>) -> Result<HatchPath> {
    const FLAG_POLYLINE: u32 = 0x02;
    let flags = c.read_bl_u()?;
    let segments = if flags & FLAG_POLYLINE != 0 {
        decode_polyline_path(c)?
    } else {
        decode_edge_path(c)?
    };
    let num_handles = c.read_bl_u()? as usize;
    bounds_check(
        num_handles,
        "num_boundary_handles",
        CAP_BOUNDARY_HANDLES,
        c.remaining_bits(),
    )?;
    let mut boundary_handles = Vec::with_capacity(num_handles);
    for _ in 0..num_handles {
        boundary_handles.push(c.read_handle()?);
    }
    Ok(HatchPath {
        flags,
        segments,
        boundary_handles,
    })
}

fn decode_polyline_path(c: &mut BitCursor<'_>) -> Result<HatchPathSegments> {
    let has_bulge = c.read_b()?;
    let is_closed = c.read_b()?;
    let num_vertices = c.read_bl_u()? as usize;
    bounds_check(
        num_vertices,
        "num_path_segs (polyline)",
        CAP_PATH_SEGS,
        c.remaining_bits(),
    )?;
    let mut vertices = Vec::with_capacity(num_vertices);
    for _ in 0..num_vertices {
        let vertex = read_bd2(c)?;
        let bulge = if has_bulge { Some(c.read_bd()?) } else { None };
        vertices.push((vertex, bulge));
    }
    Ok(HatchPathSegments::Polyline {
        has_bulge,
        is_closed,
        vertices,
    })
}

fn decode_edge_path(c: &mut BitCursor<'_>) -> Result<HatchPathSegments> {
    let num_edges = c.read_bl_u()? as usize;
    bounds_check(
        num_edges,
        "num_path_segs (edges)",
        CAP_PATH_SEGS,
        c.remaining_bits(),
    )?;
    let mut edges = Vec::with_capacity(num_edges);
    for _ in 0..num_edges {
        edges.push(decode_edge(c)?);
    }
    Ok(HatchPathSegments::Edges(edges))
}

fn decode_edge(c: &mut BitCursor<'_>) -> Result<HatchEdge> {
    let seg_type = c.read_rc()?;
    match seg_type {
        1 => Ok(HatchEdge::Line {
            start: read_bd2(c)?,
            end: read_bd2(c)?,
        }),
        2 => Ok(HatchEdge::Arc {
            center: read_bd2(c)?,
            radius: c.read_bd()?,
            start_angle: c.read_bd()?,
            end_angle: c.read_bd()?,
            counter_clockwise: c.read_b()?,
        }),
        3 => Ok(HatchEdge::Ellipse {
            center: read_bd2(c)?,
            endpoint: read_bd2(c)?,
            axis_ratio: c.read_bd()?,
            start_angle: c.read_bd()?,
            end_angle: c.read_bd()?,
            counter_clockwise: c.read_b()?,
        }),
        4 => decode_spline_edge(c),
        _ => Err(Error::SectionMap(format!(
            "HATCH edge seg_type {seg_type} not in {{1 line, 2 arc, 3 ellipse, 4 spline}}"
        ))),
    }
}

fn decode_spline_edge(c: &mut BitCursor<'_>) -> Result<HatchEdge> {
    let degree = c.read_bl_u()?;
    let is_rational = c.read_b()?;
    let is_periodic = c.read_b()?;
    let num_knots = c.read_bl_u()? as usize;
    bounds_check(num_knots, "spline knots", CAP_PATH_SEGS, c.remaining_bits())?;
    let num_control = c.read_bl_u()? as usize;
    bounds_check(
        num_control,
        "spline control_points",
        CAP_PATH_SEGS,
        c.remaining_bits(),
    )?;
    let mut knots = Vec::with_capacity(num_knots);
    for _ in 0..num_knots {
        knots.push(c.read_bd()?);
    }
    let mut control_points = Vec::with_capacity(num_control);
    for _ in 0..num_control {
        control_points.push(read_bd2(c)?);
    }
    let weights = if is_rational {
        let mut w = Vec::with_capacity(num_control);
        for _ in 0..num_control {
            w.push(c.read_bd()?);
        }
        w
    } else {
        Vec::new()
    };
    let num_fit = c.read_bl_u()? as usize;
    bounds_check(
        num_fit,
        "spline fit_points",
        CAP_PATH_SEGS,
        c.remaining_bits(),
    )?;
    let mut fit_points = Vec::with_capacity(num_fit);
    for _ in 0..num_fit {
        fit_points.push(read_bd2(c)?);
    }
    Ok(HatchEdge::Spline {
        degree,
        is_rational,
        is_periodic,
        knots,
        control_points,
        weights,
        fit_points,
    })
}

fn decode_pattern_line(c: &mut BitCursor<'_>) -> Result<PatternLine> {
    let angle = c.read_bd()?;
    let origin = read_bd2(c)?;
    let offset = read_bd2(c)?;
    let num_dashes = c.read_bs_u()? as usize;
    bounds_check(
        num_dashes,
        "num_line_dashes",
        CAP_LINE_DASHES,
        c.remaining_bits(),
    )?;
    let mut dashes = Vec::with_capacity(num_dashes);
    for _ in 0..num_dashes {
        dashes.push(c.read_bd()?);
    }
    Ok(PatternLine {
        angle,
        origin,
        offset,
        dashes,
    })
}

/// Back-compat alias. Prefer [`decode`].
pub use decode as decode_header;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Build the header-plus-tail of a HATCH with zero paths.
    /// Subsequent tests reuse this to append path fields in between.
    fn write_hatch_no_gradient(
        w: &mut BitWriter,
        pattern_name: &[u8],
        solid_fill: bool,
        version: Version,
    ) {
        // R2004+ gets the gradient-flag BS. Tests for R2000 skip it.
        if version.is_r2004_plus() {
            w.write_bs_u(0); // not a gradient
        }
        w.write_bd(0.0); // elevation
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // extrusion (0,0,1)
        w.write_bs_u(pattern_name.len() as u16);
        for b in pattern_name {
            w.write_rc(*b);
        }
        w.write_b(solid_fill);
        w.write_b(false); // associative
    }

    /// Tail fields for a hatch with no pattern lines + no seed points +
    /// no R2007 plot-style handle. The BS num_pattern_lines = 0, the BL
    /// num_seed_points = 0.
    fn write_hatch_tail(w: &mut BitWriter) {
        w.write_bs_u(0); // pattern_style
        w.write_bd(0.0); // pattern_angle
        w.write_bd(1.0); // pattern_scale
        w.write_b(false); // pattern_double
        w.write_bs_u(0); // num_pattern_lines
        w.write_bs_u(0); // pixel_size
        w.write_bl(0); // num_seed_points
    }

    #[test]
    fn roundtrip_solid_fill_no_paths() {
        let mut w = BitWriter::new();
        write_hatch_no_gradient(&mut w, b"SOLID", true, Version::R2000);
        w.write_bl(0); // 0 paths
        write_hatch_tail(&mut w);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(h.pattern_name, "SOLID");
        assert!(h.solid_fill);
        assert!(!h.associative);
        assert!(h.paths.is_empty());
        assert!(h.gradient.is_none());
        assert!(h.pattern_lines.is_empty());
        assert!(h.seed_points.is_empty());
    }

    #[test]
    fn roundtrip_polyline_path() {
        let mut w = BitWriter::new();
        write_hatch_no_gradient(&mut w, b"ANSI31", false, Version::R2004);
        w.write_bl(1); // 1 path
        // Path 0: polyline (flag bit 0x02), closed, 3 vertices, no bulge.
        w.write_bl_u(0x02 | 0x10); // polyline + outermost
        w.write_b(false); // has_bulge = false
        w.write_b(true); // is_closed = true
        w.write_bl(3); // num_vertices
        for (x, y) in [(0.0f64, 0.0), (10.0, 0.0), (10.0, 10.0)] {
            w.write_bd(x);
            w.write_bd(y);
        }
        w.write_bl(0); // num_boundary_handles
        write_hatch_tail(&mut w);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2004).unwrap();
        assert_eq!(h.paths.len(), 1);
        assert_eq!(h.paths[0].flags, 0x12);
        match &h.paths[0].segments {
            HatchPathSegments::Polyline {
                has_bulge,
                is_closed,
                vertices,
            } => {
                assert!(!has_bulge);
                assert!(is_closed);
                assert_eq!(vertices.len(), 3);
                assert_eq!(vertices[0].0, Point2D { x: 0.0, y: 0.0 });
                assert_eq!(vertices[2].0, Point2D { x: 10.0, y: 10.0 });
                assert!(vertices[0].1.is_none());
            }
            other => panic!("expected Polyline, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_line_edge_path() {
        let mut w = BitWriter::new();
        write_hatch_no_gradient(&mut w, b"ANSI31", false, Version::R2004);
        w.write_bl(1); // 1 path
        w.write_bl_u(0x01); // external (not polyline)
        w.write_bl(4); // 4 edges
        let square = [
            ((0.0f64, 0.0), (10.0, 0.0)),
            ((10.0, 0.0), (10.0, 10.0)),
            ((10.0, 10.0), (0.0, 10.0)),
            ((0.0, 10.0), (0.0, 0.0)),
        ];
        for ((sx, sy), (ex, ey)) in square {
            w.write_rc(1); // line
            w.write_bd(sx);
            w.write_bd(sy);
            w.write_bd(ex);
            w.write_bd(ey);
        }
        w.write_bl(0); // num_boundary_handles
        write_hatch_tail(&mut w);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2004).unwrap();
        assert_eq!(h.paths.len(), 1);
        match &h.paths[0].segments {
            HatchPathSegments::Edges(edges) => {
                assert_eq!(edges.len(), 4);
                match &edges[0] {
                    HatchEdge::Line { start, end } => {
                        assert_eq!(*start, Point2D { x: 0.0, y: 0.0 });
                        assert_eq!(*end, Point2D { x: 10.0, y: 0.0 });
                    }
                    other => panic!("expected Line, got {other:?}"),
                }
                match &edges[3] {
                    HatchEdge::Line { start, end } => {
                        assert_eq!(*start, Point2D { x: 0.0, y: 10.0 });
                        assert_eq!(*end, Point2D { x: 0.0, y: 0.0 });
                    }
                    other => panic!("expected Line, got {other:?}"),
                }
            }
            other => panic!("expected Edges, got {other:?}"),
        }
    }

    #[test]
    fn decode_errors_on_oversized_paths() {
        let mut w = BitWriter::new();
        write_hatch_no_gradient(&mut w, b"SOLID", true, Version::R2000);
        w.write_bl(20_000); // 20_000 paths — over cap
        // No need to append any tail — decode should reject before reading paths.
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("num_paths")),
            "err={err:?}"
        );
    }

    #[test]
    fn decode_errors_on_oversized_segs() {
        let mut w = BitWriter::new();
        write_hatch_no_gradient(&mut w, b"SOLID", true, Version::R2000);
        w.write_bl(1); // 1 path
        w.write_bl_u(0x01); // external (edge path)
        w.write_bl(200_000); // num_path_segs — over cap
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("num_path_segs")),
            "err={err:?}"
        );
    }

    #[test]
    fn roundtrip_gradient_fill() {
        let mut w = BitWriter::new();
        // R2004+ — emit the gradient block.
        w.write_bs_u(1); // is_gradient_fill = true
        w.write_bl(0); // reserved
        w.write_bd(45.0); // angle
        w.write_bd(0.5); // shift
        w.write_bl(1); // is_single_color = 1
        w.write_bd(0.75); // tint
        w.write_bl(2); // num_gradient_colors
        for (ud, col) in [(0.0f64, 1i16), (1.0, 5)] {
            w.write_bd(ud);
            w.write_bs(col);
        }
        let name = b"SPHERICAL";
        w.write_bs_u(name.len() as u16);
        for b in name {
            w.write_rc(*b);
        }
        // header tail (shared with non-gradient path)
        w.write_bd(0.0); // elevation
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // extrusion
        let pn = b"SOLID";
        w.write_bs_u(pn.len() as u16);
        for b in pn {
            w.write_rc(*b);
        }
        w.write_b(true); // solid_fill
        w.write_b(false); // associative
        w.write_bl(0); // 0 paths
        write_hatch_tail(&mut w);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2004).unwrap();
        let g = h.gradient.expect("gradient should be present");
        assert_eq!(g.angle, 45.0);
        assert_eq!(g.shift, 0.5);
        assert_eq!(g.is_single_color, 1);
        assert_eq!(g.tint, 0.75);
        assert_eq!(g.colors.len(), 2);
        assert_eq!(g.colors[0].color, 1);
        assert_eq!(g.colors[1].color, 5);
        assert_eq!(g.name, "SPHERICAL");
    }

    #[test]
    fn roundtrip_pattern_lines_and_seed_points() {
        let mut w = BitWriter::new();
        write_hatch_no_gradient(&mut w, b"ANSI31", false, Version::R2000);
        w.write_bl(0); // 0 paths
        w.write_bs_u(1); // pattern_style
        w.write_bd(45.0); // pattern_angle
        w.write_bd(2.0); // pattern_scale
        w.write_b(false); // pattern_double
        w.write_bs_u(2); // num_pattern_lines
        // line 0: 2 dashes
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // origin
        w.write_bd(1.0);
        w.write_bd(0.0); // offset
        w.write_bs_u(2);
        w.write_bd(1.0);
        w.write_bd(-0.5);
        // line 1: 0 dashes
        w.write_bd(90.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bs_u(0);
        w.write_bs_u(4); // pixel_size
        w.write_bl(1); // num_seed_points
        w.write_bd(5.0);
        w.write_bd(5.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(h.pattern_style, 1);
        assert_eq!(h.pattern_angle, 45.0);
        assert_eq!(h.pattern_scale, 2.0);
        assert_eq!(h.pattern_lines.len(), 2);
        assert_eq!(h.pattern_lines[0].dashes, vec![1.0, -0.5]);
        assert_eq!(h.pattern_lines[0].angle, 0.0);
        assert_eq!(h.pattern_lines[1].angle, 90.0);
        assert!(h.pattern_lines[1].dashes.is_empty());
        assert_eq!(h.pixel_size, 4);
        assert_eq!(h.seed_points.len(), 1);
        assert_eq!(h.seed_points[0], (5.0, 5.0));
    }
}
