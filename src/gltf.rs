//! glTF 2.0 writer — JSON + binary buffer emission for static geometry export.
//!
//! Consumes [`crate::geometry::Mesh`] triangle soups and emits a glTF 2.0
//! document ([Khronos Group glTF 2.0 Specification][spec]) as a `(String,
//! Vec<u8>)` tuple: the JSON text plus the referenced binary buffer. The
//! caller decides how to package them — separate `.gltf` + `.bin` files,
//! or a single `.glb` container.
//!
//! [spec]: https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html
//!
//! # Scope
//!
//! Static geometry only: `scene` / `nodes` / `meshes` / `materials` /
//! `buffers` / `bufferViews` / `accessors`. Textures, samplers,
//! animations, skins, and morph targets are intentionally omitted —
//! DWG does not natively carry any of them at the entity level.
//!
//! # Pure-text emission
//!
//! This module deliberately avoids pulling in `serde_json` so the
//! library stays dependency-free for glTF export. JSON is emitted via
//! `format!` strings, same pattern as [`crate::svg`] and [`crate::dxf`].
//! Float values use Rust's default `{}` formatting, which is valid
//! JSON for finite floats (no trailing dot; no `NaN`/`Inf` — see
//! [`escape_f32`]).
//!
//! # Example
//!
//! ```
//! use dwg::gltf::GltfDoc;
//!
//! let doc = GltfDoc::new("empty");
//! let (json, bin) = doc.finish();
//! assert!(json.contains("\"asset\""));
//! assert!(json.contains("\"version\": \"2.0\""));
//! assert!(bin.is_empty());
//! ```
//!
//! # ACI → PBR baseColor
//!
//! The [AutoCAD Color Index](crate::color) is flattened onto the glTF
//! metallic-roughness material as `baseColorFactor`. Defaults are
//! non-metallic (`metallicFactor = 0.0`) and a CAD-friendly
//! `roughnessFactor = 0.8` so the AutoCAD flat-fill look survives the
//! round-trip into a PBR renderer without specular hot-spots.
//!
//! # Placeholder tessellation
//!
//! [`tessellate_surface_placeholder`] returns a degenerate single-
//! triangle mesh. Real SURFACE / NURBS tessellation is tracked by
//! GitHub issue #245 (SPLINE NURBS) and will replace this stub once
//! the knot-vector evaluator lands.

use crate::color::aci_to_rgb;
use crate::entities::{DecodedEntity, Point3D};
use crate::entity_geometry::{
    arc_to_curve, circle_to_curve, ellipse_to_curve, line_to_curve, lwpolyline_to_path,
    point_to_curve, three_d_face_to_mesh,
};
use crate::geometry::{BBox3, Mesh, Transform3};

/// Index of a registered material — alias of `usize` so callers see a
/// self-documenting type at API boundaries without wrapping.
pub type MaterialId = usize;

/// Index of a registered scene node — alias of `usize`. INSERT-style
/// instancing returns one of these from
/// [`GltfDoc::add_instanced_node`].
pub type NodeId = usize;

/// glTF primitive topology mode. Subset of the seven topologies the
/// glTF 2.0 spec defines (POINTS / LINES / LINE_LOOP / LINE_STRIP /
/// TRIANGLES / TRIANGLE_STRIP / TRIANGLE_FAN); we only emit two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimitiveMode {
    /// `mode = 1` — pairs of (a, b) vertex indices form one segment.
    /// Used for LINE / CIRCLE-tessellation / ARC-tessellation /
    /// POLYLINE entities.
    Lines,
    /// `mode = 4` — triples of (a, b, c) vertex indices form one
    /// triangle. Used for 3DFACE / SURFACE-bbox.
    Triangles,
}

impl PrimitiveMode {
    /// glTF integer code per spec table §5.21 (mesh.primitive.mode).
    fn as_gltf_code(self) -> u32 {
        match self {
            PrimitiveMode::Lines => 1,
            PrimitiveMode::Triangles => 4,
        }
    }
}

/// Output container selector for [`convert_file_to_gltf`] and
/// [`GltfDoc::to_glb`].
///
/// `Gltf` returns the JSON text bytes (UTF-8) — pair with a sibling
/// `.bin` buffer if non-empty. `Glb` returns a single-file binary
/// blob per the glTF 2.0 GLB spec (12-byte header + JSON chunk + BIN
/// chunk).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GltfFormat {
    /// glTF JSON (UTF-8 bytes). Binary buffer, if any, is dropped —
    /// the CLI sidecar-`.bin` packaging is callers' responsibility
    /// when they need it.
    Gltf,
    /// GLB binary container (single self-contained file).
    Glb,
}

/// glTF 2.0 document in progress. Meshes, materials, and nodes are
/// appended with the `add_*` methods; the complete document is
/// produced by [`finish`](Self::finish).
///
/// Layout in the emitted JSON:
///
/// - `asset.version = "2.0"`
/// - `scene = 0` → `scenes[0].nodes = [0, 1, ...]` (every added node)
/// - `nodes[i]` references `meshes[mesh_idx]` (one node per registered
///   mesh, plus optional pure-instance nodes that re-reference an
///   existing mesh under a different transform)
/// - `meshes[i].primitives[0]` references POSITION + indices accessors
/// - `buffers[0]` is a single blob; bufferViews slice it
/// - Two accessors per mesh: one VEC3 float POSITION, one SCALAR u32 indices
#[derive(Debug, Clone)]
pub struct GltfDoc {
    scene_name: String,
    /// One entry per registered mesh (POSITION + indices payload owner).
    meshes: Vec<MeshEntry>,
    /// One entry per registered scene node (mesh-owning OR pure
    /// instance referencing an existing mesh).
    nodes: Vec<NodeEntry>,
    /// One entry per registered material.
    materials: Vec<MaterialEntry>,
    /// Packed little-endian binary buffer. POSITION accessors are
    /// float32 VEC3 (12 bytes / vertex); indices accessors are u32
    /// SCALAR (4 bytes / index). Every bufferView starts on a 4-byte
    /// boundary because both payload types are 4-byte aligned.
    bin: Vec<u8>,
}

#[derive(Debug, Clone)]
struct MeshEntry {
    name: String,
    material_index: MaterialId,
    vertex_count: usize,
    /// For TRIANGLES this is the triangle count; for LINES it is the
    /// segment count. The accessor `count` is computed as
    /// `index_group_count * indices_per_group()`.
    index_group_count: usize,
    primitive_mode: PrimitiveMode,
    /// POSITION min (for glTF accessor requirement).
    pos_min: [f32; 3],
    /// POSITION max (for glTF accessor requirement).
    pos_max: [f32; 3],
}

impl MeshEntry {
    /// Number of vertex indices per primitive group: 2 for LINES,
    /// 3 for TRIANGLES. Multiplied by `index_group_count` to yield
    /// the indices-accessor `count`.
    fn indices_per_group(&self) -> usize {
        match self.primitive_mode {
            PrimitiveMode::Lines => 2,
            PrimitiveMode::Triangles => 3,
        }
    }

    /// Indices-accessor `count` field — the total index value count.
    fn index_count(&self) -> usize {
        self.index_group_count * self.indices_per_group()
    }
}

#[derive(Debug, Clone)]
struct NodeEntry {
    /// Mesh index that this node displays. Multiple nodes can point
    /// at the same mesh — that's the glTF instancing model.
    mesh_index: usize,
    /// Optional node transform (16-element column-major f32 matrix).
    transform: Option<[f32; 16]>,
}

#[derive(Debug, Clone)]
struct MaterialEntry {
    name: String,
    base_color_rgba: [f32; 4],
}

