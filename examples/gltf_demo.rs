//! Emit a tiny glTF 2.0 document to stdout for smoke-testing.
//!
//! Run with:
//!   cargo run --release --example gltf_demo

use dwg::entities::Point3D;
use dwg::geometry::Mesh;
use dwg::gltf::GltfDoc;

fn main() {
    let mut doc = GltfDoc::new("demo");
    let red = doc.add_layer_material("RED", 1);
    let mut mesh = Mesh::empty();
    mesh.push_triangle(
        Point3D::new(0.0, 0.0, 0.0),
        Point3D::new(1.0, 0.0, 0.0),
        Point3D::new(0.0, 1.0, 0.0),
    );
    doc.add_mesh("tri", &mesh, red);
    let (json, bin) = doc.finish();
    print!("{json}");
    eprintln!("\n--- bin_len={} ---", bin.len());
}
