//! `dwg-dump` — hierarchical human-readable dump of a DWG file.
//!
//! Prints version, section list, class map, summary info, app info,
//! file dependency list, handle map, and object type histogram. A
//! one-stop diagnostic tool for RE / interop / corpus exploration.
//!
//! # Usage
//!
//! ```text
//! dwg-dump [--handles] [--classes] [--metadata] [--objects] <file.dwg>
//! ```
//!
//! With no flags, prints everything. Each flag selects only the
//! corresponding section.

use dwg::DwgFile;
use std::collections::BTreeMap;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let mut show_handles = false;
    let mut show_classes = false;
    let mut show_metadata = false;
    let mut show_objects = false;
    let mut show_sections = false;
    let mut path: Option<&str> = None;
    for a in &args[1..] {
        match a.as_str() {
            "--handles" => show_handles = true,
            "--classes" => show_classes = true,
            "--metadata" => show_metadata = true,
            "--objects" => show_objects = true,
            "--sections" => show_sections = true,
            "--help" | "-h" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            p if !p.starts_with("--") => path = Some(p),
            unknown => {
                eprintln!("dwg-dump: unknown flag {unknown}");
                print_usage();
                return ExitCode::FAILURE;
            }
        }
    }
    // Default: show everything.
    if !(show_handles || show_classes || show_metadata || show_objects || show_sections) {
        show_handles = true;
        show_classes = true;
        show_metadata = true;
        show_objects = true;
        show_sections = true;
    }
    let path = match path {
        Some(p) => p,
        None => {
            print_usage();
            return ExitCode::FAILURE;
        }
    };

    let f = match DwgFile::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("dwg-dump: failed to open {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("=== dwg-dump: {path} ===");
    println!("version: {}", f.version());

    if show_sections {
        println!();
        println!("=== sections ({}) ===", f.sections().len());
        for s in f.sections() {
            println!("  {:<32} {:>10} bytes at 0x{:x}", s.name, s.size, s.offset);
        }
    }

    if show_classes {
        if let Some(Ok(classes)) = f.class_map() {
            println!();
            println!("=== classes ({}) ===", classes.classes.len());
            for c in &classes.classes {
                println!(
                    "  {:>5}: {:<30} ({})",
                    c.class_number, c.dxf_class_name, c.cpp_class_name
                );
            }
        }
    }

    if show_metadata {
        if let Some(Ok(summary)) = f.summary_info() {
            println!();
            println!("=== summary info ===");
            println!("  title:    {}", summary.title);
            println!("  subject:  {}", summary.subject);
            println!("  author:   {}", summary.author);
            println!("  keywords: {}", summary.keywords);
            println!("  comments: {}", summary.comments);
        }
        if let Some(Ok(app)) = f.app_info() {
            println!();
            println!("=== app info ===");
            println!("  name:     {}", app.name);
            println!("  version:  {}", app.version);
            println!("  product:  {}", app.product);
            println!("  comment:  {}", app.comment);
        }
        if let Some(Ok(preview)) = f.preview() {
            println!();
            println!("=== preview ===");
            println!("  overall_size: {}", preview.overall_size);
            if let Some(bmp) = preview.bmp.as_ref() {
                println!("  bmp: {} bytes", bmp.len());
            }
            if let Some(wmf) = preview.wmf.as_ref() {
                println!("  wmf: {} bytes", wmf.len());
            }
        }
        if let Some(Ok(depl)) = f.file_dep_list() {
            println!();
            println!(
                "=== file dependencies ({} features, {} files) ===",
                depl.features.len(),
                depl.files.len()
            );
            for d in &depl.files {
                let feat_name = depl
                    .features
                    .get(d.feature_index as usize)
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                println!("  [{}] {} → {}", feat_name, d.full_filename, d.found_path);
            }
        }
    }

    if show_handles {
        if let Some(Ok(hmap)) = f.handle_map() {
            println!();
            println!("=== handle map ({} entries) ===", hmap.entries.len());
            let n = hmap.entries.len();
            if n > 10 {
                for e in hmap.entries.iter().take(5) {
                    println!("  0x{:04X} → offset {}", e.handle, e.offset);
                }
                println!("  ...");
                for e in hmap.entries.iter().skip(n - 5) {
                    println!("  0x{:04X} → offset {}", e.handle, e.offset);
                }
            } else {
                for e in &hmap.entries {
                    println!("  0x{:04X} → offset {}", e.handle, e.offset);
                }
            }
        }
    }

    if show_objects {
        if let Some(Ok(objs)) = f.all_objects() {
            println!();
            println!("=== objects ({}) ===", objs.len());
            let mut histo: BTreeMap<String, usize> = BTreeMap::new();
            for o in &objs {
                let name = format!("{:?}", o.kind);
                *histo.entry(name).or_insert(0) += 1;
            }
            for (name, count) in &histo {
                println!("  {:<30} {}", name, count);
            }
        }
    }

    ExitCode::SUCCESS
}

fn print_usage() {
    println!(
        "dwg-dump [--handles] [--classes] [--metadata] [--objects] [--sections] <file.dwg>\n\
         \n\
         Dumps a human-readable view of a DWG file. With no flags, shows\n\
         everything; pass one or more flags to select subsections.\n\
         \n\
         Examples:\n\
           dwg-dump test.dwg                    # full dump\n\
           dwg-dump --classes test.dwg          # just custom classes\n\
           dwg-dump --objects --handles t.dwg   # object histogram + handle map"
    );
}
