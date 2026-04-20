//! MESH entity (§19.4.66) — subdivision (SubD) surface mesh.
//!
//! Introduced in R2010 as a custom class (`ACDB_MESH_OBJECT` /
//! `ACDB_SUBDMESH`), MESH stores a subdivision-surface control cage plus
//! crease metadata. Unlike [`crate::entities::polyface_mesh::PolyfaceMesh`]
//! or [`crate::entities::polygon_mesh::PolygonMesh`] — both of which keep
//! vertices in a separate handle chain — MESH packs all geometry inline.
//!
//! Pre-R2010 files never emit this entity; older versions are treated
//! as [`crate::error::Error::Unsupported`].
//!
//! # Stream shape (R2010+, L4-34)
//!
//! ```text
//! BS   version             -- 0 or 1 (encoding variant)
//! B    blend_crease        -- whether creases are blended at corners
//! BS   subdivision_level   -- smoothing steps applied at render time
//! BS   vertex_count        -- control-cage vertices
//! BD3  × vertex_count      -- vertex positions (world coords)
//! BL   face_count          -- faces in the control cage
//! (per face)
//!   BL face_vertex_count   -- 3 for tri, 4 for quad, >4 for n-gon
//!   BL × face_vertex_count -- vertex indices into the positions array
//! BL   edge_count
//! (per edge)
//!   BS start_index         -- vertex index
//!   BS end_index           -- vertex index
//! BD   × edge_count        -- crease values (0.0 = smooth)
//! ```
//!
//! The cross-field-count stream (a single `BL face_count` followed by
//! per-face counts) reuses the defensive pattern from
//! [`crate::entities::lwpolyline::decode`]: every claimed count is capped against both a
//! hard ceiling and [`BitCursor::remaining_bits`].
//!
//! # Version gating
//!
//! Only R2010+ is supported. Earlier versions surface
//! [`crate::error::Error::Unsupported`] without attempting a best-effort decode —
//! guessing the stream shape for a file format that never contained
//! this entity would produce misaligned output downstream.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

// ========================================================================
// Defensive caps — ODA §19.4.66 "practical limits" guidance cross-checked
// against observed vertex / face / edge counts in real subdivision meshes.
// ========================================================================
const CAP_VERTICES: usize = 1_000_000;
const CAP_FACES: usize = 1_000_000;
const CAP_FACE_VERTICES: usize = 64;
const CAP_EDGES: usize = 4_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    /// Encoding version per ODA (observed: 0 or 1). Retained verbatim so
    /// round-trip writers can re-emit the same form.
    pub version: u16,
    /// Whether creases are blended across corners.
    pub blend_crease: bool,
    /// Subdivision depth applied at render time (not stored geometry).
    pub subdivision_level: u16,
    /// Control-cage vertex positions.
    pub vertices: Vec<Point3D>,
    /// One entry per face; each entry is a list of vertex indices.
    pub faces: Vec<Vec<u32>>,
    /// Edge endpoints as `(start_vertex_index, end_vertex_index)` pairs.
    pub edges: Vec<(u16, u16)>,
    /// Crease value per edge — 0.0 = smooth, >0.0 = sharpened at that
    /// subdivision step count.
    pub creases: Vec<f64>,
}

fn bounds_check(n: usize, field: &'static str, cap: usize, remaining_bits: usize) -> Result<()> {
    if n > cap || n > remaining_bits {
        Err(Error::SectionMap(format!(
            "MESH {field} count {n} exceeds cap ({cap}) or remaining_bits ({remaining_bits})"
        )))
    } else {
        Ok(())
    }
}

