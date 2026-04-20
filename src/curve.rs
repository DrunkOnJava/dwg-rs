//! Unified curve + path types for the rendering pipeline.
//!
//! Decoded entities carry their spec-level representation — endpoints
//! with deltas for LINE; center with radius and angles for ARC; and so
//! on. The SVG, glTF, and DXF writers need a single type they can
//! dispatch on rather than a match on `DecodedEntity` for every
//! primitive. This module defines that unified type.
//!
//! ```text
//! DecodedEntity  ──toCurve──▶  Curve  ──render──▶  SVG path / glTF mesh / DXF group
//!                                   └──batch────▶  Path (multi-segment)
//! ```
//!
//! # Scope
//!
//! Only the lowest-common-denominator 2D/3D curve types. NURBS + splines
//! carry full control-point + knot data; `Curve::Spline` wraps a raw
//! data struct rather than flattening to polylines (tessellation is
//! the renderer's concern, not this type's).

use crate::entities::{Point3D, Vec3D};
use crate::geometry::BBox3;

/// A single continuous curve in 3D space. Unifies the geometric
/// primitives across all entity types.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// Straight line segment from `a` to `b`.
    Line { a: Point3D, b: Point3D },
    /// Full circle in a plane defined by `center` + `normal`.
    Circle {
        center: Point3D,
        radius: f64,
        normal: Vec3D,
    },
    /// Arc on the circle at `center` + `normal`, sweeping from
    /// `start_angle` to `end_angle` (radians, CCW).
    Arc {
        center: Point3D,
        radius: f64,
        normal: Vec3D,
        start_angle: f64,
        end_angle: f64,
    },
    /// Ellipse or elliptical arc. `major_axis` carries both the
    /// direction of the major axis and its length. `ratio` is
    /// minor / major. Angles sweep from `start` to `end` in radians;
    /// a full ellipse is `start = 0, end = 2π`.
    Ellipse {
        center: Point3D,
        major_axis: Vec3D,
        normal: Vec3D,
        ratio: f64,
        start_angle: f64,
        end_angle: f64,
    },
    /// Multi-segment polyline. Each pair of consecutive vertices forms
    /// a line segment, or an arc if the matching bulge is non-zero
    /// (bulge = tan(θ/4) where θ is the arc's included angle).
    Polyline {
        vertices: Vec<PolylineVertex>,
        closed: bool,
    },
    /// NURBS curve. The renderer tessellates to line segments at its
    /// preferred tolerance.
    Spline(Spline),
    /// Helix (L4-41). A parametric curve on a cylinder.
    Helix {
        axis_start: Point3D,
        axis_end: Point3D,
        radius: f64,
        turns: f64,
    },
}

/// One vertex of a polyline. `bulge` is the arc bulge factor between
/// this vertex and the NEXT one (`tan(θ/4)`; 0 means straight segment,
/// positive values curve left, negative right).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolylineVertex {
    pub point: Point3D,
    pub bulge: f64,
}

/// NURBS spline. The renderer evaluates the curve at discrete
/// parameter values using the control points + weights + knots.
#[derive(Debug, Clone, PartialEq)]
pub struct Spline {
    /// Degree (1 = polyline, 2 = quadratic, 3 = cubic, …).
    pub degree: u32,
    /// Control points in 3D.
    pub control_points: Vec<Point3D>,
    /// Rational weights (one per control point); if empty, treated
    /// as all 1.0 (non-rational / B-spline).
    pub weights: Vec<f64>,
    /// Knot vector. Length = control_points.len() + degree + 1.
    pub knots: Vec<f64>,
    /// `true` if the curve returns to its first control point.
    pub closed: bool,
}

impl Curve {
    /// Axis-aligned bounding box enclosing this curve.
    ///
    /// Cheap + conservative: uses the curve's control polygon or
    /// arc bounds, not a tight fit. Adequate for viewport culling
    /// and layout; tight bounds would require evaluating the curve.
    pub fn bounds(&self) -> BBox3 {
        match self {
            Curve::Line { a, b } => BBox3::empty().expand(*a).expand(*b),
            Curve::Circle {
                center, radius, ..
            } => {
                // Conservative: cube of side 2*radius around center.
                let r = *radius;
                BBox3 {
                    min: Point3D::new(center.x - r, center.y - r, center.z - r),
                    max: Point3D::new(center.x + r, center.y + r, center.z + r),
                }
            }
            Curve::Arc {
                center, radius, ..
            } => {
                // Same conservative bound as circle — tight arc bounds
                // requires working out which axes the arc crosses.
                let r = *radius;
                BBox3 {
                    min: Point3D::new(center.x - r, center.y - r, center.z - r),
                    max: Point3D::new(center.x + r, center.y + r, center.z + r),
                }
            }
            Curve::Ellipse {
                center, major_axis, ratio, ..
            } => {
                // Major axis length is vec-length; minor = major * ratio.
                let ma = (major_axis.x.powi(2) + major_axis.y.powi(2) + major_axis.z.powi(2)).sqrt();
                let r = ma.max(ma * ratio.abs());
                BBox3 {
                    min: Point3D::new(center.x - r, center.y - r, center.z - r),
                    max: Point3D::new(center.x + r, center.y + r, center.z + r),
                }
            }
            Curve::Polyline { vertices, .. } => {
                vertices
                    .iter()
                    .fold(BBox3::empty(), |acc, v| acc.expand(v.point))
            }
            Curve::Spline(s) => s
                .control_points
                .iter()
                .fold(BBox3::empty(), |acc, p| acc.expand(*p)),
            Curve::Helix {
                axis_start,
                axis_end,
                radius,
                ..
            } => {
                let r = *radius;
                let bb = BBox3::empty().expand(*axis_start).expand(*axis_end);
                BBox3 {
                    min: Point3D::new(bb.min.x - r, bb.min.y - r, bb.min.z - r),
                    max: Point3D::new(bb.max.x + r, bb.max.y + r, bb.max.z + r),
                }
            }
        }
    }
}

