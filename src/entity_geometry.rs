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
//! Entities that aren't yet decoded by the entity layer (e.g., HATCH,
//! MULTILEADER) just don't have a conversion function here. Adding
//! those is a follow-up — the conversion API mirrors the entity API
//! 1:1.

use crate::curve::{Curve, Path, PolylineVertex};
use crate::entities::Point3D;
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
            Curve::Circle {
                center, radius, ..
            } => {
                assert_eq!(center, c.center);
                assert_eq!(radius, c.radius);
            }
            _ => panic!("expected Curve::Circle"),
        }
    }
}
