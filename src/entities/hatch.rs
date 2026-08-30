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
// Defensive caps — derived from ODA §19.4.75 "practical limits" guidance
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
        /// R2010+ `2RD 12` start tangent.
        start_tangent: Point2D,
        /// R2010+ `2RD 13` end tangent.
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
/// function reads every field defined by §19.4.75, including the
/// boundary path tree.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Hatch> {
    decode_with_strings(c, version, None)
}

/// Decode an R2007+ HATCH whose pattern / gradient names live in the
/// object's string stream (§19.1 split layout, §19.4.75).
///
/// The boundary-path tree and the pattern-line table follow the
/// pattern name, so reading the `TV` inline shifted all of them; the
/// five HATCH records of `sample_AC1032.dwg` recover the names
/// `"ANSI31"`, `"LINEAR"` and `"SOLID"` through the string stream.
/// The record's data fields are not yet proven to end exactly on the
/// string-stream start bit, so this asserts the weaker
/// `<= string_start` bound.
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
        fit_points.reserve(num_fit);
        for _ in 0..num_fit {
            fit_points.push(read_rd2(c)?);
        }
        start_tangent = read_rd2(c)?;
        end_tangent = read_rd2(c)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::string_stream::tests::{bits_of, build_payload};

    /// Write the R2004+ gradient block. §20.4.75 writes it on every
    /// R2004+ HATCH, gradient or not — the flag only says whether the
    /// hatch *uses* it.
    fn write_gradient_block(w: &mut BitWriter, is_gradient: u32, colors: &[(f64, u32)]) {
        w.write_bl(is_gradient as i32);
        w.write_bl(0); // reserved
        w.write_bd(45.0); // angle
        w.write_bd(0.5); // shift
        w.write_bl(1); // single colour
        w.write_bd(0.75); // tint
        w.write_bl(colors.len() as i32);
        for (unknown, rgb) in colors {
            w.write_bd(*unknown);
            w.write_bs(0);
            w.write_bl(*rgb as i32);
            w.write_rc(0);
        }
        // The gradient name `TV` slot; inline on pre-R2007.
        w.write_bs_u(0);
    }

    fn write_tv_8bit(w: &mut BitWriter, bytes: &[u8]) {
        w.write_bs_u(bytes.len() as u16);
        for b in bytes {
            w.write_rc(*b);
        }
    }

    /// Header of a non-gradient R2004 HATCH up to `num_paths`.
    fn write_hatch_header(w: &mut BitWriter, pattern_name: &[u8], solid_fill: bool) {
        write_gradient_block(w, 0, &[]);
        w.write_bd(0.0); // elevation
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // extrusion (0,0,1)
        write_tv_8bit(w, pattern_name);
        w.write_b(solid_fill);
        w.write_b(false); // associative
    }

    /// Tail of a solid-fill hatch: style, pattern type, no pixel size
    /// (no path has the derived bit), no seed points.
    fn write_solid_tail(w: &mut BitWriter) {
        w.write_bs_u(0); // style
        w.write_bs_u(1); // pattern type
        w.write_bl(0); // num seed points
    }

    #[test]
    fn roundtrip_solid_fill_no_paths() {
        let mut w = BitWriter::new();
        write_hatch_header(&mut w, b"SOLID", true);
        w.write_bl(0); // 0 paths
        write_solid_tail(&mut w);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2004).unwrap();
        assert_eq!(h.pattern_name, "SOLID");
        assert!(h.solid_fill);
        assert!(!h.associative);
        assert!(h.paths.is_empty());
        assert!(h.gradient.is_none());
        assert!(h.pattern_lines.is_empty());
        assert!(h.seed_points.is_empty());
        assert_eq!(h.pattern_type, 1);
    }

    #[test]
    fn roundtrip_polyline_path() {
        let mut w = BitWriter::new();
        write_hatch_header(&mut w, b"ANSI31", true);
        w.write_bl(1); // 1 path
        w.write_bl_u(0x02 | 0x10); // polyline + outermost
        w.write_b(false); // has_bulge
        w.write_b(true); // is_closed
        w.write_bl(3); // num vertices
        for (x, y) in [(0.0f64, 0.0), (10.0, 0.0), (10.0, 10.0)] {
            w.write_rd(x);
            w.write_rd(y);
        }
        w.write_bl(0); // num boundary handles
        write_solid_tail(&mut w);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2004).unwrap();
        assert_eq!(h.paths.len(), 1);
        assert_eq!(h.paths[0].flags, 0x12);
        assert_eq!(h.paths[0].num_boundary_handles, 0);
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
        write_hatch_header(&mut w, b"ANSI31", true);
        w.write_bl(1); // 1 path
        w.write_bl_u(0x01); // external (edge list)
        w.write_bl(4); // 4 edges
        let square = [
            ((0.0f64, 0.0), (10.0, 0.0)),
            ((10.0, 0.0), (10.0, 10.0)),
            ((10.0, 10.0), (0.0, 10.0)),
            ((0.0, 10.0), (0.0, 0.0)),
        ];
        for ((sx, sy), (ex, ey)) in square {
            w.write_rc(1); // line
            w.write_rd(sx);
            w.write_rd(sy);
            w.write_rd(ex);
            w.write_rd(ey);
        }
        w.write_bl(2); // num boundary handles — the handles are in the handle stream
        write_solid_tail(&mut w);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2004).unwrap();
        assert_eq!(h.paths.len(), 1);
        assert_eq!(h.paths[0].num_boundary_handles, 2);
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
        write_hatch_header(&mut w, b"SOLID", true);
        w.write_bl(20_000); // over cap
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2004).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("num_paths")),
            "err={err:?}"
        );
    }

    #[test]
    fn decode_errors_on_oversized_segs() {
        let mut w = BitWriter::new();
        write_hatch_header(&mut w, b"SOLID", true);
        w.write_bl(1); // 1 path
        w.write_bl_u(0x01); // edge path
        w.write_bl(200_000); // over cap
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2004).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("num_path_segs")),
            "err={err:?}"
        );
    }

    #[test]
    fn roundtrip_gradient_fill() {
        let mut w = BitWriter::new();
        write_gradient_block(&mut w, 1, &[(0.0, 0x00FF_0000), (1.0, 0x0000_00FF)]);
        w.write_bd(0.0); // elevation
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // extrusion
        write_tv_8bit(&mut w, b"SOLID");
        w.write_b(true); // solid fill
        w.write_b(false); // associative
        w.write_bl(0); // 0 paths
        write_solid_tail(&mut w);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2004).unwrap();
        let g = h.gradient.expect("gradient should be present");
        assert_eq!(g.angle, 45.0);
        assert_eq!(g.shift, 0.5);
        assert_eq!(g.is_single_color, 1);
        assert_eq!(g.tint, 0.75);
        assert_eq!(g.colors.len(), 2);
        assert_eq!(g.colors[0].rgb, 0x00FF_0000);
        assert_eq!(g.colors[1].rgb, 0x0000_00FF);
    }

    /// A non-solid hatch writes the pattern-definition block; a derived
    /// path (`pathflag & 4`) adds the `BD` pixel size.
    #[test]
    fn roundtrip_pattern_lines_and_seed_points() {
        let mut w = BitWriter::new();
        write_hatch_header(&mut w, b"ANSI31", false);
        w.write_bl(1); // one path
        w.write_bl_u(0x04); // derived path → pixel size is written
        w.write_bl(0); // no edges
        w.write_bl(0); // no boundary handles
        w.write_bs_u(1); // style
        w.write_bs_u(1); // pattern type
        w.write_bd(45.0); // pattern angle
        w.write_bd(2.0); // pattern scale
        w.write_b(false); // pattern double
        w.write_bs_u(2); // num pattern lines
        w.write_bd(0.0); // line 0 angle
        w.write_bd(0.0);
        w.write_bd(0.0); // origin
        w.write_bd(1.0);
        w.write_bd(0.0); // offset
        w.write_bs_u(2);
        w.write_bd(1.0);
        w.write_bd(-0.5);
        w.write_bd(90.0); // line 1 angle
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bs_u(0);
        w.write_bd(4.0); // pixel size
        w.write_bl(1); // one seed point
        w.write_rd(5.0);
        w.write_rd(5.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2004).unwrap();
        assert_eq!(h.pattern_style, 1);
        assert_eq!(h.pattern_angle, 45.0);
        assert_eq!(h.pattern_scale, 2.0);
        assert_eq!(h.pattern_lines.len(), 2);
        assert_eq!(h.pattern_lines[0].dashes, vec![1.0, -0.5]);
        assert_eq!(h.pattern_lines[0].angle, 0.0);
        assert_eq!(h.pattern_lines[1].angle, 90.0);
        assert!(h.pattern_lines[1].dashes.is_empty());
        assert_eq!(h.pixel_size, 4.0);
        assert_eq!(h.seed_points.len(), 1);
        assert_eq!(h.seed_points[0], (5.0, 5.0));
    }

    /// The R2018 shape: two strings in the string stream — the gradient
    /// name and then the pattern name — even on a hatch whose gradient
    /// flag is clear, and the data fields ending exactly on the
    /// string-stream start bit.
    #[test]
    fn r2018_split_stream_hatch_reads_both_names() {
        let mut w = BitWriter::new();
        w.write_bs_u(0); // no XDATA
        w.write_b(false); // no graphics preview
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
        // Gradient block with the flag clear — the `TV` slot consumes
        // no data bits on R2007+.
        w.write_bl(0);
        w.write_bl(0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bl(0);
        w.write_bd(0.0);
        w.write_bl(0);
        w.write_bd(0.0); // elevation
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // extrusion, as the sample writes it
        w.write_b(true); // solid fill
        w.write_b(false); // associative
        w.write_bl(0); // no paths
        w.write_bs_u(0); // style
        w.write_bs_u(1); // pattern type
        w.write_bl(0); // no seed points
        let body = bits_of(&w);
        let payload = build_payload(&body, &["LINEAR", "ANSI31"]);
        let h = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert!(h.gradient.is_none());
        assert_eq!(h.pattern_name, "ANSI31");
        assert!(h.solid_fill);
        assert_eq!(h.pattern_type, 1);
    }

    /// The clear-flag early return this decoder used to take consumed
    /// one `TV` too few, so the pattern name came back as the gradient
    /// name and the record misaligned. Guard against the regression.
    #[test]
    fn r2018_clear_gradient_flag_still_consumes_the_gradient_name() {
        let mut w = BitWriter::new();
        w.write_bl(0); // gradient flag clear
        w.write_bl(0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bl(0);
        w.write_bd(0.0);
        w.write_bl(0);
        write_tv_8bit(&mut w, b"LINEAR"); // gradient name, inline on R2004
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        write_tv_8bit(&mut w, b"ANSI31");
        w.write_b(true);
        w.write_b(false);
        w.write_bl(0);
        write_solid_tail(&mut w);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2004).unwrap();
        assert!(h.gradient.is_none());
        assert_eq!(h.pattern_name, "ANSI31");
    }
}
