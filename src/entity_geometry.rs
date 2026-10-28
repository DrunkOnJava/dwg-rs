//! Convert decoded entities (`crate::entities::*`) into the unified
//! [`crate::curve::Curve`] / [`crate::curve::Path`] / [`crate::geometry::Mesh`]
//! types so downstream renderers (SVG, glTF, DXF) don't need to match
//! on `DecodedEntity` for every primitive.
//!
//! These conversions are decoder-independent — they take already-
//! decoded entity structs and translate them to the rendering layer.
//! No new bit reads, no I/O, no allocations beyond the produced
//! `Curve` / `Path` value.
//!
//! Entities that aren't yet decoded by the entity layer (e.g., HATCH
//! boundary paths, MULTILEADER) get partial coverage here — functions
//! adapt to what the decoder produces and document the gap.

use crate::curve::{Curve, Path, PolylineVertex};
use crate::entities::{Point3D, Vec3D};
use crate::error::Result;
use crate::geometry::{Mesh, Transform3};

/// LINE → 2-segment Curve. (L8-11)
pub fn line_to_curve(line: &crate::entities::line::Line) -> Curve {
    Curve::Line {
        a: line.start,
        b: line.end,
    }
}

/// CIRCLE → Curve. (part of L8-12)
pub fn circle_to_curve(circle: &crate::entities::circle::Circle) -> Curve {
    Curve::Circle {
        center: circle.center,
        radius: circle.radius,
        normal: circle.extrusion,
    }
}

/// ARC → Curve. (part of L8-12)
pub fn arc_to_curve(arc: &crate::entities::arc::Arc) -> Curve {
    Curve::Arc {
        center: arc.center,
        radius: arc.radius,
        normal: arc.extrusion,
        start_angle: arc.start_angle,
        end_angle: arc.end_angle,
    }
}

/// ELLIPSE → Curve. (part of L8-12)
pub fn ellipse_to_curve(ellipse: &crate::entities::ellipse::Ellipse) -> Curve {
    Curve::Ellipse {
        center: ellipse.center,
        major_axis: ellipse.major_axis,
        normal: ellipse.extrusion,
        ratio: ellipse.axis_ratio,
        start_angle: ellipse.start_param,
        end_angle: ellipse.end_param,
    }
}

/// POINT → degenerate-line "dot" Curve at (p, p). Renderers can
/// detect degenerate Line and emit a small marker (SVG circle radius
/// 1, glTF tiny mesh, DXF POINT subclass) as appropriate.
pub fn point_to_curve(point: &crate::entities::point::Point) -> Curve {
    Curve::Line {
        a: point.position,
        b: point.position,
    }
}

/// LWPOLYLINE → Path with bulge-aware vertices. (L8-13)
///
/// LWPOLYLINE stores vertices as a flat 2D list with optional
/// per-vertex bulges (dense Vec, indexed parallel to vertices).
/// We synthesize 3D points using the elevation (default 0.0) and
/// copy bulges 1:1.
pub fn lwpolyline_to_path(p: &crate::entities::lwpolyline::LwPolyline) -> Path {
    let elevation = p.elevation.unwrap_or(0.0);
    let vertices: Vec<PolylineVertex> = p
        .vertices
        .iter()
        .enumerate()
        .map(|(i, v)| PolylineVertex {
            point: Point3D {
                x: v.x,
                y: v.y,
                z: elevation,
            },
            bulge: p.bulges.get(i).copied().unwrap_or(0.0),
        })
        .collect();
    Path {
        segments: vec![Curve::Polyline {
            vertices,
            closed: p.closed,
        }],
        closed: p.closed,
    }
}