#[derive(Debug, Clone)]
struct BufferView {
    byte_offset: usize,
    byte_length: usize,
}

impl GltfDoc {
    /// Start a new document. The `scene_name` is written onto
    /// `scenes[0].name`.
    pub fn new(scene_name: &str) -> Self {
        GltfDoc {
            scene_name: scene_name.to_string(),
            meshes: Vec::new(),
            nodes: Vec::new(),
            materials: Vec::new(),
            bin: Vec::new(),
        }
    }

    /// Register a material with a PBR `baseColorFactor`. Returns the
    /// material index that later [`add_mesh`](Self::add_mesh) calls
    /// reference.
    ///
    /// The metallic-roughness model is forced to non-metallic
    /// (`metallicFactor = 0.0`) with a moderately rough surface
    /// (`roughnessFactor = 0.8`) so AutoCAD's flat-fill palette
    /// survives round-trip to a PBR renderer without acquiring a
    /// glossy specular highlight that wasn't in the source drawing.
    /// Callers that need true PBR can write a follow-up material module.
    pub fn add_material(&mut self, name: &str, base_color_rgba: [f32; 4]) -> MaterialId {
        let idx = self.materials.len();
        self.materials.push(MaterialEntry {
            name: name.to_string(),
            base_color_rgba,
        });
        idx
    }

    /// Convenience: register a material whose `baseColorFactor` comes
    /// from the AutoCAD Color Index. `aci` is looked up via
    /// [`crate::color::aci_to_rgb`]; alpha is fixed at 1.0. Returns the
    /// material index.
    ///
    /// (L10-02 / L10-06.) ACI 7 ("white/black", which is shown white on
    /// dark backgrounds and black on light ones) maps to RGB
    /// `(255, 255, 255)` and therefore `baseColorFactor = [1, 1, 1, 1]`
    /// here — Three.js callers that want a black-on-light look should
    /// set their viewer background, not override the factor.
    pub fn add_layer_material(&mut self, name: &str, aci: u8) -> MaterialId {
        let (r, g, b) = aci_to_rgb(aci);
        let rgba = [
            (r as f32) / 255.0,
            (g as f32) / 255.0,
            (b as f32) / 255.0,
            1.0,
        ];
        self.add_material(name, rgba)
    }

    /// Register a triangle mesh. Vertex positions are packed as
    /// little-endian f32 VEC3; triangle indices as little-endian u32
    /// SCALAR. Both arrays are appended to the shared binary buffer
    /// and a bufferView is recorded for each. Returns the node index
    /// (which is the mesh's owner — a 1:1 mesh→node mapping for
    /// non-instanced meshes).
    ///
    /// The material index must come from a prior
    /// [`add_material`](Self::add_material) or
    /// [`add_layer_material`](Self::add_layer_material) call; glTF
    /// validators will reject out-of-range material references.
    pub fn add_mesh(&mut self, name: &str, mesh: &Mesh, material_index: MaterialId) -> NodeId {
        self.add_mesh_with_transform(name, mesh, material_index, None)
    }

    /// Same as [`add_mesh`](Self::add_mesh) but the owning node is
    /// emitted with a non-identity local transform. The transform is
    /// f32-converted and written as a 16-element column-major
    /// [`glTF node.matrix`][matrix] field.
    ///
    /// [matrix]: https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#reference-node
    pub fn add_mesh_with_transform(
        &mut self,
        name: &str,
        mesh: &Mesh,
        material_index: MaterialId,
        transform: Option<Transform3>,
    ) -> NodeId {
        let vertex_count = mesh.vertices.len();
        let triangle_count = mesh.triangles.len();

        // POSITION bufferView: float32 VEC3
        let (pos_min, pos_max) = self.append_positions(&mesh.vertices);

        // indices bufferView: u32 SCALAR
        for tri in &mesh.triangles {
            self.bin.extend_from_slice(&tri[0].to_le_bytes());
            self.bin.extend_from_slice(&tri[1].to_le_bytes());
            self.bin.extend_from_slice(&tri[2].to_le_bytes());
        }

        let mesh_idx = self.meshes.len();
        self.meshes.push(MeshEntry {
            name: name.to_string(),
            material_index,
            vertex_count,
            index_group_count: triangle_count,
            primitive_mode: PrimitiveMode::Triangles,
            pos_min,
            pos_max,
        });
        let node_idx = self.nodes.len();
        let transform_m = transform.map(transform_to_f32_matrix);
        self.nodes.push(NodeEntry {
            mesh_index: mesh_idx,
            transform: transform_m,
        });
        node_idx
    }

    /// Register a line mesh — vertex positions packed as f32 VEC3,
    /// indices as u32 SCALAR pairs (one pair per segment). Emits a
    /// glTF primitive with `mode = 1` (LINES). Returns the node
    /// index.
    ///
    /// Used internally by [`add_entity_mesh`](Self::add_entity_mesh)
    /// for LINE / CIRCLE / ARC / POLYLINE entities. The `segments`
    /// slice is `[a_idx, b_idx]` pairs into `vertices`.
    pub fn add_line_mesh(
        &mut self,
        name: &str,
        vertices: &[Point3D],
        segments: &[[u32; 2]],
        material_index: MaterialId,
    ) -> NodeId {
        let vertex_count = vertices.len();
        let segment_count = segments.len();

        // POSITION
        let (pos_min, pos_max) = self.append_positions(vertices);

        // indices (u32 LE pairs)
        for seg in segments {
            self.bin.extend_from_slice(&seg[0].to_le_bytes());
            self.bin.extend_from_slice(&seg[1].to_le_bytes());
        }

        let mesh_idx = self.meshes.len();
        self.meshes.push(MeshEntry {
            name: name.to_string(),
            material_index,
            vertex_count,
            index_group_count: segment_count,
            primitive_mode: PrimitiveMode::Lines,
            pos_min,
            pos_max,
        });
        let node_idx = self.nodes.len();
        self.nodes.push(NodeEntry {
            mesh_index: mesh_idx,
            transform: None,
        });
        node_idx
    }