/// A sequence of connected curves — the 2D/3D analog of an SVG `<path>`
/// element. Used for HATCH boundary loops, MLINE / LWPOLYLINE bodies,
/// and composite dimensions.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub segments: Vec<Curve>,
    /// `true` if the last segment's endpoint connects back to the
    /// first segment's startpoint.
    pub closed: bool,
}

impl Path {
    /// Empty path — no segments, not closed.
    pub const fn empty() -> Self {
        Path {
            segments: Vec::new(),
            closed: false,
        }
    }

    /// Build a simple polyline path from a sequence of points (no
    /// bulges, straight segments only).
    pub fn from_polyline(points: &[Point3D], closed: bool) -> Self {
        let closing = if closed && points.len() >= 2 {
            Some(Curve::Line {
                a: *points.last().unwrap(),
                b: *points.first().unwrap(),
            })
        } else {
            None
        };
        let segments = points
            .windows(2)
            .map(|w| Curve::Line { a: w[0], b: w[1] })
            .chain(closing)
            .collect();
        Path { segments, closed }
    }

    /// Union of every segment's bounding box.
    pub fn bounds(&self) -> BBox3 {
        self.segments
            .iter()
            .fold(BBox3::empty(), |acc, c| acc.union(&c.bounds()))
    }

    /// `true` if this path has zero segments.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_bounds_covers_both_endpoints() {
        let c = Curve::Line {
            a: Point3D::new(0.0, 0.0, 0.0),
            b: Point3D::new(10.0, -5.0, 3.0),
        };
        let b = c.bounds();
        assert_eq!(b.min, Point3D::new(0.0, -5.0, 0.0));
        assert_eq!(b.max, Point3D::new(10.0, 0.0, 3.0));
    }

    #[test]
    fn circle_bounds_is_cube_around_center() {
        let c = Curve::Circle {
            center: Point3D::new(5.0, 5.0, 5.0),
            radius: 2.0,
            normal: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        };
        let b = c.bounds();
        assert_eq!(b.min, Point3D::new(3.0, 3.0, 3.0));
        assert_eq!(b.max, Point3D::new(7.0, 7.0, 7.0));
    }

    #[test]
    fn polyline_bounds_covers_all_vertices() {
        let c = Curve::Polyline {
            vertices: vec![
                PolylineVertex {
                    point: Point3D::new(-1.0, 0.0, 0.0),
                    bulge: 0.0,
                },
                PolylineVertex {
                    point: Point3D::new(1.0, 2.0, 0.0),
                    bulge: 0.0,
                },
                PolylineVertex {
                    point: Point3D::new(0.0, -1.0, 3.0),
                    bulge: 0.0,
                },
            ],
            closed: false,
        };
        let b = c.bounds();
        assert_eq!(b.min, Point3D::new(-1.0, -1.0, 0.0));
        assert_eq!(b.max, Point3D::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn path_from_polyline_builds_lines() {
        let pts = [
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(1.0, 1.0, 0.0),
        ];
        let p = Path::from_polyline(&pts, false);
        assert_eq!(p.segments.len(), 2);
        assert!(!p.closed);
    }

    #[test]
    fn path_from_polyline_closed_adds_final_segment() {
        let pts = [
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(1.0, 1.0, 0.0),
        ];
        let p = Path::from_polyline(&pts, true);
        assert_eq!(p.segments.len(), 3);
        assert!(p.closed);
    }

    #[test]
    fn empty_path_bounds_is_empty() {
        let p = Path::empty();
        assert!(p.is_empty());
        assert!(p.bounds().is_empty());
    }

    #[test]
    fn path_bounds_unions_segment_bounds() {
        let pts = [
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(10.0, 0.0, 0.0),
            Point3D::new(10.0, 20.0, 0.0),
        ];
        let p = Path::from_polyline(&pts, false);
        let b = p.bounds();
        assert_eq!(b.min, Point3D::new(0.0, 0.0, 0.0));
        assert_eq!(b.max, Point3D::new(10.0, 20.0, 0.0));
    }

    #[test]
    fn spline_bounds_covers_control_points_only() {
        // NB: tight NURBS bounds could exclude control points, but
        // our conservative impl uses the control polygon.
        let s = Spline {
            degree: 3,
            control_points: vec![
                Point3D::new(0.0, 0.0, 0.0),
                Point3D::new(10.0, 0.0, 0.0),
                Point3D::new(10.0, 10.0, 0.0),
                Point3D::new(0.0, 10.0, 0.0),
            ],
            weights: vec![],
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            closed: false,
        };
        let c = Curve::Spline(s);
        let b = c.bounds();
        assert_eq!(b.min, Point3D::new(0.0, 0.0, 0.0));
        assert_eq!(b.max, Point3D::new(10.0, 10.0, 0.0));
    }
}
