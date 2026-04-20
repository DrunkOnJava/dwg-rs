//! `dwg-to-gltf` — convert a DWG file to glTF 2.0 (L10-07).
//!
//! Thin CLI wrapper around [`dwg::gltf::convert_file_to_gltf`].
//!
//! # Usage
//!
//! ```text
//! dwg-to-gltf INPUT.dwg                        # write glTF JSON to stdout
//! dwg-to-gltf INPUT.dwg OUTPUT.gltf            # write JSON + sidecar OUTPUT.bin
//! dwg-to-gltf INPUT.dwg OUTPUT.glb             # write single-file binary GLB
//! ```
//!
//! Output container is selected from the OUTPUT extension:
//!
//! - `.gltf` (or any other text suffix) → glTF JSON. When the document
//!   has a non-empty binary buffer, a sibling `OUTPUT.bin` is written
//!   alongside; the JSON's `buffers[0].uri` field is left absent so
//!   downstream packagers know to wire it (or repackage as `.glb`).
//! - `.glb` → glTF Binary container per spec §4.4 (single self-
//!   contained file with header + JSON chunk + optional BIN chunk).
//!
//! With no OUTPUT, glTF JSON is written to stdout — handy for piping
//! into validators or quick inspections. The binary buffer is dropped
//! in this mode (no sidecar can be located).
//!
//! # Audit-honest scope
//!
//! Per-entity mesh emission covers LINE / CIRCLE / ARC / ELLIPSE /
//! POINT / LWPOLYLINE / 3DFACE today; SURFACE entities land as
//! placeholder bbox cubes; SPLINE / TEXT / MTEXT / INSERT / HATCH /
//! DIMENSION / MLEADER / VIEWPORT / IMAGE are silently skipped (same
//! convention as `dwg-to-dxf`'s skip-list comment). Real Three.js /
//! Blender / glTF-validator acceptance is **untested** in CI; the
//! integration test asserts spec-compliant JSON shape and GLB magic
//! only.

use clap::Parser;
use dwg::gltf::{GltfFormat, convert_file_to_gltf};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "dwg-to-gltf",
    about = "Convert a DWG file to glTF 2.0 (JSON or GLB binary)",
    version
)]
struct Args {
    /// Path to a .dwg file.
    input: PathBuf,

    /// Optional output path. If omitted, glTF JSON is written to
    /// stdout (binary buffer is dropped). Use `.gltf` for JSON +
    /// sidecar `.bin`, or `.glb` for a single binary container.
    output: Option<PathBuf>,
}

/// Decide which container format to emit based on the output path's
/// extension. `.glb` selects the single-file binary container; any
/// other extension (or none) selects JSON.
fn format_for_path(path: &std::path::Path) -> GltfFormat {
    let is_glb = path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"));
    if is_glb {
        GltfFormat::Glb
    } else {
        GltfFormat::Gltf
    }
}

fn run(args: Args) -> anyhow::Result<()> {
    match args.output.as_deref() {
        None => {
            // Stdout JSON-only mode: skip binary buffer entirely.
            let bytes = convert_file_to_gltf(&args.input, GltfFormat::Gltf)?;
            std::io::stdout().write_all(&bytes)?;
        }
        Some(path) => {
            let format = format_for_path(path);
            let bytes = convert_file_to_gltf(&args.input, format)?;
            let mut f = File::create(path)?;
            f.write_all(&bytes)?;

            match format {
                GltfFormat::Gltf => {
                    // Re-run with the doc API so we can persist the
                    // binary buffer alongside. Simpler than threading
                    // a (json, bin) tuple back from convert_file_to_gltf,
                    // and the second open is cheap because the file
                    // is already in OS page cache.
                    let (_json, bin) = build_gltf_pair(&args.input)?;
                    if !bin.is_empty() {
                        let bin_path = path.with_extension("bin");
                        let mut bf = File::create(&bin_path)?;
                        bf.write_all(&bin)?;
                        eprintln!(
                            "dwg-to-gltf: wrote {} bytes glTF JSON to {} + {} bytes sidecar {}",
                            bytes.len(),
                            path.display(),
                            bin.len(),
                            bin_path.display()
                        );
                    } else {
                        eprintln!(
                            "dwg-to-gltf: wrote {} bytes glTF JSON to {} (empty binary buffer; no sidecar)",
                            bytes.len(),
                            path.display(),
                        );
                    }
                }
                GltfFormat::Glb => {
                    eprintln!(
                        "dwg-to-gltf: wrote {} bytes GLB to {}",
                        bytes.len(),
                        path.display()
                    );
                }
            }
        }
    }
    Ok(())
}

/// Re-build the same (json, bin) pair that `convert_file_to_gltf` used
/// internally so the CLI can persist the binary sidecar. Mirrors the
/// library defaults: ACI 7 layer material, identity transform,
/// best-effort decode.
fn build_gltf_pair(path: &std::path::Path) -> anyhow::Result<(String, Vec<u8>)> {
    use dwg::DwgFile;
    use dwg::gltf::GltfDoc;

    let file = DwgFile::open(path)?;
    let mut doc = GltfDoc::new(
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("dwg-scene"),
    );
    let mat = doc.add_layer_material("layer-7", 7);
    if let Some(decoded_res) = file.decoded_entities() {
        let (decoded_list, _summary) = decoded_res?;
        for (i, entity) in decoded_list.iter().enumerate() {
            let _ = doc.add_entity_mesh(&format!("e{i}"), entity, mat);
        }
    }
    Ok(doc.finish())
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dwg-to-gltf: {e}");
            ExitCode::FAILURE
        }
    }
}
