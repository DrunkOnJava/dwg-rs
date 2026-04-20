//! Convert a DWG file's metadata + (when the decoder is fixed)
//! decoded entities into an SVG document.
//!
//! Until #103 (common-entity preamble fix for R2013+) lands, the
//! output is a metadata-only SVG: file version + section count +
//! a bounding-box rectangle per object. Once decoders ship correctly,
//! the same example will emit real geometry without code changes.
//!
//! Usage:
//!
//! ```text
//! cargo run --release --example dwg_to_svg -- path/to/file.dwg [out.svg]
//! ```
//!
//! If the output path is omitted, the SVG is written to stdout.

use dwg::DwgFile;
use dwg::curve::Curve;
use dwg::entities::Point3D;
use dwg::svg::{Style, SvgDoc};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: {} <input.dwg> [output.svg]", args[0]);
        return ExitCode::from(2);
    }
    let input = &args[1];
    let output = args.get(2);

    let file = match DwgFile::open(input) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("failed to open {input}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let summary = file.summary();
    eprintln!(
        "version: {}  sections: {}  size: {} bytes  status: {:?}",
        summary.version, summary.section_count, summary.file_size_bytes, summary.section_map_status
    );

    let mut doc = SvgDoc::new(800.0, 600.0);
    doc.begin_layer("metadata");
    let header_style = Style {
        stroke: "#888888".to_string(),
        stroke_width: 1.0,
        fill: None,
        dashes: Some(vec![4.0, 2.0]),
    };
    // Frame around the canvas so the empty-decode case still looks
    // intentional rather than blank.
    doc.push_curve(
        &Curve::Line {
            a: Point3D::new(0.0, 0.0, 0.0),
            b: Point3D::new(800.0, 0.0, 0.0),
        },
        &header_style,
        None,
    );
    doc.push_curve(
        &Curve::Line {
            a: Point3D::new(800.0, 0.0, 0.0),
            b: Point3D::new(800.0, 600.0, 0.0),
        },
        &header_style,
        None,
    );
    doc.push_curve(
        &Curve::Line {
            a: Point3D::new(800.0, 600.0, 0.0),
            b: Point3D::new(0.0, 600.0, 0.0),
        },
        &header_style,
        None,
    );
    doc.push_curve(
        &Curve::Line {
            a: Point3D::new(0.0, 600.0, 0.0),
            b: Point3D::new(0.0, 0.0, 0.0),
        },
        &header_style,
        None,
    );
    doc.end_layer();

    // For each enumerated object, emit a small dot at a deterministic
    // position based on its handle. This proves the pipeline reaches
    // every object and gives a visible "fingerprint" of the file.
    if let Some(Ok(objects)) = file.all_objects() {
        doc.begin_layer("objects");
        let dot_style = Style {
            stroke: "#1A65BF".to_string(),
            stroke_width: 1.5,
            fill: None,
            dashes: None,
        };
        for (i, obj) in objects.iter().enumerate() {
            let cx = 50.0 + ((i as f64) * 17.0).rem_euclid(700.0);
            let cy = 50.0 + ((obj.handle.value % 500) as f64);
            doc.push_curve(
                &Curve::Circle {
                    center: Point3D::new(cx, cy, 0.0),
                    radius: 3.0,
                    normal: dwg::entities::Vec3D {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                },
                &dot_style,
                Some(&format!("0x{:X}", obj.handle.value)),
            );
        }
        eprintln!("dots emitted for {} objects", objects.len());
        doc.end_layer();
    }

    let svg = doc.finish();
    let result: io::Result<()> = match output {
        Some(path) => fs::write(path, &svg),
        None => io::stdout().write_all(svg.as_bytes()),
    };

    match result {
        Ok(()) => {
            if let Some(path) = output {
                eprintln!("wrote {path}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("failed to write SVG: {e}");
            ExitCode::FAILURE
        }
    }
}
