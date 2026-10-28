//! HATCH entity (§20.4.75) — filled hatch region.
//!
//! HATCH is among the densest entity types in DWG: every instance
//! encodes a gradient-fill block (R2004+), an elevation, an extrusion,
//! a pattern name, a boundary-path tree (lines / arcs / ellipses /
//! splines or an explicit polyline), a pattern-line definition, and
//! a seed-point list.
//!
//! # Field list (§20.4.75)
//!
//! ```text
//! (R2004+)
//!   BL   is_gradient_fill          -- written on every R2004+ HATCH
//!   BL   reserved
//!   BD   gradient_angle
//!   BD   gradient_shift
//!   BL   is_single_color_gradient
//!   BD   gradient_tint
//!   BL   num_gradient_colors       -- cap 16
//!   for each: BD unknown, BS unknown, BL rgb, RC ignored
//!   TV   gradient_name
//! BD   elevation
//! 3BD  extrusion
//! TV   pattern_name
//! B    solid_fill
//! B    associative
//! BL   num_paths                   -- cap 10_000
//! for each path:
//!   BL   path_flags
//!   (if path_flags & 2: polyline)
//!     B    has_bulge
//!     B    is_closed
//!     BL   num_vertices
//!     for each: 2RD vertex, (if has_bulge) BD bulge
//!   (else: edges)
//!     BL   num_edges
//!     for each: RC edge type (1 line, 2 arc, 3 ellipse, 4 spline)
//!       (line)    2RD start, 2RD end
//!       (arc)     2RD center, BD radius, BD start_angle, BD end_angle, B ccw
//!       (ellipse) 2RD center, 2RD endpoint, BD axis_ratio,
//!                 BD start_angle, BD end_angle, B ccw
//!       (spline)  BL degree, B is_rational, B is_periodic,
//!                 BL num_knots, BL num_control_points,
//!                 BD[num_knots] knots,
//!                 per control point: 2RD point, (if is_rational) BD weight,
//!                 (R2010+) BL num_fit_points,
//!                   (if num_fit_points > 0) 2RD[num_fit_points] fit points,
//!                   2RD start tangent, 2RD end tangent
//!   BL   num_boundary_handles      -- cap 1024; the handles are in the handle stream
//! BS   pattern_style
//! BS   pattern_type
//! (if !solid_fill)
//!   BD   pattern_angle
//!   BD   pattern_scale_or_spacing
//!   B    pattern_double
//!   BS   num_pattern_lines         -- cap 4096
//!   for each: BD angle, 2BD origin, 2BD offset,
//!             BS num_dashes (cap 64), BD[num_dashes] dashes
//! (if any path_flags & 4) BD pixel_size
//! BL   num_seed_points             -- cap 1024
//! 2RD[num_seed_points] seed_points
//! ```
//!
//! # Measured: a spline edge without fit points has no tangents
//!
//! §20.4.75 prints the R2010+ spline-edge tail as `BL numfitpoints`, the
//! fit points, then `2RD` start and end tangents with no condition.
//! The one spline edge in the corpus, HATCH `0xAA8` of ACadSharp's
//! `export_sample.dwg` (R2018), has 0 fit points and closes on its
//! string stream only without the two tangents. Read with them, its
//! `num_boundary_handles` decodes to 298,029,779. Without them, the
//! rest of the record reads one boundary handle, the ANSI31 pattern
//! (45°, offset (−0.0884, 0.0884): the 0.125 spacing), a pixel size
//! and one seed point at (5.13, 2.59), and ends on bit 2827, the
//! string-stream start, with both strings consumed. The edge's 9
//! control points form a closed degree-3 curve that starts and ends at
//! (4.43, 2.59), the first and last fit point of SPLINE `0xA7F`, and the
//! seed point lies inside it (ARCHITECTURE §7y). The tangents
//! are therefore read only when there are fit points, which keeps the
//! spec's order for that case. No corpus record has a spline edge with
//! fit points, so that case is unmeasured.
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
//! | pattern lines          | 4_096   |
//! | dashes per pattern line| 64      |
//! | seed points            | 1_024   |
//!
//! Each cap is paired with a remaining-bits sanity check so a claimed
//! count larger than the object's payload can possibly encode is
//! rejected immediately — defense against adversarial or truncated
//! streams.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Vec3D, read_bd2, read_bd3, read_rd2};
use crate::error::{Error, Result};
use crate::string_stream::{self, StringReader};
use crate::tables::modern;
use crate::version::Version;