    /// Convert a [`DecodedEntity`] into a glTF primitive and append
    /// it to the document. (L10-03 / L10-04.)
    ///
    /// | Entity                                             | Output                     |
    /// |----------------------------------------------------|----------------------------|
    /// | LINE                                               | 1-segment line primitive   |
    /// | CIRCLE / ARC / ELLIPSE                             | tessellated line strip     |
    /// | POINT                                              | degenerate 1-vertex line   |
    /// | LWPOLYLINE                                         | line primitive (n-1 segs)  |
    /// | 3DFACE                                             | 1-or-2-tri triangle prim   |
    /// | EXTRUDEDSURFACE / REVOLVEDSURFACE / SWEPTSURFACE / LOFTEDSURFACE | bbox-derived 12-tri box (placeholder, see L10-04) |
    ///
    /// Returns `None` for entities whose geometry isn't yet wired
    /// (TEXT, MTEXT, INSERT, HATCH, DIMENSION, SPLINE, etc.). Callers
    /// can iterate `decoded_entities()` and discard `None`s — the
    /// pattern mirrors `dxf_convert::decoded_entity_to_record`.
    ///
    /// # Surface placeholders
    ///
    /// SURFACE entities currently land as bbox-derived triangle meshes
    /// — full SAT tessellation is deferred (no ACIS kernel in this
    /// crate; tracked by issue #360). The bbox falls back to a unit
    /// cube at the origin. The degradation is documented in the
    /// emitted mesh name (`<name>:placeholder-bbox`).
    pub fn add_entity_mesh(
        &mut self,
        name: &str,
        entity: &DecodedEntity,
        material_index: MaterialId,
    ) -> Option<NodeId> {
        match entity {
            DecodedEntity::Line(line) => {
                let curve = line_to_curve(line);
                let (verts, segs) = curve_to_line_primitive(&curve);
                Some(self.add_line_mesh(name, &verts, &segs, material_index))
            }
            DecodedEntity::Circle(circle) => {
                let curve = circle_to_curve(circle);
                let (verts, segs) = curve_to_line_primitive(&curve);
                Some(self.add_line_mesh(name, &verts, &segs, material_index))
            }
            DecodedEntity::Arc(arc) => {
                let curve = arc_to_curve(arc);
                let (verts, segs) = curve_to_line_primitive(&curve);
                Some(self.add_line_mesh(name, &verts, &segs, material_index))
            }
            DecodedEntity::Ellipse(ellipse) => {
                let curve = ellipse_to_curve(ellipse);
                let (verts, segs) = curve_to_line_primitive(&curve);
                Some(self.add_line_mesh(name, &verts, &segs, material_index))
            }
            DecodedEntity::Point(point) => {
                let curve = point_to_curve(point);
                let (verts, segs) = curve_to_line_primitive(&curve);
                Some(self.add_line_mesh(name, &verts, &segs, material_index))
            }
            DecodedEntity::LwPolyline(p) => {
                let path = lwpolyline_to_path(p);
                let (verts, segs) = path_to_line_primitive(&path);
                if segs.is_empty() {
                    return None;
                }
                Some(self.add_line_mesh(name, &verts, &segs, material_index))
            }
            DecodedEntity::ThreeDFace(face) => {
                let mesh = three_d_face_to_mesh(face).ok()?;
                Some(self.add_mesh(name, &mesh, material_index))
            }
            DecodedEntity::ExtrudedSurface(_)
            | DecodedEntity::RevolvedSurface(_)
            | DecodedEntity::SweptSurface(_)
            | DecodedEntity::LoftedSurface(_) => {
                // L10-04: full tessellation requires the ACIS kernel
                // we don't ship. Emit a unit-cube bbox placeholder so
                // downstream viewers see *something* in the right
                // place rather than a missing entity.
                let bbox = entity_bbox_or_unit_cube(entity);
                let mesh = bbox_to_box_mesh(&bbox);
                Some(self.add_mesh(&format!("{name}:placeholder-bbox"), &mesh, material_index))
            }
            // SPLINE, TEXT, MTEXT, INSERT, DIMENSION, HATCH, MLEADER,
            // VIEWPORT, IMAGE, RAY, XLINE, SOLID, TRACE, table
            // entries — not yet wired. Caller should report or skip.
            _ => None,
        }
    }

    /// Emit a pure instance node — references an already-registered
    /// mesh under a (composed) transform. (L10-05.) Used by
    /// INSERT-style block expansion: the master mesh lives once in
    /// `meshes[]` and each placement is a separate node pointing at
    /// the same mesh index with a different `matrix`.
    ///
    /// Composition: the supplied `transform` is the entity's local
    /// transform expressed in WCS-equivalent terms — callers that
    /// have a chain of nested-block transforms should fold them with
    /// [`Transform3::compose_chain`] BEFORE handing the result here.
    /// This function does NOT walk parent links.
    ///
    /// `child_mesh` MUST be a node-id returned by an earlier
    /// [`add_mesh`](Self::add_mesh) / [`add_line_mesh`](Self::add_line_mesh)
    /// / [`add_entity_mesh`](Self::add_entity_mesh) call on the same
    /// document. Out-of-range ids are not detected here (would land
    /// in the JSON as an undefined-mesh reference which validators
    /// reject); callers that synthesize ids from external state
    /// should pin this with an assertion.
    pub fn add_instanced_node(&mut self, child_mesh: NodeId, transform: &Transform3) -> NodeId {
        // Resolve child_mesh node-id → mesh-index. The 1:1 invariant
        // above means `nodes[child_mesh].mesh_index` is the right
        // target.
        let mesh_index = self
            .nodes
            .get(child_mesh)
            .map(|n| n.mesh_index)
            // Defensive fallback: if the caller hands us a bogus id,
            // instance mesh 0. This will produce a valid (if
            // unexpected) glTF document rather than a Rust panic.
            // Real callers should never hit this — see fn doc above.
            .unwrap_or(0);
        let node_idx = self.nodes.len();
        self.nodes.push(NodeEntry {
            mesh_index,
            transform: Some(transform_to_f32_matrix(*transform)),
        });
        node_idx
    }

    /// Internal: pack vertex positions into the binary buffer,
    /// returning the [min, max] required by glTF accessors. Empty
    /// vertex lists fall back to all-zero min/max so the emitted
    /// accessor stays valid.
    fn append_positions(&mut self, vertices: &[Point3D]) -> ([f32; 3], [f32; 3]) {
        let mut pos_min = [f32::INFINITY; 3];
        let mut pos_max = [f32::NEG_INFINITY; 3];
        for v in vertices {
            let xf = v.x as f32;
            let yf = v.y as f32;
            let zf = v.z as f32;
            self.bin.extend_from_slice(&xf.to_le_bytes());
            self.bin.extend_from_slice(&yf.to_le_bytes());
            self.bin.extend_from_slice(&zf.to_le_bytes());
            pos_min[0] = pos_min[0].min(xf);
            pos_min[1] = pos_min[1].min(yf);
            pos_min[2] = pos_min[2].min(zf);
            pos_max[0] = pos_max[0].max(xf);
            pos_max[1] = pos_max[1].max(yf);
            pos_max[2] = pos_max[2].max(zf);
        }
        if vertices.is_empty() {
            pos_min = [0.0; 3];
            pos_max = [0.0; 3];
        }
        (pos_min, pos_max)
    }

    /// Finalize the document and return `(json_text, binary_buffer)`.
    ///
    /// The caller owns packaging: for `.gltf` + `.bin`, write both and
    /// reference the bin via `buffers[0].uri` (the emitted JSON leaves
    /// `uri` absent, so downstream code must supply it or repackage
    /// as `.glb`). For `.glb`, see [`to_glb`](Self::to_glb).
    pub fn finish(self) -> (String, Vec<u8>) {
        let GltfDoc {
            scene_name,
            meshes,
            nodes,
            materials,
            bin,
        } = self;

        // Recompute bufferView offsets in the order meshes were added.
        // Layout = concatenation of (pos_0, idx_0, pos_1, idx_1, ...).
        let mut views: Vec<BufferView> = Vec::with_capacity(meshes.len() * 2);
        let mut cursor = 0usize;
        for m in &meshes {
            let pos_len = m.vertex_count * 12;
            views.push(BufferView {
                byte_offset: cursor,
                byte_length: pos_len,
            });
            cursor += pos_len;
            let idx_len = m.index_count() * 4;
            views.push(BufferView {
                byte_offset: cursor,
                byte_length: idx_len,
            });
            cursor += idx_len;
        }
        debug_assert_eq!(cursor, bin.len());

        let json = emit_json(&scene_name, &meshes, &nodes, &materials, &views, bin.len());
        (json, bin)
    }