/// 3DFACE → Mesh with 1 or 2 triangles depending on quad-vs-triangle.
/// (L8-19)
///
/// AutoCAD's convention is that a 4-corner 3DFACE whose corner 4 equals
/// corner 3 represents a triangle (the writer collapses one edge to
/// zero length). This adapter detects that pattern and emits a single
/// triangle instead of a degenerate zero-area quad. The decoder's
/// explicit `is_triangle` flag (from the `hasNoFlagInd` bit) covers the
/// case where the writer omitted corner 4 entirely; this collapse-check
/// covers the case where the writer wrote four corners but with the
/// last two identical.
pub fn three_d_face_to_mesh(face: &crate::entities::three_d_face::ThreeDFace) -> Result<Mesh> {
    let mut m = Mesh::empty();
    let collapsed_quad = !face.is_triangle && face.corners[2] == face.corners[3];
    if face.is_triangle || collapsed_quad {
        m.push_triangle(face.corners[0], face.corners[1], face.corners[2]);
    } else {
        m.push_quad(
            face.corners[0],
            face.corners[1],
            face.corners[2],
            face.corners[3],
        );
    }
    Ok(m)
}

/// Expand one polyline edge with a bulge into an [`Curve::Arc`]. (L8-13)
///
/// # Bulge geometry
///
/// A bulge is the AutoCAD shorthand for a circular arc between two
/// polyline vertices: `bulge = tan(θ / 4)`, where θ is the arc's
/// included angle (signed — positive = CCW left turn, negative = CW
/// right turn). Zero means "straight segment, no arc."
///
/// Given chord `a → b` and bulge `β`:
///
/// ```text
/// θ        = 4 · atan(β)            (signed; CCW positive)
/// chord_len = ‖b − a‖
/// radius   = chord_len / (2 · sin(θ/2))      (|radius|; sign handled separately)
/// sagitta  = β · chord_len / 2              (signed; + = left of a→b)
/// center   = chord_midpoint + perp · (radius − |sagitta|) · sign
/// ```
///
/// The returned [`Curve::Arc`] has `start_angle` / `end_angle` set so
/// that sweeping CCW from start to end traces the original chord
/// direction. Normal defaults to `+Z` — 2D polyline plane. Callers with
/// a non-identity extrusion should post-transform.
///
/// Returns `None` for a zero or non-finite bulge, or for a degenerate
/// chord (both endpoints identical).
pub fn bulge_to_arc(a: Point3D, b: Point3D, bulge: f64) -> Option<Curve> {
    if !bulge.is_finite() || bulge == 0.0 {
        return None;
    }
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let chord_len_sq = dx * dx + dy * dy;
    if chord_len_sq == 0.0 {
        return None;
    }
    let chord_len = chord_len_sq.sqrt();

    // Full included angle θ (signed).
    let theta = 4.0 * bulge.atan();
    let half_theta = theta * 0.5;
    let sin_ht = half_theta.sin();
    if sin_ht == 0.0 {
        return None;
    }
    let radius = chord_len / (2.0 * sin_ht.abs());

    // Midpoint of the chord.
    let mx = (a.x + b.x) * 0.5;
    let my = (a.y + b.y) * 0.5;

    // Unit perpendicular to chord, rotated 90° CCW.
    let inv_cl = 1.0 / chord_len;
    let perp_x = -dy * inv_cl;
    let perp_y = dx * inv_cl;

    // Distance from chord midpoint to circle center.
    //     d = radius · cos(θ/2)
    // Signed: when |bulge| < 1 (acute arc) center sits on the opposite
    // side of the chord from the arc; bulge sign picks which side.
    let cos_ht = half_theta.cos();
    let d = radius * cos_ht;
    let sign = if bulge >= 0.0 { -1.0 } else { 1.0 };
    let cx = mx + perp_x * d * sign;
    let cy = my + perp_y * d * sign;

    let center = Point3D {
        x: cx,
        y: cy,
        z: (a.z + b.z) * 0.5,
    };

    // Start/end angles (CCW convention) in the XY plane, measured from
    // the arc center.
    let start_angle = (a.y - cy).atan2(a.x - cx);
    let end_angle = (b.y - cy).atan2(b.x - cx);

    Some(Curve::Arc {
        center,
        radius,
        normal: Vec3D {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
        start_angle,
        end_angle,
    })
}

/// Expand a sequence of `(point, bulge)` vertices into a [`Path`] of
/// straight-line and arc segments. Used by [`polyline_to_path`] and
/// [`hatch_to_paths`] to implement L8-13 bulge semantics.
///
/// Each bulge applies to the edge between the current vertex and the
/// NEXT one (closing edge for a closed path uses the final vertex's
/// bulge, consistent with AutoCAD's convention).
fn vertices_to_path(vertices: &[PolylineVertex], closed: bool) -> Path {
    if vertices.len() < 2 {
        return Path {
            segments: Vec::new(),
            closed,
        };
    }
    let mut segments = Vec::with_capacity(vertices.len());
    let n = vertices.len();
    let edge_count = if closed { n } else { n - 1 };
    for i in 0..edge_count {
        let v0 = vertices[i];
        let v1 = vertices[(i + 1) % n];
        let seg = match bulge_to_arc(v0.point, v1.point, v0.bulge) {
            Some(arc) => arc,
            None => Curve::Line {
                a: v0.point,
                b: v1.point,
            },
        };
        segments.push(seg);
    }
    Path { segments, closed }
}

/// POLYLINE (2D variant, §19.4.45) → [`Path`]. (L8-13)
///
/// # Entity shape vs. what the decoder exposes
///
/// DWG's legacy 2D POLYLINE stores its vertex list out-of-band — the
/// [`crate::entities::polyline::Polyline`] struct itself holds only
/// flags and global width/thickness; the vertex points live in chained
/// [`crate::entities::vertex::Vertex`] sub-entities referenced by
/// handle. This function therefore takes an explicit `&[Vertex]`
/// slice in addition to the header. When the object-stream walker
/// hands back the VERTEX chain for a POLYLINE, feed both in together.
///
/// The 2D elevation (`polyline.elevation`) promotes each 2D vertex to
/// a 3D `Point3D` so downstream code sees consistent geometry. Per-
/// vertex bulges are carried through via the shared [`bulge_to_arc`]
/// helper.
///
/// # Mesh / polyface variants
///
/// For `polyline.is_polyface()` the VERTEX stream has a different
/// semantics — each "vertex" in the list is either a mesh corner
/// (flag `0x80`) or a polyface face index record. This function does
/// NOT yet split those variants; it treats every entry as a polyline
/// vertex, which produces wrong geometry for POLYFACE. Downstream
/// meshing should guard on `is_polyface()` and dispatch elsewhere.
pub fn polyline_to_path(
    polyline: &crate::entities::polyline::Polyline,
    vertices: &[crate::entities::vertex::Vertex],
) -> Path {
    if polyline.is_3d() {
        polyline_3d_vertices_to_path(vertices, polyline.is_closed())
    } else {
        polyline_2d_vertices_to_path(vertices, polyline.elevation, polyline.is_closed())
    }
}

/// Collect `VERTEX` records into bulge-aware [`PolylineVertex`]es
/// (2D — each vertex's XY is kept, Z is overridden by the POLYLINE
/// elevation). Callers that already pre-project their vertices should
/// prefer [`polyline_3d_vertices_to_path`].
pub fn polyline_2d_vertices_to_path(
    vertices: &[crate::entities::vertex::Vertex],
    elevation: f64,
    closed: bool,
) -> Path {
    let pv: Vec<PolylineVertex> = vertices
        .iter()
        .map(|v| PolylineVertex {
            point: Point3D {
                x: v.location.x,
                y: v.location.y,
                z: elevation,
            },
            bulge: v.bulge,
        })
        .collect();
    vertices_to_path(&pv, closed)
}

/// Collect `VERTEX` records into bulge-aware [`PolylineVertex`]es
/// (3D — each vertex's full 3D location is preserved). Bulges on a
/// 3D polyline are non-standard in AutoCAD but we honour them anyway
/// since the field exists; renderers may choose to ignore them.
pub fn polyline_3d_vertices_to_path(
    vertices: &[crate::entities::vertex::Vertex],
    closed: bool,
) -> Path {
    let pv: Vec<PolylineVertex> = vertices
        .iter()
        .map(|v| PolylineVertex {
            point: v.location,
            bulge: v.bulge,
        })
        .collect();
    vertices_to_path(&pv, closed)
}

/// HATCH → boundary [`Path`]s. (L8-15)
///
/// Each [`crate::entities::hatch::HatchPath`] in the hatch's boundary
/// tree becomes one [`Path`] in the returned `Vec`. The hatch's
/// `elevation` field lifts every 2D boundary point into 3D.
///
/// # Path-type dispatch
///
/// | Input                                               | Output                                          |
/// |-----------------------------------------------------|-------------------------------------------------|
/// | [`HatchPathSegments::Polyline`] (with bulges)       | Line + [`Curve::Arc`] segments via bulge-to-arc |
/// | [`HatchPathSegments::Edges`]::Line                  | [`Curve::Line`]                                 |
/// | [`HatchPathSegments::Edges`]::Arc                   | [`Curve::Arc`] (2D, +Z normal)                  |
/// | [`HatchPathSegments::Edges`]::Ellipse               | [`Curve::Ellipse`] (endpoint treated as major-axis offset from center) |
/// | [`HatchPathSegments::Edges`]::Spline                | [`Curve::Spline`]                               |
///
/// # Ellipse caveat
///
/// `HatchEdge::Ellipse.endpoint` is spec'd as the major-axis endpoint
/// expressed relative to the ellipse center (spec §19.4.33.3) — this
/// function treats it as an offset vector for `Curve::Ellipse::major_axis`,
/// which matches the convention used by [`ellipse_to_curve`] above. The
/// `counter_clockwise` flag is not yet threaded through `Curve::Ellipse`
/// (which always sweeps CCW); callers that need CW sweep should swap
/// the start / end angles.
///
/// [`HatchPathSegments::Polyline`]: crate::entities::hatch::HatchPathSegments::Polyline
/// [`HatchPathSegments::Edges`]: crate::entities::hatch::HatchPathSegments::Edges
pub fn hatch_to_paths(hatch: &crate::entities::hatch::Hatch) -> Vec<Path> {
    use crate::entities::hatch::{HatchEdge, HatchPathSegments};

    let z = hatch.elevation;
    hatch
        .paths
        .iter()
        .map(|hp| match &hp.segments {
            HatchPathSegments::Polyline {
                is_closed,
                vertices,
                ..
            } => {
                let pv: Vec<PolylineVertex> = vertices
                    .iter()
                    .map(|(pt, bulge)| PolylineVertex {
                        point: Point3D {
                            x: pt.x,
                            y: pt.y,
                            z,
                        },
                        bulge: bulge.unwrap_or(0.0),
                    })
                    .collect();
                vertices_to_path(&pv, *is_closed)
            }
            HatchPathSegments::Edges(edges) => {
                let segments: Vec<Curve> = edges
                    .iter()
                    .map(|e| match e {
                        HatchEdge::Line { start, end } => Curve::Line {
                            a: Point3D {
                                x: start.x,
                                y: start.y,
                                z,
                            },
                            b: Point3D {
                                x: end.x,
                                y: end.y,
                                z,
                            },
                        },
                        HatchEdge::Arc {
                            center,
                            radius,
                            start_angle,
                            end_angle,
                            ..
                        } => Curve::Arc {
                            center: Point3D {
                                x: center.x,
                                y: center.y,
                                z,
                            },
                            radius: *radius,
                            normal: Vec3D {
                                x: 0.0,
                                y: 0.0,
                                z: 1.0,
                            },
                            start_angle: *start_angle,
                            end_angle: *end_angle,
                        },
                        HatchEdge::Ellipse {
                            center,
                            endpoint,
                            axis_ratio,
                            start_angle,
                            end_angle,
                            ..
                        } => Curve::Ellipse {
                            center: Point3D {
                                x: center.x,
                                y: center.y,
                                z,
                            },
                            major_axis: Vec3D {
                                x: endpoint.x,
                                y: endpoint.y,
                                z: 0.0,
                            },
                            normal: Vec3D {
                                x: 0.0,
                                y: 0.0,
                                z: 1.0,
                            },
                            ratio: *axis_ratio,
                            start_angle: *start_angle,
                            end_angle: *end_angle,
                        },
                        HatchEdge::Spline {
                            degree,
                            knots,
                            control_points,
                            weights,
                            ..
                        } => Curve::Spline(crate::curve::Spline {
                            degree: *degree,
                            control_points: control_points
                                .iter()
                                .map(|p| Point3D { x: p.x, y: p.y, z })
                                .collect(),
                            weights: weights.clone(),
                            knots: knots.clone(),
                            closed: false,
                        }),
                    })
                    .collect();
                Path {
                    segments,
                    closed: false,
                }
            }
        })
        .collect()
}

/// TEXT → baseline-level [`Curve::TextBaseline`]. (L8-16)
///
/// # Why not glyph polylines
///
/// Tessellating glyph outlines requires a font loader (TrueType /
/// SHX / CFF), Unicode → glyph-id mapping, and a kerning table —
/// none of which is geometry. The DWG decoder owns "where the text
/// goes"; the SVG / glTF layer owns "what the glyphs look like".
/// This function produces the boundary between those two: an
/// anchored, oriented, sized record carrying the literal string.
///
/// # Coordinate handling
///
/// The TEXT entity's `insertion_point` is in 2D OCS; this function
/// promotes it to 3D using `text.elevation` for Z. The OCS → WCS
/// projection via the entity's `extrusion` vector is NOT applied
/// here — callers that need full WCS placement should post-multiply
/// with [`crate::geometry::Transform3::arbitrary_axis`].
///
/// Rotation is the TEXT's `rotation_angle` (radians, CCW around the
/// OCS Z axis). Height is the TEXT's `height` (independent of
/// `width_factor`, which scales character advance, not the
/// baseline-to-cap height).
///
/// # `style_name`
///
/// Always `None` from this function — the DWG STYLE-table reference
/// is carried by the entity's common preamble (handle), not by the
/// TEXT struct, and resolution requires the symbol-table walker.
/// Downstream may post-fill once the STYLE handle is dereferenced.
pub fn text_to_curve(text: &crate::entities::text::Text) -> Result<Curve> {
    let insertion = Point3D {
        x: text.insertion_point.x,
        y: text.insertion_point.y,
        z: text.elevation,
    };
    Ok(Curve::TextBaseline {
        insertion,
        height: text.height,
        rotation: text.rotation_angle,
        content: text.text.clone(),
        style_name: None,
    })
}

/// DIMENSION → ext lines + dim line + arrows + label baseline. (L8-17)
///
/// # Approximation
///
/// A full DIMENSION rendering would require the associated DIMSTYLE
/// (arrowhead kind, tick vs. arrow, text size, extension-line offset,
/// text placement rules). The DIMSTYLE handle isn't dereferenced here,
/// so this function uses geometric defaults derived from the dimension
/// itself: arrowhead size = 3% of the dimension's measured length
/// (clamped to a sensible floor); text height = same. Renderers that
/// need DIMSTYLE-driven sizing should post-multiply or replace.
///
/// # Per-subtype emission
///
/// | Subtype          | Emitted paths                                                                  |
/// |------------------|--------------------------------------------------------------------------------|
/// | Linear           | 2 ext lines, 1 dim line through pt10, 2 arrow triangles, 1 label baseline      |
/// | Aligned          | same as Linear (ext lines and dim line share pt13→pt14 direction)              |
/// | Ordinate         | leader path (feature → endpoint)                                               |
/// | Angular 3-point  | 2 rays (vertex pt15 → pt13, → pt14) + dim-arc chord pt10→pt15                  |
/// | Angular 2-line   | 2 ref-line stubs (pt13→pt14, pt15→pt10)                                        |
/// | Radius           | dim line (center pt10 → chord pt15) + 1 arrow at chord                         |
/// | Diameter         | dim line (chord pt15 → opposite chord pt10) + 2 arrows                         |
///
/// Decoded subtypes outside this list return an empty `Vec` — the
/// honest degradation pattern used elsewhere in this module. Arrows
/// are 2D triangles in the dim-line plane; their tips touch the
/// dim-line endpoints, with base perpendicular to the dim-line
/// direction.
pub fn dimension_to_paths(d: &crate::entities::dimension::Dimension) -> Result<Vec<Path>> {
    use crate::entities::dimension::Dimension as D;
    let paths = match d {
        D::Linear(ld) => linear_dim_paths(ld.def_point_13, ld.def_point_14, ld.def_point_10),
        D::Aligned(ad) => linear_dim_paths(ad.def_point_13, ad.def_point_14, ad.def_point_10),
        D::Ordinate(od) => vec![Path::from_polyline(
            &[od.feature_location_13, od.leader_endpoint_14],
            false,
        )],
        D::Angular3Pt(ad) => vec![
            Path::from_polyline(&[ad.def_point_15, ad.def_point_13], false),
            Path::from_polyline(&[ad.def_point_15, ad.def_point_14], false),
            Path::from_polyline(&[ad.def_point_10, ad.def_point_15], false),
        ],
        D::Angular2Line(ad) => vec![
            Path::from_polyline(&[ad.def_point_13, ad.def_point_14], false),
            Path::from_polyline(&[ad.def_point_15, ad.def_point_10], false),
        ],
        D::Radius(rd) => {
            let mut out = vec![Path::from_polyline(
                &[rd.def_point_10, rd.def_point_15],
                false,
            )];
            let arrow_len = arrow_size(rd.def_point_10, rd.def_point_15);
            if let Some(arrow) = arrow_triangle(rd.def_point_15, rd.def_point_10, arrow_len) {
                out.push(arrow);
            }
            out
        }
        D::Diameter(dd) => {
            let mut out = vec![Path::from_polyline(
                &[dd.def_point_15, dd.def_point_10],
                false,
            )];
            let arrow_len = arrow_size(dd.def_point_15, dd.def_point_10);
            if let Some(a) = arrow_triangle(dd.def_point_15, dd.def_point_10, arrow_len) {
                out.push(a);
            }
            if let Some(a) = arrow_triangle(dd.def_point_10, dd.def_point_15, arrow_len) {
                out.push(a);
            }
            out
        }
    };
    Ok(paths)
}

/// Build the path bundle for a linear or aligned dimension given the
/// two extension-line origins and the on-dim-line measurement point.
///
/// Closed-form construction (no iterative root-finding):
///
/// 1. `axis = pt14 − pt13` is the dim-line direction in 2D
///    (Z is preserved from `pt10`).
/// 2. The dim line passes through `pt10` parallel to `axis` —
///    so the projected endpoints are `pt10 + axis * (proj_t)` where
///    `proj_t` is the parametric projection of `pt13` and `pt14`
///    onto `axis` measured FROM `pt10`.
/// 3. Ext lines connect each definition point to its projected dim-line
///    endpoint.
/// 4. Arrow triangles point inward at each dim-line endpoint.
/// 5. Label baseline is centered on the dim line.
fn linear_dim_paths(pt13: Point3D, pt14: Point3D, pt10: Point3D) -> Vec<Path> {
    let ax = pt14.x - pt13.x;
    let ay = pt14.y - pt13.y;
    let len_sq = ax * ax + ay * ay;
    if len_sq == 0.0 {
        return vec![Path::from_polyline(&[pt13, pt10], false)];
    }
    let inv_len = 1.0 / len_sq.sqrt();
    let unit_x = ax * inv_len;
    let unit_y = ay * inv_len;

    let dx_13 = pt13.x - pt10.x;
    let dy_13 = pt13.y - pt10.y;
    let t_13 = dx_13 * unit_x + dy_13 * unit_y;
    let dx_14 = pt14.x - pt10.x;
    let dy_14 = pt14.y - pt10.y;
    let t_14 = dx_14 * unit_x + dy_14 * unit_y;

    let dim_a = Point3D {
        x: pt10.x + unit_x * t_13,
        y: pt10.y + unit_y * t_13,
        z: pt10.z,
    };
    let dim_b = Point3D {
        x: pt10.x + unit_x * t_14,
        y: pt10.y + unit_y * t_14,
        z: pt10.z,
    };

    let arrow_len = arrow_size(dim_a, dim_b);
    let mid = Point3D {
        x: (dim_a.x + dim_b.x) * 0.5,
        y: (dim_a.y + dim_b.y) * 0.5,
        z: dim_a.z,
    };
    let label_rotation = unit_y.atan2(unit_x);

    let mut out = Vec::with_capacity(6);
    out.push(Path::from_polyline(&[pt13, dim_a], false));
    out.push(Path::from_polyline(&[pt14, dim_b], false));
    out.push(Path::from_polyline(&[dim_a, dim_b], false));
    if let Some(arrow) = arrow_triangle(dim_a, dim_b, arrow_len) {
        out.push(arrow);
    }
    if let Some(arrow) = arrow_triangle(dim_b, dim_a, arrow_len) {
        out.push(arrow);
    }
    out.push(Path {
        segments: vec![Curve::TextBaseline {
            insertion: mid,
            height: arrow_len,
            rotation: label_rotation,
            content: String::new(),
            style_name: None,
        }],
        closed: false,
    });
    out
}

/// Choose a default arrow size from the dim-line length: 3% of length,
/// floored at a small absolute value so micro-dimensions still render.
fn arrow_size(a: Point3D, b: Point3D) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len = (dx * dx + dy * dy).sqrt();
    (len * 0.03).max(1e-3)
}