// ========================================================================
// Defensive caps — derived from ODA §20.4.75 "practical limits" guidance
// cross-checked against observed worst-case values in real drawings.
// ========================================================================
const CAP_GRADIENT_COLORS: usize = 16;
const CAP_PATHS: usize = 10_000;
const CAP_PATH_SEGS: usize = 100_000;
const CAP_BOUNDARY_HANDLES: usize = 1_024;
const CAP_PATTERN_LINES: usize = 4_096;
const CAP_LINE_DASHES: usize = 64;
const CAP_SEED_POINTS: usize = 1_024;

/// `pathflag & 2` — the path is a polyline rather than an edge list.
const FLAG_POLYLINE: u32 = 0x02;
/// `pathflag & 4` — the path is derived from a boundary object. A hatch
/// with any such path carries the `BD` pixel-size hint (§20.4.75).
const FLAG_DERIVED: u32 = 0x04;

#[derive(Debug, Clone, PartialEq)]
pub struct Hatch {
    pub gradient: Option<GradientFill>,
    pub elevation: f64,
    pub extrusion: Vec3D,
    pub pattern_name: String,
    pub solid_fill: bool,
    pub associative: bool,
    pub paths: Vec<HatchPath>,
    /// `BS 75` hatch style: 0 odd parity, 1 outermost, 2 whole area.
    pub pattern_style: u16,
    /// `BS 76` pattern type: 0 user-defined, 1 predefined, 2 custom.
    pub pattern_type: u16,
    /// `BD 52` — only written when the hatch is not a solid fill.
    pub pattern_angle: f64,
    /// `BD 41` — only written when the hatch is not a solid fill.
    pub pattern_scale: f64,
    /// `B 77` — only written when the hatch is not a solid fill.
    pub pattern_double: bool,
    pub pattern_lines: Vec<PatternLine>,
    /// `BD 47` — only written when some path flag has bit 4 set.
    pub pixel_size: f64,
    pub seed_points: Vec<(f64, f64)>,
}

/// Gradient-fill block (§20.4.75, R2004+).
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
    /// `BD 463`.
    pub unknown_double: f64,
    /// `BS` — undocumented in §20.4.75 beyond its type.
    pub unknown_short: i16,
    /// `BL 63/421` RGB word.
    pub rgb: u32,
    /// `RC` — the spec names this the "ignored color byte".
    pub ignored_color_byte: u8,
}

/// A single boundary path in the HATCH tree. Stores the raw
/// `path_type_flags` bitset (`1` external, `2` polyline, `4` derived,
/// `8` textbox, `16` outermost) plus the segments themselves.
#[derive(Debug, Clone, PartialEq)]
pub struct HatchPath {
    pub flags: u32,
    pub segments: HatchPathSegments,
    /// `BL 97` — the number of boundary-object handles this path owns.
    /// The handles themselves are **not** data-stream fields: §20.4.75
    /// puts them in the record's "Common Entity Handle Data", after the
    /// data stream, so the count is all the data stream carries.
    pub num_boundary_handles: u32,
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
        /// R2010+ only (`R24` in §20.4.75); empty on earlier releases.
        fit_points: Vec<Point2D>,
        /// R2010+ `2RD 12` start tangent; zero when there are no fit
        /// points, since the record then stores none (module docs).
        start_tangent: Point2D,
        /// R2010+ `2RD 13` end tangent; zero when there are no fit points.
        end_tangent: Point2D,
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
/// function reads every field defined by §20.4.75, including the
/// boundary path tree.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Hatch> {
    decode_with_strings(c, version, None)
}

