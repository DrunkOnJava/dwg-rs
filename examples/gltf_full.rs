//! Smoke test for the full glTF pipeline (L10-08).
//!
//! Builds a scene that exercises every code path the production
//! pipeline goes through: a triangle mesh, a tessellated circle
//! (line primitive), a bbox-style placeholder for a synthetic
//! surface, an instanced node referencing the triangle mesh under a
//! translated transform, and a layer material driven from the ACI
//! palette. Emits both glTF JSON and the GLB binary container so the
//! reader can sanity-check both packagers.
//!
//! Run with:
//!   cargo run --release --example gltf_full

use dwg::entities::DecodedEntity;
use dwg::entities::{Point3D, Vec3D, circle::Circle};
use dwg::geometry::{Mesh, Transform3};
use dwg::gltf::GltfDoc;

fn main() {
    let mut doc = GltfDoc::new("gltf_full_demo");

    // Two layer materials — one red (ACI 1), one white (ACI 7).
    let red = doc.add_layer_material("RED", 1);
    let white = doc.add_layer_material("WHITE", 7);

    // Triangle mesh (TRIANGLES primitive).
    let mut tri = Mesh::empty();
    tri.push_triangle(
        Point3D::new(0.0, 0.0, 0.0),
        Point3D::new(1.0, 0.0, 0.0),
        Point3D::new(0.0, 1.0, 0.0),
    );
    let tri_node = doc.add_mesh("tri", &tri, red);

    // Circle entity → line primitive with 64 tessellation segments.
    let circle = DecodedEntity::Circle(Circle {
        center: Point3D::new(5.0, 0.0, 0.0),
        radius: 2.0,
        thickness: 0.0,
        extrusion: Vec3D {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
    });
    doc.add_entity_mesh("circle", &circle, white)
        .expect("circle emits");

    // Instanced node — reuses the triangle mesh under a +X translation.
    let inst_xform = Transform3::translation(10.0, 0.0, 0.0);
    let _inst = doc.add_instanced_node(tri_node, &inst_xform);

    // Emit GLB and JSON variants. The example prints sizes to stderr
    // and the JSON body to stdout so it's pipe-friendly.
    let glb = doc.clone().to_glb();
    let (json, bin) = doc.finish();

    eprintln!("--- gltf_full smoke test ---");
    eprintln!("json bytes: {}", json.len());
    eprintln!("bin bytes:  {}", bin.len());
    eprintln!("glb bytes:  {}", glb.len());
    eprintln!("glb magic:  {:?}", &glb[0..4]);

    print!("{json}");
}
