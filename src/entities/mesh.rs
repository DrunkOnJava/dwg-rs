//! MESH entity — subdivision (SubD) surface mesh, the `MESH` custom
//! class (DXF subclass `AcDbSubDMesh`).
//!
//! ODA spec v5.4.1 has no section for MESH; its field list is measured
//! from the corpus. MESH stores a subdivision-surface control cage plus
//! crease metadata. Unlike [`crate::entities::polyface_mesh::PolyfaceMesh`]
//! or [`crate::entities::polygon_mesh::PolygonMesh`] — both of which keep
//! vertices in a separate handle chain — MESH packs all geometry inline.
//!
//! # Stream shape — measured, the same on every release
//!
//! ```text
//! BS   version             -- 2 on every measured record     DXF 71
//! B    blend_crease        -- whether creases are blended    DXF 72
//! BS   subdivision_level   -- smoothing steps at render time DXF 91
//! BS   vertex_count        -- control-cage vertices          DXF 92
//! BD3  × vertex_count      -- vertex positions (world coords) DXF 10
//! BL   face_list_size      -- number of BL entries in the face list,
//!                             NOT the number of faces        DXF 93
//! (repeated until face_list_size entries are consumed)
//!   BL face_vertex_count   -- 3 for tri, 4 for quad, >4 for n-gon
//!   BL × face_vertex_count -- vertex indices into the positions array
//! BL   edge_count                                            DXF 94
//! (per edge)
//!   BL start_index         -- vertex index
//!   BL end_index           -- vertex index
//! BL   crease_count                                          DXF 95
//! BD   × crease_count      -- crease values (0.0 = smooth)   DXF 140
//! BL   trailing count      -- 0 on every measured record     DXF 90
//! ```
//!
//! The DXF column is the group AutoCAD writes for each field, in the
//! same order, in the `AcDbSubDMesh` group of its DXF export.
//!
//! ## What the bytes say
//!
//! `sample_AC1032.dwg` (R2018) holds two MESH records. Read with the
//! face-list count above as a *face* count, the second face-vertex count
//! comes out as 132 / 128 and the parse dies; read as a flat entry
//! count the two records walk their whole face and edge lists:
//!
//! | handle | verts | face-list entries | faces | edges | creases | V − E + F |
//! |--------|-------|-------------------|-------|-------|---------|-----------|
//! | `0x343` | 62   | 336               | 72    | 132   | 132     | 2         |
//! | `0x380` | 64   | 320               | 64    | 128   | 128     | 0         |
//!
//! The Euler characteristics (2 = closed surface, 0 = one-handle torus)
//! are the independent check that the face and edge lists were read at
//! the right offsets, and every face index and edge endpoint is below
//! the vertex count — which [`decode`] enforces.
//!
//! Edge endpoints are `BL`, not `BS`: with `BS` the first pair of the
//! `0x343` mesh reads `(1, 60)` correctly but the list then walks off
//! its own budget.
//!
//! ## The trailing count (2026-10-26, #71)
//!
//! After the last crease both records leave exactly two bits, `10`,
//! before the §19.1 *strings present* flag. `10` is the zero code of a
//! `BS`, a `BL` and a `BD` alike, which is why §7i of ARCHITECTURE.md
//! left it unread and both records erroring at `delta -2`. AutoCAD's DXF
//! export of the same drawing settles it: the `AcDbSubDMesh` group ends
//! `95`, `140` × crease count, then one more `90` whose value is `0`.
//! Group 90 is the group of every other `BL` in the record, so the two
//! bits are a `BL` equal to 0.
//!
//! Nothing in the corpus says what a non-zero value introduces, so
//! [`decode`] returns [`Error::Unsupported`] for one rather than closing
//! on a guess.
//!
//! ## Every release, not only R2010+
//!
//! AutoCAD 2025 writes the same record when it saves the drawing as
//! R14, R2000, R2004 or R2007 — not a proxy. The previous decoder
//! refused every pre-R2010 MESH on the assumption that those releases
//! never hold one. With the trailing `BL` read, the field list above
//! closes at `delta 0` on all 14 MESH records of ACadSharp's seven
//! saves of one drawing, R14 to R2018, and on the 12 that have a DXF
//! twin (R2000 to R2018) every one of the record's values equals the
//! DXF's, group by group: 926 for `0x343` and 904 for `0x380`.
//! `examples/probe_mesh.rs` reproduces it.
//!
//! The `BS` widths of the version, subdivision level and vertex count
//! are not settled: every measured value is below 256, where a `BS` and
//! a `BL` spend the same bits. DXF writes 91 and 92 as 32-bit groups.
//!
//! Every claimed count is capped against both a hard ceiling and
//! [`BitCursor::remaining_bits`], the defensive pattern from
//! [`crate::entities::lwpolyline::decode`].

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