/// Decode an R2007+ HATCH whose pattern / gradient names live in the
/// object's string stream (§19.1 split layout, §20.4.75).
///
/// The boundary-path tree and the pattern-line table follow the
/// pattern name, so reading the `TV` inline shifted all of them; the
/// HATCH records of `sample_AC1032.dwg` recover the names `"ANSI31"`,
/// `"LINEAR"` and `"SOLID"` through the string stream. The data
/// fields must end exactly on the string-stream start bit.
pub fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<Hatch> {
    let (mut strings, string_start) = modern::open_entity(payload, version)?;
    let mut c = BitCursor::new(payload);
    string_stream::seek(&mut c, object_body_start)?;
    crate::common_entity::read_common_entity_data(&mut c, version)?;
    let hatch = decode_with_strings(&mut c, version, Some(&mut strings))?;
    let at = c.position_bits();
    if at != string_start {
        return Err(modern::misaligned("HATCH", at, string_start));
    }
    Ok(hatch)
}

fn decode_with_strings(
    c: &mut BitCursor<'_>,
    version: Version,
    mut strings: Option<&mut StringReader<'_>>,
) -> Result<Hatch> {
    let gradient = decode_gradient(c, version, strings.as_deref_mut())?;

    let elevation = c.read_bd()?;
    let extrusion = read_bd3(c)?;
    let pattern_name = modern::read_tv_field(c, version, strings)?;
    let solid_fill = c.read_b()?;
    let associative = c.read_b()?;

    let num_paths = c.read_bl_u()? as usize;
    bounds_check(num_paths, "num_paths", CAP_PATHS, c.remaining_bits())?;
    let mut paths = Vec::with_capacity(num_paths);
    let mut any_derived_path = false;
    for _ in 0..num_paths {
        let path = decode_path(c, version)?;
        any_derived_path |= path.flags & FLAG_DERIVED != 0;
        paths.push(path);
    }

    let pattern_style = c.read_bs_u()?;
    let pattern_type = c.read_bs_u()?;

    // §20.4.75 guards the whole pattern-definition block with
    // `if (!solidfill)`. A solid hatch writes none of it.
    let mut pattern_angle = 0.0;
    let mut pattern_scale = 0.0;
    let mut pattern_double = false;
    let mut pattern_lines = Vec::new();
    if !solid_fill {
        pattern_angle = c.read_bd()?;
        pattern_scale = c.read_bd()?;
        pattern_double = c.read_b()?;
        let num_pattern_lines = c.read_bs_u()? as usize;
        bounds_check(
            num_pattern_lines,
            "num_pattern_lines",
            CAP_PATTERN_LINES,
            c.remaining_bits(),
        )?;
        pattern_lines.reserve(num_pattern_lines);
        for _ in 0..num_pattern_lines {
            pattern_lines.push(decode_pattern_line(c)?);
        }
    }

    // §20.4.75: `if (ANY of the pathflags & 4) { pixelsize BD }`.
    let pixel_size = if any_derived_path { c.read_bd()? } else { 0.0 };

    let num_seed_points = c.read_bl_u()? as usize;
    bounds_check(
        num_seed_points,
        "num_seed_points",
        CAP_SEED_POINTS,
        c.remaining_bits(),
    )?;
    let mut seed_points = Vec::with_capacity(num_seed_points);
    for _ in 0..num_seed_points {
        let p = read_rd2(c)?;
        seed_points.push((p.x, p.y));
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
        pattern_type,
        pattern_angle,
        pattern_scale,
        pattern_double,
        pattern_lines,
        pixel_size,
        seed_points,
    })
}

fn decode_gradient(
    c: &mut BitCursor<'_>,
    version: Version,
    strings: Option<&mut StringReader<'_>>,
) -> Result<Option<GradientFill>> {
    if !version.is_r2004_plus() {
        return Ok(None);
    }
    // §20.4.75 types the gradient flag as a `BL`, not a `BS`, and does
    // **not** guard the rest of the block behind it: the whole gradient
    // record — including its `TV` name — is written on every R2004+
    // HATCH. Measured on `sample_AC1032.dwg`, where all eight HATCH
    // records carry two strings in their string stream (`"LINEAR"` then
    // the pattern name `"ANSI31"` / `"AR-PARQ1"` / `"HVEGE100"` / ...)
    // even though six of them have the flag clear. Returning early on a
    // clear flag consumed one `TV` too few and shifted the boundary
    // path tree.
    let is_gradient_fill = c.read_bl_u()?;
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
        let unknown_short = c.read_bs()?;
        let rgb = c.read_bl_u()?;
        let ignored_color_byte = c.read_rc()?;
        colors.push(GradientColor {
            unknown_double,
            unknown_short,
            rgb,
            ignored_color_byte,
        });
    }
    let name = modern::read_tv_field(c, version, strings)?;
    if is_gradient_fill == 0 {
        return Ok(None);
    }
    Ok(Some(GradientFill {
        angle,
        shift,
        is_single_color,
        tint,
        colors,
        name,
    }))
}