    /// Finalize the document and return a single GLB-formatted byte
    /// buffer per the glTF 2.0 [Binary container][glb] spec §4.4.
    ///
    /// [glb]: https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#binary-gltf-layout
    ///
    /// Layout:
    ///
    /// ```text
    /// 12 bytes  header   "glTF" + 0x02000000 (version 2 LE) + total length LE
    /// 8  bytes  JSON chunk header   length + 0x4E4F534A ("JSON")
    /// N  bytes  JSON chunk data     padded with 0x20 (space) to 4-byte multiple
    /// 8  bytes  BIN chunk header    length + 0x004E4942 ("BIN\0")     [optional]
    /// N  bytes  BIN chunk data      padded with 0x00 to 4-byte multiple
    /// ```
    ///
    /// When the binary buffer is empty the BIN chunk is omitted entirely.
    pub fn to_glb(self) -> Vec<u8> {
        let (json, bin) = self.finish();
        encode_glb(json.as_bytes(), &bin)
    }
}

/// Placeholder surface-tessellation stub. Returns a degenerate single-
/// triangle mesh at the origin so callers can wire up the glTF pipeline
/// before real NURBS tessellation lands.
///
/// Tracked by issue #245 (SPLINE NURBS). When that issue ships, this
/// function will be removed and surface entities will route to the
/// actual tessellator.
pub fn tessellate_surface_placeholder() -> Mesh {
    let mut m = Mesh::empty();
    m.push_triangle(
        Point3D::new(0.0, 0.0, 0.0),
        Point3D::new(0.0, 0.0, 0.0),
        Point3D::new(0.0, 0.0, 0.0),
    );
    m
}