/// Closed-form arrow triangle whose tip is at `tip` pointing AWAY from
/// `other` (i.e., outward along the line from `other` to `tip`). The
/// base is perpendicular to that direction, length = `size`.
///
/// Returns `None` when `tip == other` (no orientation available).
fn arrow_triangle(tip: Point3D, other: Point3D, size: f64) -> Option<Path> {
    let dx = tip.x - other.x;
    let dy = tip.y - other.y;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return None;
    }
    let inv_len = 1.0 / len_sq.sqrt();
    let ux = dx * inv_len;
    let uy = dy * inv_len;
    let base_cx = tip.x - ux * size;
    let base_cy = tip.y - uy * size;
    let half = size * 0.5;
    let px = -uy;
    let py = ux;
    let b1 = Point3D {
        x: base_cx + px * half,
        y: base_cy + py * half,
        z: tip.z,
    };
    let b2 = Point3D {
        x: base_cx - px * half,
        y: base_cy - py * half,
        z: tip.z,
    };
    Some(Path::from_polyline(&[tip, b1, b2], true))
}

/// SPLINE → [`Curve::Spline`] (NURBS) or fit-points polyline. (L8-14)
///
/// # Branch handling
///
/// SPLINE has two on-wire forms (§19.4.44):
///
/// - **Control form** (`scenario == 1`): degree, knots, control points,
///   and optional weights. Maps directly to [`Curve::Spline`] wrapping a
///   [`crate::curve::Spline`]. When the entity is rational with all
///   weights equal, the weights vector is dropped to the empty `vec![]`
///   (downstream B-spline semantics — no rational evaluation needed).
///
/// - **Fit form** (`scenario == 2`): a list of points the curve passes
///   through plus tangent constraints, but no control polygon. Honest
///   degradation: emit the fit points as a `Curve::Polyline` (no bulges
///   — fit-point splines are smooth, not piecewise-circular). Renderers
///   that want a true fit-spline can run a separate "fit-to-control"
///   solver downstream; this adapter stays in the geometry-passthrough
///   role.
///
/// # Degree clamping
///
/// The on-wire `degree` is `f64` (the spec leaves room for fractional
/// values though all practical writers store an integer). This adapter
/// truncates to `u32`, clamping to `1..=15` so a corrupt or insane
/// degree doesn't propagate.
pub fn spline_to_curve(spline: &crate::entities::spline::Spline) -> Result<Curve> {
    if let Some(ctrl) = &spline.control {
        let degree = clamp_spline_degree(spline.degree);
        let weights =
            if ctrl.rational && !ctrl.weights.is_empty() && !weights_all_equal(&ctrl.weights) {
                ctrl.weights.clone()
            } else {
                Vec::new()
            };
        return Ok(Curve::Spline(crate::curve::Spline {
            degree,
            control_points: ctrl.control_points.clone(),
            weights,
            knots: ctrl.knots.clone(),
            closed: ctrl.closed,
        }));
    }

    if let Some(fit) = &spline.fit {
        let vertices: Vec<PolylineVertex> = fit
            .fit_points
            .iter()
            .map(|p| PolylineVertex {
                point: *p,
                bulge: 0.0,
            })
            .collect();
        return Ok(Curve::Polyline {
            vertices,
            closed: false,
        });
    }

    Ok(Curve::Polyline {
        vertices: Vec::new(),
        closed: false,
    })
}

