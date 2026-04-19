//! Print a DWG file's human-readable metadata: summary info (title,
//! author, comments), application info (writer / version / product),
//! and the file-dependency list.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example dump_metadata -- path/to/file.dwg
//! ```

use dwg::DwgFile;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: dump_metadata <file.dwg>");
        return ExitCode::FAILURE;
    };

    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("failed to open {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("file:    {path}");
    println!("version: {}", file.version());

    if let Some(Ok(summary)) = file.summary_info() {
        println!("\n[Summary Info]");
        println!("  title:    {}", summary.title);
        println!("  subject:  {}", summary.subject);
        println!("  author:   {}", summary.author);
        println!("  keywords: {}", summary.keywords);
        println!("  comments: {}", summary.comments);
    }

    if let Some(Ok(app)) = file.app_info() {
        println!("\n[App Info]");
        println!("  name:    {}", app.name);
        println!("  version: {}", app.version);
        println!("  product: {}", app.product);
    }

    if let Some(Ok(deps)) = file.file_dep_list() {
        println!("\n[File Dependencies]");
        println!("  features: {}", deps.features.len());
        println!("  files:    {}", deps.files.len());
        for dep in &deps.files {
            let feat = deps
                .features
                .get(dep.feature_index as usize)
                .map(String::as_str)
                .unwrap_or("?");
            println!("    [{feat}] {} → {}", dep.full_filename, dep.found_path);
        }
    }

    ExitCode::SUCCESS
}