/// Convert a [`Transform3`] (row-major f64 4×4) into the 16-element
/// column-major f32 matrix that glTF wants on `node.matrix`.
fn transform_to_f32_matrix(t: Transform3) -> [f32; 16] {
    // glTF column-major: out[c*4 + r] = t.m[r][c]
    let mut out = [0.0f32; 16];
    for r in 0..4 {
        for c in 0..4 {
            out[c * 4 + r] = t.m[r][c] as f32;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Curve / Path → line-primitive (vertices + segment-pair indices) adapters.
// ---------------------------------------------------------------------------

/// Number of segments used to tessellate a full circle. 64 keeps the
/// chord error under ~0.1% of radius and keeps glTF buffer sizes
/// modest. Real renderers can request denser tessellation by post-
/// processing the line strip.
const CIRCLE_TESSELLATION_SEGMENTS: usize = 64;

fn curve_to_line_primitive(curve: &crate::curve::Curve) -> (Vec<Point3D>, Vec<[u32; 2]>) {
    use crate::curve::Curve;
    use crate::geometry::VecOps;
    match curve {
        Curve::Line { a, b } => (vec![*a, *b], vec![[0, 1]]),
        Curve::Circle { center, radius, .. } => {
            // Tessellate as a closed line strip in the XY plane of the
            // center. We don't apply the entity's normal here — full
            // OCS→WCS projection is L8-02 territory and would need
            // arbitrary_axis(). Bake-in is fine for the most common
            // case (normal = +Z).
            let mut verts = Vec::with_capacity(CIRCLE_TESSELLATION_SEGMENTS);
            for i in 0..CIRCLE_TESSELLATION_SEGMENTS {
                let theta =
                    (i as f64) * std::f64::consts::TAU / (CIRCLE_TESSELLATION_SEGMENTS as f64);
                verts.push(Point3D::new(
                    center.x + radius * theta.cos(),
                    center.y + radius * theta.sin(),
                    center.z,
                ));
            }
            let mut segs = Vec::with_capacity(CIRCLE_TESSELLATION_SEGMENTS);
            for i in 0..CIRCLE_TESSELLATION_SEGMENTS {
                let next = (i + 1) % CIRCLE_TESSELLATION_SEGMENTS;
                segs.push([i as u32, next as u32]);
            }
            (verts, segs)
        }
        Curve::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        } => {
            let mut sweep = end_angle - start_angle;
            // CCW sweep — same convention as bulge_to_arc + the SVG
            // emitter. Negative spans wrap forward.
            if sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
            // Match arc-density to circle-density on equivalent radius.
            let segs_count = ((CIRCLE_TESSELLATION_SEGMENTS as f64)
                * (sweep / std::f64::consts::TAU))
                .ceil()
                .max(1.0) as usize;
            let mut verts = Vec::with_capacity(segs_count + 1);
            for i in 0..=segs_count {
                let t = (i as f64) / (segs_count as f64);
                let theta = start_angle + sweep * t;
                verts.push(Point3D::new(
                    center.x + radius * theta.cos(),
                    center.y + radius * theta.sin(),
                    center.z,
                ));
            }
            let mut segs = Vec::with_capacity(segs_count);
            for i in 0..segs_count {
                segs.push([i as u32, (i + 1) as u32]);
            }
            (verts, segs)
        }
        Curve::Ellipse {
            center,
            major_axis,
            ratio,
            start_angle,
            end_angle,
            ..
        } => {
            // Standard parametric: P(t) = C + cos(t)·major + sin(t)·minor
            // where minor = major rotated 90° in the entity plane,
            // scaled by |major| · ratio.
            let major_len = major_axis.length();
            // Minor axis perpendicular to major in the XY plane (good
            // enough for the common +Z normal case; full OCS handling
            // is L8-02).
            let minor_dir = crate::entities::Vec3D {
                x: -major_axis.y,
                y: major_axis.x,
                z: 0.0,
            }
            .normalize(1e-12);
            let minor_len = major_len * ratio;
            let mut sweep = end_angle - start_angle;
            if sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
            let segs_count = ((CIRCLE_TESSELLATION_SEGMENTS as f64)
                * (sweep / std::f64::consts::TAU))
                .ceil()
                .max(1.0) as usize;
            let mut verts = Vec::with_capacity(segs_count + 1);
            for i in 0..=segs_count {
                let t = (i as f64) / (segs_count as f64);
                let theta = start_angle + sweep * t;
                let cos_t = theta.cos();
                let sin_t = theta.sin();
                verts.push(Point3D::new(
                    center.x + cos_t * major_axis.x + sin_t * minor_dir.x * minor_len,
                    center.y + cos_t * major_axis.y + sin_t * minor_dir.y * minor_len,
                    center.z,
                ));
            }
            let mut segs = Vec::with_capacity(segs_count);
            for i in 0..segs_count {
                segs.push([i as u32, (i + 1) as u32]);
            }
            (verts, segs)
        }
        Curve::Polyline { vertices, closed } => {
            let mut verts: Vec<Point3D> = vertices.iter().map(|v| v.point).collect();
            let mut segs = Vec::with_capacity(verts.len());
            if verts.len() >= 2 {
                for i in 0..(verts.len() - 1) {
                    segs.push([i as u32, (i + 1) as u32]);
                }
                if *closed {
                    segs.push([(verts.len() - 1) as u32, 0]);
                }
            } else if verts.len() == 1 {
                // Degenerate: emit a self-segment so the renderer
                // still sees a vertex.
                verts.push(verts[0]);
                segs.push([0, 1]);
            }
            (verts, segs)
        }
        // SPLINE → fall back to chord through control points so the
        // renderer at least sees something. Real NURBS tessellation
        // is L8-14.
        Curve::Spline(s) => {
            let mut verts: Vec<Point3D> = s.control_points.clone();
            let mut segs = Vec::with_capacity(verts.len());
            if verts.len() >= 2 {
                for i in 0..(verts.len() - 1) {
                    segs.push([i as u32, (i + 1) as u32]);
                }
            } else if verts.len() == 1 {
                verts.push(verts[0]);
                segs.push([0, 1]);
            }
            (verts, segs)
        }
        Curve::Helix { .. } => {
            // Helix tessellation is L8-14 territory — emit a single
            // origin vertex pair for now so the entity isn't dropped.
            (vec![Point3D::new(0.0, 0.0, 0.0); 2], vec![[0, 1]])
        }
        Curve::TextBaseline { insertion, .. } => {
            // TEXT geometry is a placement record, not a renderable
            // primitive at the line-mesh layer. Emit a degenerate pair
            // at the insertion point so the gltf scene retains the
            // anchor location for downstream font-render passes.
            (vec![*insertion; 2], vec![[0, 1]])
        }
    }
}

fn path_to_line_primitive(path: &crate::curve::Path) -> (Vec<Point3D>, Vec<[u32; 2]>) {
    let mut verts: Vec<Point3D> = Vec::new();
    let mut segs: Vec<[u32; 2]> = Vec::new();
    for seg in &path.segments {
        let (sv, ss) = curve_to_line_primitive(seg);
        let base = verts.len() as u32;
        verts.extend_from_slice(&sv);
        for s in &ss {
            segs.push([s[0] + base, s[1] + base]);
        }
    }
    (verts, segs)
}

// ---------------------------------------------------------------------------
// Surface placeholder helpers (L10-04).
// ---------------------------------------------------------------------------

/// Best-effort bbox extraction for surface entities. Without an ACIS
/// kernel we can't introspect the SAT blob; falls back to a unit cube
/// centered at the origin so the placeholder mesh still has measurable
/// size.
fn entity_bbox_or_unit_cube(entity: &DecodedEntity) -> BBox3 {
    // Future work: grep textual SAT for "point" records to derive a
    // tighter bbox without a full kernel.
    let _ = entity;
    BBox3 {
        min: Point3D::new(-0.5, -0.5, -0.5),
        max: Point3D::new(0.5, 0.5, 0.5),
    }
}

/// Convert a 3D bbox into a 12-triangle box [`Mesh`]. Triangles wind
/// CCW from outside per the glTF default front-face convention.
fn bbox_to_box_mesh(b: &BBox3) -> Mesh {
    let (lo, hi) = (b.min, b.max);
    let p000 = Point3D::new(lo.x, lo.y, lo.z);
    let p100 = Point3D::new(hi.x, lo.y, lo.z);
    let p110 = Point3D::new(hi.x, hi.y, lo.z);
    let p010 = Point3D::new(lo.x, hi.y, lo.z);
    let p001 = Point3D::new(lo.x, lo.y, hi.z);
    let p101 = Point3D::new(hi.x, lo.y, hi.z);
    let p111 = Point3D::new(hi.x, hi.y, hi.z);
    let p011 = Point3D::new(lo.x, hi.y, hi.z);
    let mut m = Mesh::empty();
    // -Z (bottom)
    m.push_quad(p000, p010, p110, p100);
    // +Z (top)
    m.push_quad(p001, p101, p111, p011);
    // -Y (front)
    m.push_quad(p000, p100, p101, p001);
    // +Y (back)
    m.push_quad(p010, p011, p111, p110);
    // -X (left)
    m.push_quad(p000, p001, p011, p010);
    // +X (right)
    m.push_quad(p100, p110, p111, p101);
    m
}

// ---------------------------------------------------------------------------
// File-level convert entry point (L10-07 library spine).
// ---------------------------------------------------------------------------

/// Open `path` as a DWG, decode every entity, and emit a glTF document
/// in the requested format. Library-level entry for the `dwg-to-gltf`
/// CLI; isolated here so downstream consumers can convert without
/// depending on `clap` / `anyhow`.
///
/// Defaults applied:
///
/// - Single shared material `"layer-7"` (ACI 7 = white). Per-layer
///   material splitting will land once the entity layer surfaces
///   common-entity color/layer fields uniformly (tracked alongside
///   L8-21).
/// - Identity scene transform.
/// - Best-effort decode — entities that don't yet have a glTF
///   adapter are skipped silently (same convention as
///   `dxf_convert::convert_dwg_to_dxf`'s skip-list).
///
/// Returns the JSON bytes ([`GltfFormat::Gltf`]) or the GLB binary
/// blob ([`GltfFormat::Glb`]). For `Gltf` callers that want the
/// binary buffer alongside, build a [`GltfDoc`] manually and call
/// [`GltfDoc::finish`] instead — `convert_file_to_gltf` collapses to
/// JSON-only because the CLI sidecar-`.bin` packaging is the caller's
/// job.
pub fn convert_file_to_gltf(path: &std::path::Path, format: GltfFormat) -> crate::Result<Vec<u8>> {
    let file = crate::DwgFile::open(path)?;
    let scene_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("dwg-scene");
    convert_dwg_to_gltf(&file, format, scene_name)
}

/// Same as [`convert_file_to_gltf`] but takes an already-opened
/// [`crate::DwgFile`]. Useful for callers that already have a
/// parsed handle (browser uploads, in-memory fixtures, integration
/// tests). `scene_name` is the logical name embedded in the glTF
/// `scene.name` field.
pub fn convert_dwg_to_gltf(
    file: &crate::DwgFile,
    format: GltfFormat,
    scene_name: &str,
) -> crate::Result<Vec<u8>> {
    let mut doc = GltfDoc::new(scene_name);
    let mat = doc.add_layer_material("layer-7", 7);
    if let Some(decoded_res) = file.decoded_entities() {
        let (decoded_list, _summary) = decoded_res?;
        for (i, entity) in decoded_list.iter().enumerate() {
            let _ = doc.add_entity_mesh(&format!("e{i}"), entity, mat);
        }
    }
    Ok(match format {
        GltfFormat::Gltf => {
            let (json, _bin) = doc.finish();
            json.into_bytes()
        }
        GltfFormat::Glb => doc.to_glb(),
    })
}

// ---------------------------------------------------------------------------
// GLB binary container (L10-07 packaging).
// ---------------------------------------------------------------------------

/// Encode a `(json, bin)` pair into a single GLB-formatted byte
/// vector. Emits the JSON chunk unconditionally; emits the BIN chunk
/// only when `bin` is non-empty (the GLB spec allows omitting it).
fn encode_glb(json: &[u8], bin: &[u8]) -> Vec<u8> {
    // Pad the JSON chunk with ASCII spaces (0x20) and the BIN chunk
    // with NUL (0x00) to a 4-byte boundary, per the spec.
    let json_padded_len = (json.len() + 3) & !3;
    let bin_padded_len = (bin.len() + 3) & !3;

    let header_len = 12usize;
    let json_chunk_len = 8 + json_padded_len;
    let bin_chunk_len = if bin.is_empty() {
        0
    } else {
        8 + bin_padded_len
    };
    let total_len = header_len + json_chunk_len + bin_chunk_len;

    let mut out = Vec::with_capacity(total_len);

    // 12-byte header
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total_len as u32).to_le_bytes());

    // JSON chunk
    out.extend_from_slice(&(json_padded_len as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(json);
    out.resize(out.len() + (json_padded_len - json.len()), 0x20);

    // BIN chunk (optional)
    if !bin.is_empty() {
        out.extend_from_slice(&(bin_padded_len as u32).to_le_bytes());
        // ASCII "BIN\0" = 0x42, 0x49, 0x4E, 0x00.
        out.extend_from_slice(&[0x42, 0x49, 0x4E, 0x00]);
        out.extend_from_slice(bin);
        out.resize(out.len() + (bin_padded_len - bin.len()), 0x00);
    }

    debug_assert_eq!(out.len(), total_len);
    out
}

// ---------------------------------------------------------------------------
// JSON emission helpers (no serde_json dependency).
// ---------------------------------------------------------------------------

fn emit_json(
    scene_name: &str,
    meshes: &[MeshEntry],
    nodes: &[NodeEntry],
    materials: &[MaterialEntry],
    views: &[BufferView],
    buffer_len: usize,
) -> String {
    let mut out = String::new();
    out.push('{');
    out.push('\n');

    // asset
    out.push_str("  \"asset\": { \"version\": \"2.0\", \"generator\": \"dwg-rs glTF writer\" },\n");

    // scene + scenes
    out.push_str("  \"scene\": 0,\n");
    out.push_str("  \"scenes\": [ { \"name\": \"");
    out.push_str(&json_escape(scene_name));
    out.push_str("\", \"nodes\": [");
    for i in 0..nodes.len() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&i.to_string());
    }
    out.push_str("] } ],\n");

    // nodes
    out.push_str("  \"nodes\": [");
    for (i, n) in nodes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n    { \"mesh\": ");
        out.push_str(&n.mesh_index.to_string());
        if let Some(mat) = &n.transform {
            out.push_str(", \"matrix\": [");
            for (k, v) in mat.iter().enumerate() {
                if k > 0 {
                    out.push_str(", ");
                }
                out.push_str(&escape_f32(*v));
            }
            out.push(']');
        }
        out.push_str(" }");
    }
    out.push_str("\n  ],\n");

    // meshes
    out.push_str("  \"meshes\": [");
    for (i, m) in meshes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let pos_accessor = i * 2;
        let idx_accessor = i * 2 + 1;
        out.push_str("\n    { \"name\": \"");
        out.push_str(&json_escape(&m.name));
        out.push_str("\", \"primitives\": [ { \"attributes\": { \"POSITION\": ");
        out.push_str(&pos_accessor.to_string());
        out.push_str(" }, \"indices\": ");
        out.push_str(&idx_accessor.to_string());
        out.push_str(", \"material\": ");
        out.push_str(&m.material_index.to_string());
        out.push_str(", \"mode\": ");
        out.push_str(&m.primitive_mode.as_gltf_code().to_string());
        out.push_str(" } ] }");
    }
    out.push_str("\n  ],\n");

    // materials
    out.push_str("  \"materials\": [");
    for (i, mat) in materials.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n    { \"name\": \"");
        out.push_str(&json_escape(&mat.name));
        out.push_str("\", \"pbrMetallicRoughness\": { \"baseColorFactor\": [");
        for (k, v) in mat.base_color_rgba.iter().enumerate() {
            if k > 0 {
                out.push_str(", ");
            }
            out.push_str(&escape_f32(*v));
        }
        out.push_str("], \"metallicFactor\": 0.0, \"roughnessFactor\": 0.8 } }");
    }
    out.push_str("\n  ],\n");

    // buffers
    out.push_str("  \"buffers\": [ { \"byteLength\": ");
    out.push_str(&buffer_len.to_string());
    out.push_str(" } ],\n");

    // bufferViews
    out.push_str("  \"bufferViews\": [");
    for (i, v) in views.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        // target: 34962 = ARRAY_BUFFER (POSITION), 34963 = ELEMENT_ARRAY_BUFFER (indices)
        let target = if i % 2 == 0 { 34962 } else { 34963 };
        out.push_str("\n    { \"buffer\": 0, \"byteOffset\": ");
        out.push_str(&v.byte_offset.to_string());
        out.push_str(", \"byteLength\": ");
        out.push_str(&v.byte_length.to_string());
        out.push_str(", \"target\": ");
        out.push_str(&target.to_string());
        out.push_str(" }");
    }
    out.push_str("\n  ],\n");

    // accessors (two per mesh: POSITION then indices)
    out.push_str("  \"accessors\": [");
    for (i, m) in meshes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        // POSITION: VEC3 float (componentType 5126)
        out.push_str("\n    { \"bufferView\": ");
        out.push_str(&(i * 2).to_string());
        out.push_str(", \"componentType\": 5126, \"count\": ");
        out.push_str(&m.vertex_count.to_string());
        out.push_str(", \"type\": \"VEC3\", \"min\": [");
        for (k, v) in m.pos_min.iter().enumerate() {
            if k > 0 {
                out.push_str(", ");
            }
            out.push_str(&escape_f32(*v));
        }
        out.push_str("], \"max\": [");
        for (k, v) in m.pos_max.iter().enumerate() {
            if k > 0 {
                out.push_str(", ");
            }
            out.push_str(&escape_f32(*v));
        }
        out.push_str("] },");

        // indices: SCALAR u32 (componentType 5125)
        out.push_str("\n    { \"bufferView\": ");
        out.push_str(&(i * 2 + 1).to_string());
        out.push_str(", \"componentType\": 5125, \"count\": ");
        out.push_str(&m.index_count().to_string());
        out.push_str(", \"type\": \"SCALAR\" }");
    }
    out.push_str("\n  ]\n");

    out.push('}');
    out
}