/// Clamp the on-wire SPLINE degree (a `BL` per §20.4.40) into a
/// reasonable `u32` range for the renderer.
fn clamp_spline_degree(d: i32) -> u32 {
    d.clamp(1, 15) as u32
}

/// True when every entry in `weights` equals the first within a tight
/// tolerance — a rational spline whose weights are all equal degenerates
/// to a non-rational B-spline and the weights vector is redundant.
fn weights_all_equal(weights: &[f64]) -> bool {
    if let Some(&first) = weights.first() {
        weights.iter().all(|w| (w - first).abs() < 1e-12)
    } else {
        true
    }
}

/// INSERT → composite instance transform. (L8-18)
///
/// Composes (in apply-order):
///
/// 1. **Scale** about the local origin (`scale.x`, `scale.y`, `scale.z`).
/// 2. **Z-axis rotation** by `rotation` radians.
/// 3. **Arbitrary-axis projection** from the OCS defined by `extrusion`
///    to WCS (per [`Transform3::arbitrary_axis`]).
/// 4. **Translation** to the WCS `insertion_point`.
///
/// The returned transform takes a point in the BLOCK's local coordinate
/// frame to that point's WCS position when the block is placed by this
/// INSERT. Block expansion (walking the BLOCK body and applying this
/// transform to each contained entity's geometry) is a separate pass —
/// see L5-05 in the roadmap.
pub fn insert_to_transform(insert: &crate::entities::insert::Insert) -> Transform3 {
    let scale = Transform3::scale(insert.scale.x, insert.scale.y, insert.scale.z);
    let rotate = Transform3::rotation_z(insert.rotation);
    let extrude = Transform3::arbitrary_axis(insert.extrusion);
    let translate = Transform3::translation(
        insert.insertion_point.x,
        insert.insertion_point.y,
        insert.insertion_point.z,
    );
    // `compose_chain` applies in REVERSE chain order to a point: the
    // last entry is the innermost (applied first), the first entry is
    // the outermost (applied last). For INSERT we want
    // scale → rotate → extrude → translate, so the chain reverses to
    // [translate, extrude, rotate, scale].
    Transform3::compose_chain(&[translate, extrude, rotate, scale])
}