/// Decode a MESH entity's type-specific payload.
///
/// # Errors
///
/// * [`Error::Unsupported`] — pre-R2010 version.
/// * [`Error::SectionMap`] — any count exceeds the cap or the remaining
///   payload bit budget.
/// * Propagated cursor errors when primitives run out of bits.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Mesh> {
    if !version.is_r2010_plus() {
        return Err(Error::Unsupported {
            feature: format!("MESH (subdivision) on pre-R2010 file ({version:?})"),
        });
    }

    let ver = c.read_bs_u()?;
    let blend_crease = c.read_b()?;
    let subdivision_level = c.read_bs_u()?;

    let num_vertices = c.read_bs_u()? as usize;
    bounds_check(
        num_vertices,
        "vertex_count",
        CAP_VERTICES,
        c.remaining_bits(),
    )?;
    let mut vertices = Vec::with_capacity(num_vertices);
    for _ in 0..num_vertices {
        vertices.push(read_bd3(c)?);
    }

    let num_faces = c.read_bl_u()? as usize;
    bounds_check(num_faces, "face_count", CAP_FACES, c.remaining_bits())?;
    let mut faces = Vec::with_capacity(num_faces);
    for _ in 0..num_faces {
        let fvc = c.read_bl_u()? as usize;
        bounds_check(
            fvc,
            "face_vertex_count",
            CAP_FACE_VERTICES,
            c.remaining_bits(),
        )?;
        let mut vs = Vec::with_capacity(fvc);
        for _ in 0..fvc {
            vs.push(c.read_bl_u()?);
        }
        faces.push(vs);
    }

    let num_edges = c.read_bl_u()? as usize;
    bounds_check(num_edges, "edge_count", CAP_EDGES, c.remaining_bits())?;
    let mut edges = Vec::with_capacity(num_edges);
    for _ in 0..num_edges {
        let a = c.read_bs_u()?;
        let b = c.read_bs_u()?;
        edges.push((a, b));
    }

    // One crease per edge. ODA §19.4.66 states the crease array's length
    // equals edge_count; enforcing that here keeps mis-sized streams
    // from silently succeeding.
    let mut creases = Vec::with_capacity(num_edges);
    for _ in 0..num_edges {
        creases.push(c.read_bd()?);
    }

    Ok(Mesh {
        version: ver,
        blend_crease,
        subdivision_level,
        vertices,
        faces,
        edges,
        creases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Scalar fields shared by every MESH test fixture. Kept separate
    /// from the list-valued fields so `write_mesh` stays under the
    /// 7-arg clippy ceiling without deeply nesting the test payload
    /// definitions.
    struct MeshHeader {
        version: u16,
        blend_crease: bool,
        subdivision_level: u16,
    }

    /// Helper — write a minimal MESH payload for the R2010+ shape.
    fn write_mesh(
        w: &mut BitWriter,
        hdr: &MeshHeader,
        vertices: &[Point3D],
        faces: &[&[u32]],
        edges: &[(u16, u16)],
        creases: &[f64],
    ) {
        w.write_bs_u(hdr.version);
        w.write_b(hdr.blend_crease);
        w.write_bs_u(hdr.subdivision_level);
        w.write_bs_u(vertices.len() as u16);
        for v in vertices {
            w.write_bd(v.x);
            w.write_bd(v.y);
            w.write_bd(v.z);
        }
        w.write_bl(faces.len() as i32);
        for f in faces {
            w.write_bl(f.len() as i32);
            for &i in *f {
                w.write_bl(i as i32);
            }
        }
        w.write_bl(edges.len() as i32);
        for &(a, b) in edges {
            w.write_bs_u(a);
            w.write_bs_u(b);
        }
        for &c in creases {
            w.write_bd(c);
        }
    }

    #[test]
    fn roundtrip_minimal_cube_cage() {
        let mut w = BitWriter::new();
        // A 2-face cage sharing one edge — enough to cover every count
        // path without drowning the test in coordinates.
        let verts = [
            Point3D {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            Point3D {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Point3D {
                x: 1.0,
                y: 1.0,
                z: 0.0,
            },
            Point3D {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
        ];
        let face_a: &[u32] = &[0, 1, 2];
        let face_b: &[u32] = &[0, 2, 3];
        let edges = [(0u16, 1u16), (1, 2), (2, 0), (2, 3), (3, 0)];
        let creases = [0.0f64, 0.0, 1.0, 0.0, 0.0];
        write_mesh(
            &mut w,
            &MeshHeader {
                version: 0,
                blend_crease: false,
                subdivision_level: 2,
            },
            &verts,
            &[face_a, face_b],
            &edges,
            &creases,
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c, Version::R2010).unwrap();
        assert_eq!(m.version, 0);
        assert!(!m.blend_crease);
        assert_eq!(m.subdivision_level, 2);
        assert_eq!(m.vertices.len(), 4);
        assert_eq!(
            m.vertices[1],
            Point3D {
                x: 1.0,
                y: 0.0,
                z: 0.0
            }
        );
        assert_eq!(m.faces.len(), 2);
        assert_eq!(m.faces[0], vec![0, 1, 2]);
        assert_eq!(m.faces[1], vec![0, 2, 3]);
        assert_eq!(m.edges.len(), 5);
        assert_eq!(m.edges[2], (2, 0));
        assert_eq!(m.creases, vec![0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn roundtrip_blend_crease_true_higher_level() {
        let mut w = BitWriter::new();
        let verts = [
            Point3D {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            Point3D {
                x: 2.0,
                y: 0.0,
                z: 0.0,
            },
            Point3D {
                x: 1.0,
                y: 2.0,
                z: 0.0,
            },
        ];
        let face: &[u32] = &[0, 1, 2];
        let edges = [(0u16, 1u16), (1, 2), (2, 0)];
        let creases = [0.5f64, 0.0, 0.25];
        write_mesh(
            &mut w,
            &MeshHeader {
                version: 1,
                blend_crease: true,
                subdivision_level: 4,
            },
            &verts,
            &[face],
            &edges,
            &creases,
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c, Version::R2013).unwrap();
        assert_eq!(m.version, 1);
        assert!(m.blend_crease);
        assert_eq!(m.subdivision_level, 4);
        assert_eq!(m.faces[0].len(), 3);
        assert_eq!(m.creases[0], 0.5);
        assert_eq!(m.creases[2], 0.25);
    }

    #[test]
    fn rejects_pre_r2010() {
        let mut w = BitWriter::new();
        write_mesh(
            &mut w,
            &MeshHeader {
                version: 0,
                blend_crease: false,
                subdivision_level: 1,
            },
            &[],
            &[],
            &[],
            &[],
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2007).unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }), "err={err:?}");
    }

    #[test]
    fn rejects_oversized_face_count() {
        let mut w = BitWriter::new();
        // Version + blend + subdivision_level + vertex_count(0)
        w.write_bs_u(0);
        w.write_b(false);
        w.write_bs_u(0);
        w.write_bs_u(0);
        // face_count far beyond CAP_FACES
        w.write_bl(2_000_000);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2010).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("face_count")),
            "err={err:?}"
        );
    }
}
