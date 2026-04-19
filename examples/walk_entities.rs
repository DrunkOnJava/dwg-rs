//! Walk every drawable entity in a DWG file and print a histogram of
//! entity types. Demonstrates the handle-driven object iteration API.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example walk_entities -- path/to/file.dwg
//! ```

use dwg::DwgFile;
use std::collections::BTreeMap;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: walk_entities <file.dwg>");
        return ExitCode::FAILURE;
    };

    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("failed to open {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let objects = match file.all_objects() {
        Some(Ok(list)) => list,
        Some(Err(e)) => {
            eprintln!("object walk failed: {e}");
            return ExitCode::FAILURE;
        }
        None => {
            eprintln!("file format does not support handle-driven walking");
            return ExitCode::FAILURE;
        }
    };

    let mut histo: BTreeMap<String, usize> = BTreeMap::new();
    let mut entity_count = 0usize;
    for raw in &objects {
        if raw.is_entity() {
            entity_count += 1;
        }
        *histo.entry(format!("{:?}", raw.kind)).or_insert(0) += 1;
    }

    println!("total objects: {}", objects.len());
    println!("entities:      {}", entity_count);
    println!();
    println!("by type:");
    for (name, count) in &histo {
        println!("  {:<30} {}", name, count);
    }

    ExitCode::SUCCESS
}