/// 3DSOLID → axis-aligned bbox placeholder mesh. (L8-20)
///
/// # Honest degradation
///
/// 3DSOLID stores its B-rep as an opaque ACIS SAT (Standard ACIS Text)
/// blob. Decoding ACIS is a separate, large project (the Spatial Corp
/// kernel format has its own multi-thousand-line parser); this crate
/// surfaces the blob via [`crate::entities::three_d_solid::ThreeDSolid::sat_blob`]
/// for downstream consumers but does NOT tessellate it.
///
/// To keep the renderer pipeline working in the meantime, this adapter
/// emits an empty mesh. Per the SECURITY-side principle "no panics,
/// every parser returns Result", we return `Ok(Mesh::empty())` rather
/// than `Error::Unsupported` — the caller can detect the empty mesh
/// (`mesh.vertices.is_empty()`) and skip / substitute. Returning an
/// error here would force every walk caller to special-case 3DSOLID.
pub fn three_d_solid_to_mesh(_solid: &crate::entities::three_d_solid::ThreeDSolid) -> Result<Mesh> {
    // ACIS SAT decoding is intentionally out of scope here — see the
    // module docstring on `crate::entities::three_d_solid` for the
    // "concatenated SAT blob" handoff. Returning an empty mesh keeps
    // the renderer pipeline iterating; downstream callers can substitute
    // a bbox or "??" placeholder if they have one.
    Ok(Mesh::empty())
}
