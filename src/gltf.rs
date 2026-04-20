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
//! metallic-roughness material as `baseColorFactor` — a non-metallic,
//! fully-rough default is used so the AutoCAD flat-fill look survives
//! the round-trip into a PBR renderer.
//!
//! # Placeholder tessellation
//!
//! [`tessellate_surface_placeholder`] returns a degenerate single-
//! triangle mesh. Real SURFACE / NURBS tessellation is tracked by
//! GitHub issue #245 (SPLINE NURBS) and will replace this stub once
//! the knot-vector evaluator lands.

use crate::color::aci_to_rgb;
use crate::geometry::{Mesh, Transform3};

/// glTF 2.0 document in progress. Meshes, materials, and nodes are
/// appended with the `add_*` methods; the complete document is
/// produced by [`finish`](Self::finish).
///
/// Layout in the emitted JSON:
///
/// - `asset.version = "2.0"`
/// - `scene = 0` → `scenes[0].nodes = [0, 1, ...]` (every added node)
/// - `nodes[i]` references `meshes[i]` (one node per mesh, 1:1)
/// - `meshes[i].primitives[0]` references POSITION + indices accessors
/// - `buffers[0]` is a single blob; bufferViews slice it
/// - Two accessors per mesh: one VEC3 float POSITION, one SCALAR u32 indices
#[derive(Debug, Clone)]
pub struct GltfDoc {
    scene_name: String,
    /// One entry per registered mesh. Each carries the node + mesh JSON
    /// fragments and the binary-buffer offsets it occupies.
    meshes: Vec<MeshEntry>,
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
    material_index: usize,
    /// Optional node transform (16-element column-major f32 matrix).
    transform: Option<[f32; 16]>,
    vertex_count: usize,
    triangle_count: usize,
    /// POSITION min (for glTF accessor requirement).
    pos_min: [f32; 3],
    /// POSITION max (for glTF accessor requirement).
    pos_max: [f32; 3],
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
            materials: Vec::new(),
            bin: Vec::new(),
        }
    }

    /// Register a material with a PBR `baseColorFactor`. Returns the
    /// material index that later [`add_mesh`](Self::add_mesh) calls
    /// reference.
    ///
    /// The metallic-roughness model is forced to non-metallic + fully-
    /// rough so AutoCAD's flat-fill palette survives round-trip to a
    /// physically-based renderer. Callers that need true PBR can write
    /// a follow-up material module.
    pub fn add_material(&mut self, name: &str, base_color_rgba: [f32; 4]) -> usize {
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
    pub fn add_layer_material(&mut self, name: &str, aci: u8) -> usize {
        let (r, g, b) = aci_to_rgb(aci);
        let rgba = [
            (r as f32) / 255.0,
            (g as f32) / 255.0,
            (b as f32) / 255.0,
            1.0,
        ];
        self.add_material(name, rgba)
    }

    /// Register a mesh. Vertex positions are packed as little-endian
    /// f32 VEC3; triangle indices as little-endian u32 SCALAR. Both
    /// arrays are appended to the shared binary buffer and a bufferView
    /// is recorded for each. Returns the mesh index.
    ///
    /// The material index must come from a prior
    /// [`add_material`](Self::add_material) or
    /// [`add_layer_material`](Self::add_layer_material) call; glTF
    /// validators will reject out-of-range material references.
    pub fn add_mesh(&mut self, name: &str, mesh: &Mesh, material_index: usize) -> usize {
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
        material_index: usize,
        transform: Option<Transform3>,
    ) -> usize {
        let vertex_count = mesh.vertices.len();
        let triangle_count = mesh.triangles.len();

        // --- POSITION bufferView: float32 VEC3 ---
        let mut pos_min = [f32::INFINITY; 3];
        let mut pos_max = [f32::NEG_INFINITY; 3];
        for v in &mesh.vertices {
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
        if vertex_count == 0 {
            // glTF requires min/max on POSITION accessors but we can't
            // compute them on an empty list; fall back to zeros so the
            // emitted accessor is still a valid (if degenerate) VEC3.
            pos_min = [0.0; 3];
            pos_max = [0.0; 3];
        }

        // --- indices bufferView: u32 SCALAR ---
        for tri in &mesh.triangles {
            self.bin.extend_from_slice(&tri[0].to_le_bytes());
            self.bin.extend_from_slice(&tri[1].to_le_bytes());
            self.bin.extend_from_slice(&tri[2].to_le_bytes());
        }

        // bufferView indices are assigned implicitly: mesh N owns
        // views [N*2, N*2+1] for POSITION and indices respectively.
        let transform_m = transform.map(transform_to_f32_matrix);

        let idx = self.meshes.len();
        self.meshes.push(MeshEntry {
            name: name.to_string(),
            material_index,
            transform: transform_m,
            vertex_count,
            triangle_count,
            pos_min,
            pos_max,
        });
        idx
    }

    /// Finalize the document and return `(json_text, binary_buffer)`.
    ///
    /// The caller owns packaging: for `.gltf` + `.bin`, write both and
    /// reference the bin via `buffers[0].uri` (the emitted JSON leaves
    /// `uri` absent, so downstream code must supply it or repackage
    /// as `.glb`). For `.glb`, wrap both in the binary container per
    /// the glTF 2.0 binary chunk spec §4.4.
    pub fn finish(self) -> (String, Vec<u8>) {
        let GltfDoc {
            scene_name,
            meshes,
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
            let idx_len = m.triangle_count * 3 * 4;
            views.push(BufferView {
                byte_offset: cursor,
                byte_length: idx_len,
            });
            cursor += idx_len;
        }
        debug_assert_eq!(cursor, bin.len());

        let json = emit_json(&scene_name, &meshes, &materials, &views, bin.len());
        (json, bin)
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
    use crate::entities::Point3D;
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
// JSON emission helpers (no serde_json dependency).
// ---------------------------------------------------------------------------

fn emit_json(
    scene_name: &str,
    meshes: &[MeshEntry],
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
    for i in 0..meshes.len() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&i.to_string());
    }
    out.push_str("] } ],\n");

    // nodes
    out.push_str("  \"nodes\": [");
    for (i, m) in meshes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n    { \"mesh\": ");
        out.push_str(&i.to_string());
        if let Some(mat) = &m.transform {
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
        out.push_str(", \"mode\": 4 } ] }");
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
        out.push_str("], \"metallicFactor\": 0.0, \"roughnessFactor\": 1.0 } }");
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
        out.push_str(&(m.triangle_count * 3).to_string());
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
        assert!(json.contains("\"roughnessFactor\": 1"));
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
}
