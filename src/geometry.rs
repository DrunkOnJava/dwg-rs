//! Geometry primitives for the rendering pipeline.
//!
//! This module is decoder-independent. It takes decoded entity data
//! (`Point3D`, `Vec3D` from [`crate::entities`]) and provides the
//! vector math, affine transforms, and bounding-volume operations
//! needed by the SVG/glTF export layers (Phase 9 / Phase 10 of the
//! roadmap).
//!
//! # Design
//!
//! - **Pure math, no allocations.** Every operation here is `Copy`.
//! - **`f64` throughout.** DWG measurements are stored as doubles;
//!   single precision loses bits on large drawings (city-scale CAD
//!   can legitimately have coordinates in the millions with millimetre
//!   precision).
//! - **Right-handed, Z-up.** Matches AutoCAD's World Coordinate System.
//!
//! # Scope
//!
//! Covers the ~10 operations downstream renderers actually need:
//!
//! - Vector arithmetic (add / sub / scale / dot / cross / normalize / length)
//! - 4×4 affine transforms (compose, invert, transform point/vector)
//! - Axis-aligned bounding boxes (union / contains / intersect / empty)
//! - Linear interpolation for tessellation
//!
//! NURBS evaluation, curve subdivision, surface tessellation, and the
//! "Arbitrary Axis" algorithm are in separate modules (Phase 8 tasks
//! L8-02, L8-14, L8-15 track those).

use crate::entities::{Point2D, Point3D, Vec3D};

// ---------------------------------------------------------------------------
// Vector math on Point2D / Point3D / Vec3D.
// ---------------------------------------------------------------------------

impl Point2D {
    /// Construct a new 2D point.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Vector from `self` to `other`.
    pub fn to(&self, other: Point2D) -> Point2D {
        Point2D {
            x: other.x - self.x,
            y: other.y - self.y,
        }
    }