fn decode_path(c: &mut BitCursor<'_>, version: Version) -> Result<HatchPath> {
    let flags = c.read_bl_u()?;
    let segments = if flags & FLAG_POLYLINE != 0 {
        decode_polyline_path(c)?
    } else {
        decode_edge_path(c, version)?
    };
    let num_boundary_handles = c.read_bl_u()?;
    bounds_check(
        num_boundary_handles as usize,
        "num_boundary_handles",
        CAP_BOUNDARY_HANDLES,
        c.remaining_bits(),
    )?;
    Ok(HatchPath {
        flags,
        segments,
        num_boundary_handles,
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
        let vertex = read_rd2(c)?;
        let bulge = if has_bulge { Some(c.read_bd()?) } else { None };
        vertices.push((vertex, bulge));
    }
    Ok(HatchPathSegments::Polyline {
        has_bulge,
        is_closed,
        vertices,
    })
}

fn decode_edge_path(c: &mut BitCursor<'_>, version: Version) -> Result<HatchPathSegments> {
    let num_edges = c.read_bl_u()? as usize;
    bounds_check(
        num_edges,
        "num_path_segs (edges)",
        CAP_PATH_SEGS,
        c.remaining_bits(),
    )?;
    let mut edges = Vec::with_capacity(num_edges);
    for _ in 0..num_edges {
        edges.push(decode_edge(c, version)?);
    }
    Ok(HatchPathSegments::Edges(edges))
}

fn decode_edge(c: &mut BitCursor<'_>, version: Version) -> Result<HatchEdge> {
    let seg_type = c.read_rc()?;
    match seg_type {
        1 => Ok(HatchEdge::Line {
            start: read_rd2(c)?,
            end: read_rd2(c)?,
        }),
        2 => Ok(HatchEdge::Arc {
            center: read_rd2(c)?,
            radius: c.read_bd()?,
            start_angle: c.read_bd()?,
            end_angle: c.read_bd()?,
            counter_clockwise: c.read_b()?,
        }),
        3 => Ok(HatchEdge::Ellipse {
            center: read_rd2(c)?,
            endpoint: read_rd2(c)?,
            axis_ratio: c.read_bd()?,
            start_angle: c.read_bd()?,
            end_angle: c.read_bd()?,
            counter_clockwise: c.read_b()?,
        }),
        4 => decode_spline_edge(c, version),
        _ => Err(Error::SectionMap(format!(
            "HATCH edge seg_type {seg_type} not in {{1 line, 2 arc, 3 ellipse, 4 spline}}"
        ))),
    }
}

fn decode_spline_edge(c: &mut BitCursor<'_>, version: Version) -> Result<HatchEdge> {
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
    let mut weights = Vec::new();
    for _ in 0..num_control {
        control_points.push(read_rd2(c)?);
        if is_rational {
            weights.push(c.read_bd()?);
        }
    }
    // The fit-point block is tagged `R24` in §20.4.75 — R2010 and later.
    // Its tangents follow only when there are fit points (module docs).
    let mut fit_points = Vec::new();
    let mut start_tangent = Point2D::default();
    let mut end_tangent = Point2D::default();
    if version.is_r2010_plus() {
        let num_fit = c.read_bl_u()? as usize;
        bounds_check(
            num_fit,
            "spline fit_points",
            CAP_PATH_SEGS,
            c.remaining_bits(),
        )?;
        if num_fit > 0 {
            fit_points.reserve(num_fit);
            for _ in 0..num_fit {
                fit_points.push(read_rd2(c)?);
            }
            start_tangent = read_rd2(c)?;
            end_tangent = read_rd2(c)?;
        }
    }
    Ok(HatchEdge::Spline {
        degree,
        is_rational,
        is_periodic,
        knots,
        control_points,
        weights,
        fit_points,
        start_tangent,
        end_tangent,
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
