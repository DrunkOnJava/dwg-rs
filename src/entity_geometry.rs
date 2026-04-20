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
use crate::geometry::Mesh;

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
pub fn three_d_face_to_mesh(face: &crate::entities::three_d_face::ThreeDFace) -> Mesh {
    let mut m = Mesh::empty();
    if face.is_triangle {
        m.push_triangle(face.corners[0], face.corners[1], face.corners[2]);
    } else {
        m.push_quad(
            face.corners[0],
            face.corners[1],
            face.corners[2],
            face.corners[3],
        );
    }
    m
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

/// TEXT → baseline anchor line. (L8-16, stub)
///
/// # Approximation
///
/// This function does NOT render glyph outlines — that's L9-05
/// territory (font fallback, character polylines, Unicode mapping).
/// Instead it emits a single straight line from the text's insertion
/// point along the baseline direction, with length equal to the text
/// height. Renderers can:
///
/// 1. Draw the line as a debug anchor marking where text would appear.
/// 2. Overlay the literal `text.text` string at `insertion_point`
///    rotated by `text.rotation_angle`.
/// 3. Compute a bounding box using height × width_factor × char count
///    as a fallback extent.
///
/// The insertion point is promoted to 3D using `text.elevation` (Z).
/// The baseline direction is `(cos(θ), sin(θ), 0)` where θ is the
/// TEXT's `rotation_angle` in radians, matching the AutoCAD WCS
/// convention. No OCS → WCS projection via `extrusion` is applied
/// here — callers that need true 3D placement for tilted UCS should
/// post-transform.
pub fn text_to_curve(text: &crate::entities::text::Text) -> Curve {
    let a = Point3D {
        x: text.insertion_point.x,
        y: text.insertion_point.y,
        z: text.elevation,
    };
    let dx = text.rotation_angle.cos() * text.height;
    let dy = text.rotation_angle.sin() * text.height;
    let b = Point3D {
        x: a.x + dx,
        y: a.y + dy,
        z: a.z,
    };
    Curve::Line { a, b }
}

/// DIMENSION → constituent dimension-line / extension-line / arrow
/// primitives. (L8-17, skeletal)
///
/// # Approximation
///
/// A full DIMENSION rendering would require the associated DIMSTYLE
/// (arrowhead kind, tick vs. arrow, text size, extension-line offset,
/// text placement rules) — this crate does not yet wire the DIMSTYLE
/// lookup through. The function instead emits the geometric skeleton
/// derivable from the dimension's definition points alone:
///
/// | Subtype          | Emitted paths                                                              |
/// |------------------|----------------------------------------------------------------------------|
/// | Linear           | dim-line (pt13→pt14), two extension lines (pt13→pt10, pt14→pt10)           |
/// | Aligned          | same as Linear                                                             |
/// | Ordinate         | leader (pt13→pt14)                                                         |
/// | Angular 3-point  | two rays (pt15→pt13, pt15→pt14), dim-arc chord (pt10→pt15)                 |
/// | Angular 2-line   | four rays (pt13→pt14, pt15→pt10), reference point stub                     |
/// | Radius           | dim-line (pt10→pt15)                                                       |
/// | Diameter         | dim-line (pt15→pt10)                                                       |
///
/// Arrowhead triangles are NOT emitted — rendering an arrow requires
/// the DIMSTYLE block reference or fixed arrow scale, neither of which
/// is reachable from this struct alone. Renderers that need arrows
/// should synthesize them from the dimension-line endpoints and their
/// own arrow style.
pub fn dimension_to_paths(d: &crate::entities::dimension::Dimension) -> Vec<Path> {
    use crate::entities::dimension::Dimension as D;
    match d {
        D::Linear(ld) => vec![
            Path::from_polyline(&[ld.def_point_13, ld.def_point_14], false),
            Path::from_polyline(&[ld.def_point_13, ld.def_point_10], false),
            Path::from_polyline(&[ld.def_point_14, ld.def_point_10], false),
        ],
        D::Aligned(ad) => vec![
            Path::from_polyline(&[ad.def_point_13, ad.def_point_14], false),
            Path::from_polyline(&[ad.def_point_13, ad.def_point_10], false),
            Path::from_polyline(&[ad.def_point_14, ad.def_point_10], false),
        ],
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
        D::Radius(rd) => vec![Path::from_polyline(
            &[rd.def_point_10, rd.def_point_15],
            false,
        )],
        D::Diameter(dd) => vec![Path::from_polyline(
            &[dd.def_point_15, dd.def_point_10],
            false,
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_to_curve_preserves_endpoints() {
        let l = crate::entities::line::Line {
            start: Point3D::new(1.0, 2.0, 3.0),
            end: Point3D::new(4.0, 5.0, 6.0),
            thickness: 0.0,
            extrusion: crate::entities::Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            is_2d: false,
        };
        match line_to_curve(&l) {
            Curve::Line { a, b } => {
                assert_eq!(a, l.start);
                assert_eq!(b, l.end);
            }
            _ => panic!("expected Curve::Line"),
        }
    }

    #[test]
    fn point_to_curve_emits_degenerate_line() {
        let p = crate::entities::point::Point {
            position: Point3D::new(7.0, 8.0, 9.0),
            thickness: 0.0,
            extrusion: crate::entities::Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            x_axis_angle: 0.0,
        };
        match point_to_curve(&p) {
            Curve::Line { a, b } => {
                assert_eq!(a, p.position);
                assert_eq!(b, p.position);
            }
            _ => panic!("expected degenerate Curve::Line"),
        }
    }

    #[test]
    fn circle_to_curve_preserves_center_and_radius() {
        let c = crate::entities::circle::Circle {
            center: Point3D::new(10.0, 20.0, 0.0),
            radius: 5.0,
            thickness: 0.0,
            extrusion: crate::entities::Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        };
        match circle_to_curve(&c) {
            Curve::Circle { center, radius, .. } => {
                assert_eq!(center, c.center);
                assert_eq!(radius, c.radius);
            }
            _ => panic!("expected Curve::Circle"),
        }
    }

    // ------------------------------------------------------------
    // L8-13 — POLYLINE bulge expansion
    // ------------------------------------------------------------

    /// A 180° (semicircle) arc corresponds to bulge = 1.0. For a unit
    /// chord along +X the arc must have radius 0.5 and center at the
    /// chord midpoint (0.5, 0).
    #[test]
    fn bulge_to_arc_semicircle_unit_chord() {
        let a = Point3D::new(0.0, 0.0, 0.0);
        let b = Point3D::new(1.0, 0.0, 0.0);
        let arc = bulge_to_arc(a, b, 1.0).expect("semicircle should decode");
        match arc {
            Curve::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ..
            } => {
                assert!((center.x - 0.5).abs() < 1e-12, "center.x = {}", center.x);
                assert!((center.y - 0.0).abs() < 1e-12, "center.y = {}", center.y);
                assert!((radius - 0.5).abs() < 1e-12, "radius = {}", radius);
                // Two angles must differ by π (semicircle).
                let sweep = (end_angle - start_angle).abs();
                let diff = (sweep - std::f64::consts::PI).abs();
                assert!(diff < 1e-12, "sweep = {}", sweep);
            }
            other => panic!("expected Arc, got {other:?}"),
        }
    }

    /// Zero bulge yields `None` (caller substitutes a `Curve::Line`).
    #[test]
    fn bulge_to_arc_zero_bulge_is_none() {
        let a = Point3D::new(0.0, 0.0, 0.0);
        let b = Point3D::new(1.0, 0.0, 0.0);
        assert!(bulge_to_arc(a, b, 0.0).is_none());
    }

    /// Degenerate chord (a == b) is also `None`.
    #[test]
    fn bulge_to_arc_degenerate_chord_is_none() {
        let a = Point3D::new(3.0, 4.0, 5.0);
        assert!(bulge_to_arc(a, a, 0.25).is_none());
    }

    /// Non-finite bulges are defensive `None`.
    #[test]
    fn bulge_to_arc_non_finite_is_none() {
        let a = Point3D::new(0.0, 0.0, 0.0);
        let b = Point3D::new(1.0, 0.0, 0.0);
        assert!(bulge_to_arc(a, b, f64::NAN).is_none());
        assert!(bulge_to_arc(a, b, f64::INFINITY).is_none());
    }

    /// Bulge sign toggles which side of the chord the arc bulges
    /// towards — the arc center flips across the chord.
    #[test]
    fn bulge_to_arc_sign_flips_center_side() {
        let a = Point3D::new(0.0, 0.0, 0.0);
        let b = Point3D::new(2.0, 0.0, 0.0);
        let pos = bulge_to_arc(a, b, 0.5).unwrap();
        let neg = bulge_to_arc(a, b, -0.5).unwrap();
        match (pos, neg) {
            (Curve::Arc { center: cp, .. }, Curve::Arc { center: cn, .. }) => {
                // Both centers sit on the y-axis through the midpoint.
                assert!((cp.x - 1.0).abs() < 1e-12);
                assert!((cn.x - 1.0).abs() < 1e-12);
                // y signs differ.
                assert!(cp.y.signum() != cn.y.signum() || cp.y == 0.0);
            }
            _ => panic!("expected two arcs"),
        }
    }

    /// A 2D POLYLINE with three vertices, straight between the first
    /// two and a semicircle between the second and third, produces
    /// a [`Path`] with 2 segments: one Line, one Arc.
    #[test]
    fn polyline_2d_vertices_to_path_mixed_bulges() {
        let vertices = [
            crate::entities::vertex::Vertex {
                flag: 0,
                location: Point3D::new(0.0, 0.0, 0.0),
                start_width: 0.0,
                end_width: 0.0,
                bulge: 0.0,
                vertex_id: None,
                tangent_direction: None,
            },
            crate::entities::vertex::Vertex {
                flag: 0,
                location: Point3D::new(1.0, 0.0, 0.0),
                start_width: 0.0,
                end_width: 0.0,
                bulge: 1.0, // semicircle to the next vertex
                vertex_id: None,
                tangent_direction: None,
            },
            crate::entities::vertex::Vertex {
                flag: 0,
                location: Point3D::new(2.0, 0.0, 0.0),
                start_width: 0.0,
                end_width: 0.0,
                bulge: 0.0,
                vertex_id: None,
                tangent_direction: None,
            },
        ];
        let path = polyline_2d_vertices_to_path(&vertices, 7.5, false);
        assert_eq!(path.segments.len(), 2);
        assert!(!path.closed);
        assert!(matches!(&path.segments[0], Curve::Line { .. }));
        assert!(matches!(&path.segments[1], Curve::Arc { .. }));

        // Elevation lifted every point to Z = 7.5.
        if let Curve::Line { a, b } = &path.segments[0] {
            assert!((a.z - 7.5).abs() < 1e-12);
            assert!((b.z - 7.5).abs() < 1e-12);
        }
    }

    /// Closed polyline adds a final edge wrapping back to the first
    /// vertex, using the LAST vertex's bulge for that closing edge.
    #[test]
    fn polyline_2d_vertices_closed_wraps_to_start() {
        let vertices = [
            crate::entities::vertex::Vertex {
                flag: 0,
                location: Point3D::new(0.0, 0.0, 0.0),
                start_width: 0.0,
                end_width: 0.0,
                bulge: 0.0,
                vertex_id: None,
                tangent_direction: None,
            },
            crate::entities::vertex::Vertex {
                flag: 0,
                location: Point3D::new(1.0, 0.0, 0.0),
                start_width: 0.0,
                end_width: 0.0,
                bulge: 0.0,
                vertex_id: None,
                tangent_direction: None,
            },
            crate::entities::vertex::Vertex {
                flag: 0,
                location: Point3D::new(1.0, 1.0, 0.0),
                start_width: 0.0,
                end_width: 0.0,
                bulge: 0.0,
                vertex_id: None,
                tangent_direction: None,
            },
        ];
        let path = polyline_2d_vertices_to_path(&vertices, 0.0, true);
        assert_eq!(path.segments.len(), 3);
        assert!(path.closed);
        // Last segment connects (1,1) back to (0,0).
        match &path.segments[2] {
            Curve::Line { a, b } => {
                assert_eq!(a.x, 1.0);
                assert_eq!(a.y, 1.0);
                assert_eq!(b.x, 0.0);
                assert_eq!(b.y, 0.0);
            }
            other => panic!("expected closing Line, got {other:?}"),
        }
    }

    /// 3D polyline preserves the full Z of each vertex (no elevation
    /// override).
    #[test]
    fn polyline_3d_vertices_preserves_z() {
        let vertices = [
            crate::entities::vertex::Vertex {
                flag: 0,
                location: Point3D::new(0.0, 0.0, -1.0),
                start_width: 0.0,
                end_width: 0.0,
                bulge: 0.0,
                vertex_id: None,
                tangent_direction: None,
            },
            crate::entities::vertex::Vertex {
                flag: 0,
                location: Point3D::new(10.0, 0.0, 5.0),
                start_width: 0.0,
                end_width: 0.0,
                bulge: 0.0,
                vertex_id: None,
                tangent_direction: None,
            },
        ];
        let path = polyline_3d_vertices_to_path(&vertices, false);
        assert_eq!(path.segments.len(), 1);
        if let Curve::Line { a, b } = &path.segments[0] {
            assert_eq!(a.z, -1.0);
            assert_eq!(b.z, 5.0);
        } else {
            panic!("expected 3D Line");
        }
    }

    /// `polyline_to_path` dispatches to 2D or 3D helper based on
    /// `polyline.is_3d()`.
    #[test]
    fn polyline_to_path_dispatches_on_is_3d() {
        let vs = [crate::entities::vertex::Vertex {
            flag: 0,
            location: Point3D::new(1.0, 2.0, 3.0),
            start_width: 0.0,
            end_width: 0.0,
            bulge: 0.0,
            vertex_id: None,
            tangent_direction: None,
        }];
        let hdr_3d = crate::entities::polyline::Polyline {
            flag: 0x08, // 3D bit set
            curve_type: 0,
            default_start_width: 0.0,
            default_end_width: 0.0,
            thickness: 0.0,
            elevation: 999.0, // should be ignored
            extrusion: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        };
        let path = polyline_to_path(&hdr_3d, &vs);
        // Only one vertex → no segments.
        assert!(path.segments.is_empty());

        // 2D variant with elevation applied.
        let hdr_2d = crate::entities::polyline::Polyline {
            flag: 0x00,
            elevation: 42.0,
            ..hdr_3d
        };
        let two = [
            vs[0].clone(),
            crate::entities::vertex::Vertex {
                flag: 0,
                location: Point3D::new(5.0, 6.0, 7.0),
                start_width: 0.0,
                end_width: 0.0,
                bulge: 0.0,
                vertex_id: None,
                tangent_direction: None,
            },
        ];
        let path2 = polyline_to_path(&hdr_2d, &two);
        assert_eq!(path2.segments.len(), 1);
        if let Curve::Line { a, b } = &path2.segments[0] {
            // Z overridden by header elevation.
            assert!((a.z - 42.0).abs() < 1e-12);
            assert!((b.z - 42.0).abs() < 1e-12);
        }
    }

    // ------------------------------------------------------------
    // L8-15 — HATCH → Paths (stub)
    // ------------------------------------------------------------

    /// Until `hatch_to_paths` walks the boundary-path tree and emits
    /// geometry, a fully-populated HATCH still returns an empty path
    /// list. Test pins the contract so a future implementer updates this
    /// assertion when path geometry starts being produced.
    #[test]
    fn hatch_to_paths_is_empty_until_boundary_decoder_lands() {
        let h = crate::entities::hatch::Hatch {
            gradient: None,
            extrusion: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            elevation: 0.0,
            pattern_name: "SOLID".into(),
            solid_fill: true,
            associative: false,
            paths: Vec::new(),
            pattern_style: 0,
            pattern_angle: 0.0,
            pattern_scale: 1.0,
            pattern_double: false,
            pattern_lines: Vec::new(),
            pixel_size: 0,
            seed_points: Vec::new(),
        };
        let paths = hatch_to_paths(&h);
        assert!(paths.is_empty());
    }

    // ------------------------------------------------------------
    // L8-16 — TEXT → baseline line
    // ------------------------------------------------------------

    /// A TEXT with rotation 0, height 2.5, insertion (10,20), elevation
    /// 3 should emit a baseline from (10,20,3) to (12.5,20,3).
    #[test]
    fn text_to_curve_emits_axis_aligned_baseline() {
        let t = crate::entities::text::Text {
            elevation: 3.0,
            insertion_point: crate::entities::Point2D { x: 10.0, y: 20.0 },
            alignment_point: None,
            extrusion: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            thickness: 0.0,
            oblique_angle: 0.0,
            rotation_angle: 0.0,
            height: 2.5,
            width_factor: 1.0,
            text: "HELLO".into(),
            generation: 0,
            h_align: 0,
            v_align: 0,
        };
        match text_to_curve(&t) {
            Curve::Line { a, b } => {
                assert_eq!(a, Point3D::new(10.0, 20.0, 3.0));
                assert_eq!(b, Point3D::new(12.5, 20.0, 3.0));
            }
            other => panic!("expected Line, got {other:?}"),
        }
    }

    /// A 90° rotation swings the baseline onto the +Y axis.
    #[test]
    fn text_to_curve_respects_rotation() {
        let t = crate::entities::text::Text {
            elevation: 0.0,
            insertion_point: crate::entities::Point2D { x: 0.0, y: 0.0 },
            alignment_point: None,
            extrusion: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            thickness: 0.0,
            oblique_angle: 0.0,
            rotation_angle: std::f64::consts::FRAC_PI_2,
            height: 1.0,
            width_factor: 1.0,
            text: String::new(),
            generation: 0,
            h_align: 0,
            v_align: 0,
        };
        match text_to_curve(&t) {
            Curve::Line { a, b } => {
                assert!((a.x - 0.0).abs() < 1e-12);
                assert!((a.y - 0.0).abs() < 1e-12);
                assert!((b.x - 0.0).abs() < 1e-12, "b.x = {}", b.x);
                assert!((b.y - 1.0).abs() < 1e-12, "b.y = {}", b.y);
            }
            other => panic!("expected Line, got {other:?}"),
        }
    }

    // ------------------------------------------------------------
    // L8-17 — DIMENSION → skeletal paths
    // ------------------------------------------------------------

    /// Build a minimal `DimensionCommon` for tests.
    fn stub_common() -> crate::entities::dimension::DimensionCommon {
        crate::entities::dimension::DimensionCommon {
            version_flag: 0,
            extrusion: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            text_midpoint: crate::entities::Point2D { x: 0.0, y: 0.0 },
            elevation: 0.0,
            flags: 0,
            user_text: String::new(),
            text_rotation: 0.0,
            horiz_dir: 0.0,
            ins_scale: Point3D::new(1.0, 1.0, 1.0),
            ins_rotation: 0.0,
            attachment: 0,
            line_spacing_style: 0,
            line_spacing_factor: 1.0,
            actual_measurement: 0.0,
            def_point_12: crate::entities::Point2D { x: 0.0, y: 0.0 },
            flip_arrow_1: false,
            flip_arrow_2: false,
        }
    }

    /// Linear dimension emits three paths: dim-line + two extension
    /// lines. Each path is a single straight segment.
    #[test]
    fn dimension_to_paths_linear_emits_three_lines() {
        let d = crate::entities::dimension::Dimension::Linear(
            crate::entities::dimension::LinearDimension {
                common: stub_common(),
                def_point_13: Point3D::new(0.0, 0.0, 0.0),
                def_point_14: Point3D::new(10.0, 0.0, 0.0),
                def_point_10: Point3D::new(5.0, 5.0, 0.0),
                extension_line_rotation: 0.0,
                dim_rotation: 0.0,
            },
        );
        let paths = dimension_to_paths(&d);
        assert_eq!(paths.len(), 3);
        for p in &paths {
            assert_eq!(p.segments.len(), 1);
            assert!(matches!(&p.segments[0], Curve::Line { .. }));
        }
    }

    /// Radius dimension emits a single dim-line from center to chord.
    #[test]
    fn dimension_to_paths_radius_emits_one_line() {
        let d = crate::entities::dimension::Dimension::Radius(
            crate::entities::dimension::RadiusDimension {
                common: stub_common(),
                def_point_10: Point3D::new(0.0, 0.0, 0.0),
                def_point_15: Point3D::new(5.0, 0.0, 0.0),
                leader_length: 2.5,
            },
        );
        let paths = dimension_to_paths(&d);
        assert_eq!(paths.len(), 1);
        match &paths[0].segments[0] {
            Curve::Line { a, b } => {
                assert_eq!(*a, Point3D::new(0.0, 0.0, 0.0));
                assert_eq!(*b, Point3D::new(5.0, 0.0, 0.0));
            }
            other => panic!("expected Line, got {other:?}"),
        }
    }

    /// Angular3Pt emits three paths (two rays + chord).
    #[test]
    fn dimension_to_paths_angular3pt_emits_three_paths() {
        let d = crate::entities::dimension::Dimension::Angular3Pt(
            crate::entities::dimension::Angular3PtDimension {
                common: stub_common(),
                def_point_10: Point3D::new(0.0, 0.0, 0.0),
                def_point_13: Point3D::new(1.0, 0.0, 0.0),
                def_point_14: Point3D::new(0.0, 1.0, 0.0),
                def_point_15: Point3D::new(0.0, 0.0, 0.0),
            },
        );
        let paths = dimension_to_paths(&d);
        assert_eq!(paths.len(), 3);
    }

    /// Ordinate emits a single leader path.
    #[test]
    fn dimension_to_paths_ordinate_emits_one_leader() {
        let d = crate::entities::dimension::Dimension::Ordinate(
            crate::entities::dimension::OrdinateDimension {
                common: stub_common(),
                def_point_10: Point3D::new(0.0, 0.0, 0.0),
                feature_location_13: Point3D::new(3.0, 0.0, 0.0),
                leader_endpoint_14: Point3D::new(5.0, 5.0, 0.0),
                flag_2: 0,
            },
        );
        let paths = dimension_to_paths(&d);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].segments.len(), 1);
    }
}