/// Escape a string for embedding inside a JSON string literal.
/// Handles the structural characters the spec requires (`\`, `"`,
/// control range) — does NOT transcode non-ASCII, since UTF-8 is a
/// valid JSON string representation per RFC 8259 §7.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Format an f32 as JSON. JSON does not allow `NaN` / `+Inf` / `-Inf`,
/// so we substitute `0` for non-finite values (conservative choice;
/// glTF validators reject non-finite accessors).
fn escape_f32(v: f32) -> String {
    if v.is_finite() {
        // Rust's default `{}` on f32 emits shortest round-trip decimal
        // representation, which is valid JSON.
        format!("{v}")
    } else {
        "0".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::Point3D;

    #[test]
    fn empty_doc_has_valid_asset_block() {
        let doc = GltfDoc::new("empty_scene");
        let (json, bin) = doc.finish();
        assert!(json.contains("\"asset\""));
        assert!(json.contains("\"version\": \"2.0\""));
        assert!(json.contains("\"generator\""));
        assert!(json.contains("\"scene\": 0"));
        assert!(json.contains("\"name\": \"empty_scene\""));
        assert!(bin.is_empty());
    }

    #[test]
    fn add_mesh_produces_meshes_zero_entry_with_right_indices() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [0.5, 0.5, 0.5, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        let idx = doc.add_mesh("triangle", &mesh, mat);
        assert_eq!(idx, 0);
        let (json, _) = doc.finish();
        assert!(json.contains("\"name\": \"triangle\""));
        // POSITION accessor is index 0, indices accessor is index 1 for mesh 0.
        assert!(json.contains("\"POSITION\": 0"));
        assert!(json.contains("\"indices\": 1"));
        assert!(json.contains("\"material\": 0"));
        // mode 4 = TRIANGLES
        assert!(json.contains("\"mode\": 4"));
    }

    #[test]
    fn add_material_red_aci_maps_to_base_color_factor() {
        let mut doc = GltfDoc::new("s");
        let idx = doc.add_layer_material("red_layer", 1);
        assert_eq!(idx, 0);
        let (json, _) = doc.finish();
        // ACI 1 = pure red → [1.0, 0.0, 0.0, 1.0].
        // f32 of 1.0 formats as "1", 0.0 as "0".
        assert!(
            json.contains("[1, 0, 0, 1]"),
            "expected [1, 0, 0, 1] in JSON, got: {json}"
        );
        assert!(json.contains("\"name\": \"red_layer\""));
        assert!(json.contains("\"metallicFactor\": 0"));
        assert!(json.contains("\"roughnessFactor\": 0.8"));
    }

    #[test]
    fn add_layer_material_white_aci_seven_emits_pure_white_base_color() {
        // L10-02 acceptance test: layer "0" defaults to ACI 7
        // (white/black). Round-trip into glTF must produce
        // baseColorFactor = [1, 1, 1, 1].
        let mut doc = GltfDoc::new("s");
        doc.add_layer_material("0", 7);
        let (json, _) = doc.finish();
        assert!(
            json.contains("[1, 1, 1, 1]"),
            "expected ACI 7 (white) → [1, 1, 1, 1] in JSON, got: {json}"
        );
        assert!(json.contains("\"name\": \"0\""));
    }

    #[test]
    fn add_layer_material_red_aci_one_produces_pure_red_rgb_components() {
        // L10-06 verification: ACI 1 (red) → [1, 0, 0]. Pinned
        // separately from the [1,0,0,1] check above so the RGB-only
        // contract is explicit and changes to alpha-handling don't
        // silently invalidate L10-06.
        let (r, g, b) = aci_to_rgb(1);
        assert_eq!((r, g, b), (255, 0, 0));
        let mut doc = GltfDoc::new("s");
        doc.add_layer_material("red", 1);
        let (json, _) = doc.finish();
        // The RGB tail "1, 0, 0" must precede the alpha component.
        assert!(json.contains("[1, 0, 0, 1]"));
    }

    #[test]
    fn add_mesh_writes_vertex_and_index_data_at_right_offsets() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        // Three distinct points so we can pattern-match on the bytes.
        mesh.push_triangle(
            Point3D::new(1.0, 2.0, 3.0),
            Point3D::new(4.0, 5.0, 6.0),
            Point3D::new(7.0, 8.0, 9.0),
        );
        doc.add_mesh("t", &mesh, mat);
        let (json, bin) = doc.finish();
        // 3 vertices * 12 bytes + 3 indices * 4 bytes = 36 + 12 = 48.
        assert_eq!(bin.len(), 48);
        // First vertex: (1.0, 2.0, 3.0) as f32 LE.
        let x: [u8; 4] = bin[0..4].try_into().unwrap();
        let y: [u8; 4] = bin[4..8].try_into().unwrap();
        let z: [u8; 4] = bin[8..12].try_into().unwrap();
        assert_eq!(f32::from_le_bytes(x), 1.0);
        assert_eq!(f32::from_le_bytes(y), 2.0);
        assert_eq!(f32::from_le_bytes(z), 3.0);
        // First index begins at byte 36.
        let i0: [u8; 4] = bin[36..40].try_into().unwrap();
        assert_eq!(u32::from_le_bytes(i0), 0);
        // bufferView lengths encoded correctly.
        assert!(json.contains("\"byteLength\": 36"));
        assert!(json.contains("\"byteLength\": 12"));
        assert!(json.contains("\"byteLength\": 48"));
        // Accessor types present.
        assert!(json.contains("\"type\": \"VEC3\""));
        assert!(json.contains("\"type\": \"SCALAR\""));
        assert!(json.contains("\"componentType\": 5126"));
        assert!(json.contains("\"componentType\": 5125"));
    }

    #[test]
    fn transform_on_node_is_emitted_as_sixteen_element_matrix() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        let t = Transform3::translation(10.0, 20.0, 30.0);
        doc.add_mesh_with_transform("t", &mesh, mat, Some(t));
        let (json, _) = doc.finish();
        assert!(json.contains("\"matrix\""));
        // Column-major translation: the translation components sit in
        // slots 12, 13, 14 (the last column).
        // Looking for "10, 20, 30, 1" at the tail of the 16-element array.
        assert!(
            json.contains("10, 20, 30, 1]"),
            "expected translation tail '10, 20, 30, 1]' in matrix, got: {json}"
        );
        // Count commas inside the matrix array to confirm 16 elements (15 separators).
        let start = json.find("\"matrix\": [").expect("matrix key present");
        let sub = &json[start..];
        let end = sub.find(']').unwrap();
        let arr = &sub[..end];
        assert_eq!(
            arr.matches(',').count(),
            15,
            "matrix should have 16 elements"
        );
    }

    #[test]
    fn finish_produces_json_that_starts_with_brace_and_ends_with_brace() {
        let doc = GltfDoc::new("x");
        let (json, _) = doc.finish();
        assert!(json.starts_with('{'), "json must start with '{{'");
        assert!(json.ends_with('}'), "json must end with '}}'");
    }

    #[test]
    fn layer_material_green_aci_3_maps_to_green() {
        let mut doc = GltfDoc::new("s");
        doc.add_layer_material("green", 3);
        let (json, _) = doc.finish();
        // ACI 3 = pure green → [0.0, 1.0, 0.0, 1.0] → "[0, 1, 0, 1]".
        assert!(json.contains("[0, 1, 0, 1]"));
    }

    #[test]
    fn tessellate_surface_placeholder_returns_degenerate_triangle() {
        let m = tessellate_surface_placeholder();
        assert_eq!(m.triangles.len(), 1);
        assert_eq!(m.vertices.len(), 3);
        // All three vertices at the origin — degenerate.
        for v in &m.vertices {
            assert_eq!(*v, Point3D::new(0.0, 0.0, 0.0));
        }
    }

    #[test]
    fn two_meshes_get_sequential_buffer_view_indices() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        let a = doc.add_mesh("a", &mesh, mat);
        let b = doc.add_mesh("b", &mesh, mat);
        assert_eq!(a, 0);
        assert_eq!(b, 1);
        let (json, bin) = doc.finish();
        // Two meshes * (3 vertices * 12 + 3 indices * 4) = 2 * 48 = 96 bytes.
        assert_eq!(bin.len(), 96);
        // Mesh 1 references POSITION accessor 2 and indices accessor 3.
        assert!(json.contains("\"POSITION\": 2"));
        assert!(json.contains("\"indices\": 3"));
    }

    #[test]
    fn identity_transform_still_emitted_when_provided() {
        // Exercises the "Some(Transform3::identity())" path so callers
        // can force a node.matrix emission even if it's the identity.
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        doc.add_mesh_with_transform("t", &mesh, mat, Some(Transform3::identity()));
        let (json, _) = doc.finish();
        assert!(json.contains("\"matrix\""));
    }

    #[test]
    fn no_transform_skips_matrix_field() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        doc.add_mesh("t", &mesh, mat);
        let (json, _) = doc.finish();
        assert!(!json.contains("\"matrix\""));
    }

    #[test]
    fn position_min_max_computed_from_vertices() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(-5.0, -3.0, 0.0),
            Point3D::new(10.0, 0.0, 2.0),
            Point3D::new(0.0, 7.0, -1.0),
        );
        doc.add_mesh("t", &mesh, mat);
        let (json, _) = doc.finish();
        // min = (-5, -3, -1), max = (10, 7, 2).
        assert!(json.contains("\"min\": [-5, -3, -1]"));
        assert!(json.contains("\"max\": [10, 7, 2]"));
    }

    #[test]
    fn json_escape_handles_structural_characters() {
        assert_eq!(json_escape("a\"b"), "a\\\"b");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
        assert_eq!(json_escape("plain"), "plain");
    }

    #[test]
    fn escape_f32_replaces_non_finite_with_zero() {
        assert_eq!(escape_f32(f32::NAN), "0");
        assert_eq!(escape_f32(f32::INFINITY), "0");
        assert_eq!(escape_f32(f32::NEG_INFINITY), "0");
        assert_eq!(escape_f32(1.5), "1.5");
        assert_eq!(escape_f32(0.0), "0");
    }

    #[test]
    fn scene_name_is_json_escaped() {
        let doc = GltfDoc::new("scene \"quoted\"");
        let (json, _) = doc.finish();
        assert!(json.contains("\"name\": \"scene \\\"quoted\\\"\""));
    }

    #[test]
    fn buffer_view_targets_are_array_and_element_array() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        doc.add_mesh("t", &mesh, mat);
        let (json, _) = doc.finish();
        // 34962 = ARRAY_BUFFER (POSITION)
        assert!(json.contains("\"target\": 34962"));
        // 34963 = ELEMENT_ARRAY_BUFFER (indices)
        assert!(json.contains("\"target\": 34963"));
    }

    // -----------------------------------------------------------------
    // L10-03 — line-primitive emission
    // -----------------------------------------------------------------

    #[test]
    fn add_line_mesh_emits_mode_one_primitive() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let verts = vec![
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 1.0, 1.0),
            Point3D::new(2.0, 2.0, 2.0),
        ];
        let segs = vec![[0u32, 1u32], [1u32, 2u32]];
        doc.add_line_mesh("polyline", &verts, &segs, mat);
        let (json, bin) = doc.finish();
        // 3 verts * 12 bytes + 2 segments * 2 indices * 4 bytes = 36 + 16 = 52.
        assert_eq!(bin.len(), 52);
        // mode 1 = LINES per glTF spec.
        assert!(
            json.contains("\"mode\": 1"),
            "expected line primitive mode 1, got: {json}"
        );
        // indices accessor count = 2 segments * 2 = 4.
        assert!(json.contains("\"componentType\": 5125, \"count\": 4"));
    }

    #[test]
    fn add_entity_mesh_line_emits_line_primitive() {
        use crate::entities::{Vec3D, line::Line};
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let line = Line {
            start: Point3D::new(0.0, 0.0, 0.0),
            end: Point3D::new(10.0, 10.0, 0.0),
            thickness: 0.0,
            extrusion: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            is_2d: true,
        };
        let entity = DecodedEntity::Line(line);
        let node = doc.add_entity_mesh("e0", &entity, mat).expect("line emits");
        assert_eq!(node, 0);
        let (json, _) = doc.finish();
        assert!(json.contains("\"mode\": 1"));
        assert!(json.contains("\"name\": \"e0\""));
    }

    #[test]
    fn add_entity_mesh_circle_emits_tessellated_line_strip() {
        use crate::entities::{Vec3D, circle::Circle};
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let c = Circle {
            center: Point3D::new(0.0, 0.0, 0.0),
            radius: 1.0,
            thickness: 0.0,
            extrusion: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        };
        let entity = DecodedEntity::Circle(c);
        doc.add_entity_mesh("circle", &entity, mat)
            .expect("circle emits");
        let (json, bin) = doc.finish();
        // 64-segment circle: 64 vertices * 12 + 64 segments * 8 = 768 + 512 = 1280.
        assert_eq!(bin.len(), 1280);
        assert!(json.contains("\"mode\": 1"));
    }

    #[test]
    fn add_entity_mesh_3dface_emits_triangle_primitive() {
        use crate::entities::three_d_face::ThreeDFace;
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let face = ThreeDFace {
            corners: [
                Point3D::new(0.0, 0.0, 0.0),
                Point3D::new(1.0, 0.0, 0.0),
                Point3D::new(1.0, 1.0, 0.0),
                Point3D::new(0.0, 1.0, 0.0),
            ],
            invisible_edges: 0,
            is_triangle: false,
        };
        let entity = DecodedEntity::ThreeDFace(face);
        doc.add_entity_mesh("face", &entity, mat).expect("emits");
        let (json, _) = doc.finish();
        assert!(json.contains("\"mode\": 4"));
    }

    // -----------------------------------------------------------------
    // L10-05 — instanced node composition
    // -----------------------------------------------------------------

    #[test]
    fn add_instanced_node_appends_node_pointing_at_existing_mesh() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        let parent = doc.add_mesh("master", &mesh, mat);
        let t = Transform3::translation(10.0, 0.0, 0.0);
        let inst = doc.add_instanced_node(parent, &t);
        // Instance node id is sequential after the parent.
        assert!(inst > parent);
        let (json, bin) = doc.finish();
        // The mesh is registered exactly once.
        assert_eq!(bin.len(), 48);
        // Two scene nodes — both point at mesh 0.
        let mesh_zero_count = json.matches("\"mesh\": 0").count();
        assert!(
            mesh_zero_count >= 2,
            "expected both nodes to reference mesh 0, json: {json}"
        );
    }

    #[test]
    fn add_instanced_node_composes_translation_into_matrix() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        let parent = doc.add_mesh("master", &mesh, mat);
        let chain = Transform3::compose_chain(&[
            Transform3::translation(5.0, 0.0, 0.0),
            Transform3::translation(0.0, 7.0, 0.0),
        ]);
        doc.add_instanced_node(parent, &chain);
        let (json, _) = doc.finish();
        // Composed translation lands at slots 12, 13, 14 (column-major
        // last column). 5 + 0 = 5 along X, 0 + 7 = 7 along Y, 0 along Z.
        assert!(
            json.contains("5, 7, 0, 1]"),
            "expected composed translation '5, 7, 0, 1]' in matrix, got: {json}"
        );
    }

    // -----------------------------------------------------------------
    // L10-07 — GLB binary container
    // -----------------------------------------------------------------

    #[test]
    fn glb_starts_with_magic_and_records_total_length() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        doc.add_mesh("t", &mesh, mat);
        let glb = doc.to_glb();
        // Magic
        assert_eq!(&glb[0..4], b"glTF");
        // Version 2 LE
        assert_eq!(&glb[4..8], &2u32.to_le_bytes());
        // Total length matches buffer length
        let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, glb.len());
        // JSON chunk type
        assert_eq!(&glb[16..20], b"JSON");
    }

    #[test]
    fn glb_with_no_bin_omits_bin_chunk() {
        let doc = GltfDoc::new("s");
        let glb = doc.to_glb();
        // Empty doc has empty buffer — only header + JSON chunk.
        assert_eq!(&glb[0..4], b"glTF");
        // Confirm no "BIN\0" magic anywhere in the file.
        assert!(
            !glb.windows(4).any(|w| w == [0x42, 0x49, 0x4E, 0x00]),
            "BIN chunk header must not appear when binary buffer is empty"
        );
    }

    #[test]
    fn glb_with_bin_includes_bin_chunk_header() {
        let mut doc = GltfDoc::new("s");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        doc.add_mesh("t", &mesh, mat);
        let glb = doc.to_glb();
        // BIN chunk magic must appear.
        assert!(
            glb.windows(4).any(|w| w == [0x42, 0x49, 0x4E, 0x00]),
            "BIN chunk header must appear when binary buffer is non-empty"
        );
    }

    #[test]
    fn glb_chunk_lengths_are_4_byte_aligned() {
        let mut doc = GltfDoc::new("hello");
        let mat = doc.add_material("m", [1.0, 1.0, 1.0, 1.0]);
        let mut mesh = Mesh::empty();
        mesh.push_triangle(
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(1.0, 0.0, 0.0),
            Point3D::new(0.0, 1.0, 0.0),
        );
        doc.add_mesh("t", &mesh, mat);
        let glb = doc.to_glb();
        // JSON chunk length sits at offset 12 (after 12-byte header).
        let json_chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(json_chunk_len % 4, 0, "JSON chunk length must align to 4");
        // BIN chunk length sits at offset 12 + 8 + json_chunk_len.
        let bin_off = 12 + 8 + json_chunk_len;
        let bin_chunk_len =
            u32::from_le_bytes(glb[bin_off..bin_off + 4].try_into().unwrap()) as usize;
        assert_eq!(bin_chunk_len % 4, 0, "BIN chunk length must align to 4");
    }

    #[test]
    fn convert_format_enum_variants_exist() {
        // Compile-time confirmation that GltfFormat has both variants.
        let _gltf = GltfFormat::Gltf;
        let _glb = GltfFormat::Glb;
    }
}