    /// Euclidean distance to `other`.
    pub fn distance(&self, other: Point2D) -> f64 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Linear interpolation: `t == 0` → `self`, `t == 1` → `other`.
    pub fn lerp(&self, other: Point2D, t: f64) -> Point2D {
        Point2D {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }
}

impl Point3D {
    /// Construct a new 3D point.
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Component-wise add.
    pub fn add(&self, other: Vec3D) -> Point3D {
        Point3D {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }

    /// Component-wise sub.
    pub fn sub(&self, other: Point3D) -> Vec3D {
        Vec3D {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }

    /// Euclidean distance to `other`.
    pub fn distance(&self, other: Point3D) -> f64 {
        let d = self.sub(other);
        (d.x * d.x + d.y * d.y + d.z * d.z).sqrt()
    }

    /// Linear interpolation between self and other at parameter `t`.
    pub fn lerp(&self, other: Point3D, t: f64) -> Point3D {
        Point3D {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
            z: self.z + (other.z - self.z) * t,
        }
    }
}

/// Vector-math helpers on the Vec3D alias. Defined as free functions
/// because Vec3D = Point3D (type alias) and Rust disallows duplicate
/// inherent impls on the aliased type — users get these via
/// [`VecOps`] in scope.
pub trait VecOps: Copy {
    /// Scalar multiply.
    fn scale(self, k: f64) -> Self;
    /// Dot product.
    fn dot(self, other: Self) -> f64;
    /// Cross product (3D).
    fn cross(self, other: Self) -> Self;
    /// Length (magnitude).
    fn length(self) -> f64;
    /// Unit vector. Returns the zero vector if length is < `epsilon`.
    fn normalize(self, epsilon: f64) -> Self;
}

impl VecOps for Vec3D {
    fn scale(self, k: f64) -> Self {
        Vec3D {
            x: self.x * k,
            y: self.y * k,
            z: self.z * k,
        }
    }

    fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn cross(self, other: Self) -> Self {
        Vec3D {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    fn normalize(self, epsilon: f64) -> Self {
        let len = self.length();
        if len < epsilon {
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        } else {
            self.scale(1.0 / len)
        }
    }
}

// ---------------------------------------------------------------------------
// 4×4 affine transform (row-major, right-handed).
// ---------------------------------------------------------------------------

/// 4×4 affine transform matrix stored in row-major order.
///
/// The convention is `point_world = m * point_local` (column-vector
/// multiplication), which means:
///
/// - `m[0..3][3]` is the translation column.
/// - `m[3][..]` is always `[0, 0, 0, 1]` for affine transforms — we
///   don't construct projective transforms through this type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform3 {
    pub m: [[f64; 4]; 4],
}

impl Default for Transform3 {
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform3 {
    /// Identity transform.
    pub const fn identity() -> Self {
        Transform3 {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Translation transform.
    pub const fn translation(x: f64, y: f64, z: f64) -> Self {
        Transform3 {
            m: [
                [1.0, 0.0, 0.0, x],
                [0.0, 1.0, 0.0, y],
                [0.0, 0.0, 1.0, z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Uniform scale.
    pub const fn scale_uniform(k: f64) -> Self {
        Transform3 {
            m: [
                [k, 0.0, 0.0, 0.0],
                [0.0, k, 0.0, 0.0],
                [0.0, 0.0, k, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Non-uniform scale.
    pub const fn scale(sx: f64, sy: f64, sz: f64) -> Self {
        Transform3 {
            m: [
                [sx, 0.0, 0.0, 0.0],
                [0.0, sy, 0.0, 0.0],
                [0.0, 0.0, sz, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Rotation around the Z axis by `radians`. Right-hand rule.
    pub fn rotation_z(radians: f64) -> Self {
        let (s, c) = radians.sin_cos();
        Transform3 {
            m: [
                [c, -s, 0.0, 0.0],
                [s, c, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Matrix-multiply (compose): `self * other` applied as a single
    /// transform means "first `other`, then `self`" to a point.
    pub fn compose(&self, other: &Transform3) -> Transform3 {
        let mut out = Transform3 { m: [[0.0; 4]; 4] };
        for i in 0..4 {
            for j in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += self.m[i][k] * other.m[k][j];
                }
                out.m[i][j] = sum;
            }
        }
        out
    }

    /// Transform a point (applies translation + linear part).
    pub fn transform_point(&self, p: Point3D) -> Point3D {
        let x = self.m[0][0] * p.x + self.m[0][1] * p.y + self.m[0][2] * p.z + self.m[0][3];
        let y = self.m[1][0] * p.x + self.m[1][1] * p.y + self.m[1][2] * p.z + self.m[1][3];
        let z = self.m[2][0] * p.x + self.m[2][1] * p.y + self.m[2][2] * p.z + self.m[2][3];
        Point3D { x, y, z }
    }

    /// Transform a vector (linear part only, ignores translation).
    pub fn transform_vector(&self, v: Vec3D) -> Vec3D {
        Vec3D {
            x: self.m[0][0] * v.x + self.m[0][1] * v.y + self.m[0][2] * v.z,
            y: self.m[1][0] * v.x + self.m[1][1] * v.y + self.m[1][2] * v.z,
            z: self.m[2][0] * v.x + self.m[2][1] * v.y + self.m[2][2] * v.z,
        }
    }

    /// Build a transform from World Coordinate System (WCS) to a User
    /// Coordinate System (UCS) defined by an origin + X axis + Y axis.
    /// (L8-01.) The Z axis is computed as `x_axis × y_axis`.
    ///
    /// Use this when an entity stores its coordinates in UCS-local
    /// space and you need to lift them to WCS for rendering: take the
    /// inverse of this transform via `invert_orthonormal` and apply
    /// to entity coordinates.
    pub fn ucs_from_axes(origin: Point3D, x_axis: Vec3D, y_axis: Vec3D) -> Self {
        let x = x_axis.normalize(1e-12);
        let y = y_axis.normalize(1e-12);
        let z = x.cross(y).normalize(1e-12);
        Transform3 {
            m: [
                [x.x, y.x, z.x, origin.x],
                [x.y, y.y, z.y, origin.y],
                [x.z, y.z, z.z, origin.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Invert an orthonormal transform (rotation + translation only —
    /// no scale or shear). Cheap closed-form inverse: transpose the
    /// 3×3 rotation block and negate the translation. UCS↔WCS pairs
    /// are always orthonormal, so this is the right inverse for them.
    pub fn invert_orthonormal(&self) -> Transform3 {
        let mut out = Transform3 { m: [[0.0; 4]; 4] };
        for i in 0..3 {
            for j in 0..3 {
                out.m[i][j] = self.m[j][i];
            }
        }
        // New translation = -Rᵀ · old translation.
        let tx = self.m[0][3];
        let ty = self.m[1][3];
        let tz = self.m[2][3];
        out.m[0][3] = -(out.m[0][0] * tx + out.m[0][1] * ty + out.m[0][2] * tz);
        out.m[1][3] = -(out.m[1][0] * tx + out.m[1][1] * ty + out.m[1][2] * tz);
        out.m[2][3] = -(out.m[2][0] * tx + out.m[2][1] * ty + out.m[2][2] * tz);
        out.m[3][3] = 1.0;
        out
    }

    /// AutoCAD's "Arbitrary Axis Algorithm" — derives a stable UCS basis
    /// from an extrusion-direction normal vector. (L8-02.) Per the
    /// publicly-documented algorithm in the ODA spec appendix and
    /// Autodesk DXF reference: when |Nx| < 1/64 AND |Ny| < 1/64, take
    /// the world Y axis as Wy; otherwise take the world Z axis as Wz.
    /// Then build:
    ///
    ///   Ax = Wy × Nz_normal       (or Wz × Nz_normal)
    ///   Ay = Nz_normal × Ax
    ///
    /// The returned transform takes UCS-local coordinates to WCS
    /// for an entity whose extrusion is `normal`.
    pub fn arbitrary_axis(normal: Vec3D) -> Self {
        let n = normal.normalize(1e-12);
        let world_y = Vec3D {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        let world_z = Vec3D {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        let pivot = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 {
            world_y
        } else {
            world_z
        };
        let ax = pivot.cross(n).normalize(1e-12);
        let ay = n.cross(ax);
        Transform3::ucs_from_axes(Point3D::new(0.0, 0.0, 0.0), ax, ay)
    }

    /// Compose this transform with `other`, treating `self` as an
    /// entity-level instance transform applied AFTER `other` (which
    /// is typically the parent block's transform). Equivalent to
    /// [`compose`](Self::compose).
    ///
    /// The named alias exists for L8-03 readability — at INSERT
    /// expansion time, callers write
    /// `parent_transform.then(insert_transform)` to express
    /// "the INSERT is positioned within the block".
    pub fn then(&self, other: &Transform3) -> Transform3 {
        other.compose(self)
    }

    /// Build a single composite transform from a chain of nested
    /// block-INSERT transforms, with the outermost block first and
    /// the innermost INSERT last. (L8-04.) Useful for flattening a
    /// nested-block hierarchy at render time without walking the
    /// entity tree per-vertex.
    ///
    /// `chain.iter().rev().fold(identity, |acc, t| acc.compose(t))`
    /// — but the public API hides the fold direction so callers don't
    /// have to reason about whether to reverse.
    pub fn compose_chain(chain: &[Transform3]) -> Transform3 {
        let mut acc = Transform3::identity();
        for t in chain {
            acc = acc.compose(t);
        }
        acc
    }
}

// ---------------------------------------------------------------------------
// Axis-aligned bounding box (3D).
// ---------------------------------------------------------------------------

/// Axis-aligned bounding box in WCS.
///
/// An "empty" bbox is represented by `min` > `max` on every axis — this
/// lets [`BBox3::union`] work correctly starting from an empty bbox
/// without needing an Option wrapper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox3 {
    pub min: Point3D,
    pub max: Point3D,
}

impl BBox3 {
    /// An empty bbox that acts as the identity element for [`Self::union`].
    pub const fn empty() -> Self {
        BBox3 {
            min: Point3D {
                x: f64::INFINITY,
                y: f64::INFINITY,
                z: f64::INFINITY,
            },
            max: Point3D {
                x: f64::NEG_INFINITY,
                y: f64::NEG_INFINITY,
                z: f64::NEG_INFINITY,
            },
        }
    }

    /// A bbox containing a single point.
    pub const fn point(p: Point3D) -> Self {
        BBox3 { min: p, max: p }
    }

    /// `true` if this bbox contains no points (min > max on any axis).
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    /// Bbox that contains both `self` and `other`.
    pub fn union(&self, other: &BBox3) -> BBox3 {
        BBox3 {
            min: Point3D {
                x: self.min.x.min(other.min.x),
                y: self.min.y.min(other.min.y),
                z: self.min.z.min(other.min.z),
            },
            max: Point3D {
                x: self.max.x.max(other.max.x),
                y: self.max.y.max(other.max.y),
                z: self.max.z.max(other.max.z),
            },
        }
    }

    /// Expand to include a point.
    pub fn expand(&self, p: Point3D) -> BBox3 {
        self.union(&BBox3::point(p))
    }

    /// `true` if `p` lies inside or on the bbox boundary.
    pub fn contains(&self, p: Point3D) -> bool {
        !self.is_empty()
            && p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }

    /// Width / height / depth vector. Returns the zero vector for
    /// empty bboxes.
    pub fn size(&self) -> Vec3D {
        if self.is_empty() {
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        } else {
            self.max.sub(self.min)
        }
    }

    /// Center point. Undefined for empty bboxes (caller should check).
    pub fn center(&self) -> Point3D {
        Point3D {
            x: (self.min.x + self.max.x) * 0.5,
            y: (self.min.y + self.max.y) * 0.5,
            z: (self.min.z + self.max.z) * 0.5,
        }
    }
}

// ---------------------------------------------------------------------------
// Triangle / quad mesh (for 3D face entities).
// ---------------------------------------------------------------------------

/// An indexed mesh: shared vertex list + triangle indices.
///
/// Quads from DWG 3DFACE / polyface-mesh entities are split into two
/// triangles at ingest; everything downstream (glTF, render) consumes
/// triangle-only meshes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// Unique vertex positions.
    pub vertices: Vec<Point3D>,
    /// Three indices per triangle, referring to `vertices`.
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    /// Empty mesh (no vertices, no triangles).
    pub const fn empty() -> Self {
        Mesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        }
    }

    /// Append a triangle; returns the triangle index. Callers are
    /// responsible for deduplicating `vertices` if sharing is desired
    /// (the mesh stores whatever the caller hands it).
    pub fn push_triangle(&mut self, a: Point3D, b: Point3D, c: Point3D) -> usize {
        let i = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&[a, b, c]);
        self.triangles.push([i, i + 1, i + 2]);
        self.triangles.len() - 1
    }

    /// Append a quad as two triangles (ABC + ACD).
    pub fn push_quad(&mut self, a: Point3D, b: Point3D, c: Point3D, d: Point3D) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&[a, b, c, d]);
        self.triangles.push([base, base + 1, base + 2]);
        self.triangles.push([base, base + 2, base + 3]);
    }

    /// Axis-aligned bounding box enclosing every vertex.
    pub fn bounds(&self) -> BBox3 {
        self.vertices
            .iter()
            .fold(BBox3::empty(), |acc, p| acc.expand(*p))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