// ========================================================================
// Defensive caps, well above the vertex / face / edge counts of real
// subdivision meshes.
// ========================================================================
const CAP_VERTICES: usize = 1_000_000;
const CAP_FACE_LIST: usize = 8_000_000;
const CAP_FACE_VERTICES: usize = 64;
const CAP_EDGES: usize = 4_000_000;
const CAP_CREASES: usize = 4_000_000;

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
    pub edges: Vec<(u32, u32)>,
    /// Crease value per edge — 0.0 = smooth, >0.0 = sharpened at that
    /// subdivision step count.
    pub creases: Vec<f64>,
}

/// Every face and edge index addresses the vertex array. A parse that
/// has drifted produces indices far past its end, so this is the
/// decoder's cheapest self-check — it fires long before the counts do.
fn check_index(index: u32, num_vertices: usize, what: &'static str) -> Result<u32> {
    if index as usize >= num_vertices {
        return Err(Error::SectionMap(format!(
            "MESH {what} index {index} is outside the {num_vertices}-vertex cage"
        )));
    }
    Ok(index)
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

/// Decode a MESH entity's type-specific payload. The record is the same
/// on every release measured (R14 to R2018), so `version` selects
/// nothing.
///
/// # Errors
///
/// * [`Error::Unsupported`] — a non-zero trailing count, whose contents
///   have never been measured.
/// * [`Error::SectionMap`] — any count exceeds the cap or the remaining
///   payload bit budget.
/// * Propagated cursor errors when primitives run out of bits.
pub fn decode(c: &mut BitCursor<'_>, _version: Version) -> Result<Mesh> {
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

    // Measured: this count is the *length of the face list in `BL`
    // entries*, not a face count. See the module docs.
    let face_list_size = c.read_bl_u()? as usize;
    bounds_check(
        face_list_size,
        "face_list_size",
        CAP_FACE_LIST,
        c.remaining_bits(),
    )?;
    let mut faces: Vec<Vec<u32>> = Vec::new();
    let mut consumed = 0usize;
    while consumed < face_list_size {
        let fvc = c.read_bl_u()? as usize;
        bounds_check(
            fvc,
            "face_vertex_count",
            CAP_FACE_VERTICES,
            c.remaining_bits(),
        )?;
        consumed += 1;
        if consumed + fvc > face_list_size {
            return Err(Error::SectionMap(format!(
                "MESH face of {fvc} vertices overruns the {face_list_size}-entry \
                 face list ({consumed} entries already read)"
            )));
        }
        let mut vs = Vec::with_capacity(fvc);
        for _ in 0..fvc {
            vs.push(check_index(c.read_bl_u()?, num_vertices, "face vertex")?);
        }
        consumed += fvc;
        faces.push(vs);
    }

    let num_edges = c.read_bl_u()? as usize;
    bounds_check(num_edges, "edge_count", CAP_EDGES, c.remaining_bits())?;
    let mut edges = Vec::with_capacity(num_edges);
    for _ in 0..num_edges {
        let a = check_index(c.read_bl_u()?, num_vertices, "edge start")?;
        let b = check_index(c.read_bl_u()?, num_vertices, "edge end")?;
        edges.push((a, b));
    }

    // Measured: the crease array carries its own `BL` count, which
    // equalled `edge_count` on both records of `sample_AC1032.dwg` but
    // is read rather than assumed.
    let num_creases = c.read_bl_u()? as usize;
    bounds_check(num_creases, "crease_count", CAP_CREASES, c.remaining_bits())?;
    let mut creases = Vec::with_capacity(num_creases);
    for _ in 0..num_creases {
        creases.push(c.read_bd()?);
    }

    let trailing_count = c.read_bl_u()?;
    if trailing_count != 0 {
        return Err(Error::Unsupported {
            feature: format!(
                "MESH trailing count {trailing_count} (DXF group 90 after the creases): \
                 only 0, with nothing after it, has been measured"
            ),
        });
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
        edges: &[(u32, u32)],
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
        // The face list is sized in BL entries: one count per face plus
        // that face's indices.
        let face_list_size: usize = faces.iter().map(|f| 1 + f.len()).sum();
        w.write_bl(face_list_size as i32);
        for f in faces {
            w.write_bl(f.len() as i32);
            for &i in *f {
                w.write_bl(i as i32);
            }
        }
        w.write_bl(edges.len() as i32);
        for &(a, b) in edges {
            w.write_bl_u(a);
            w.write_bl_u(b);
        }
        w.write_bl(creases.len() as i32);
        for &c in creases {
            w.write_bd(c);
        }
        w.write_bl(0);
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
        let edges = [(0u32, 1u32), (1, 2), (2, 0), (2, 3), (3, 0)];
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
        let edges = [(0u32, 1u32), (1, 2), (2, 0)];
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
    fn rejects_oversized_face_list_size() {
        let mut w = BitWriter::new();
        // Version + blend + subdivision_level + vertex_count(0)
        w.write_bs_u(0);
        w.write_b(false);
        w.write_bs_u(0);
        w.write_bs_u(0);
        // face_list_size far beyond CAP_FACE_LIST
        w.write_bl(20_000_000);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2010).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("face_list_size")),
            "err={err:?}"
        );
    }
}
