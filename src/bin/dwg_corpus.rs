//! `dwg-corpus` — sweep a directory of .dwg files, print a summary line per
//! file (version, size, section count), and exit non-zero if any file fails
//! to open. Useful as a smoke test against large sample corpora.

use clap::Parser;
use dwg::DwgFile;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "dwg-corpus",
    about = "Sweep a directory of .dwg files",
    version
)]
struct Args {
    /// Directory to scan (recursively). If a single .dwg file is passed,
    /// it's treated as a one-element corpus.
    path: PathBuf,

    /// Fail the whole run if any file errors. By default we just log and
    /// continue.
    #[arg(long)]
    strict: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let mut files = Vec::new();
    gather(&args.path, &mut files);
    files.sort();

    if files.is_empty() {
        eprintln!("dwg-corpus: no .dwg files under {}", args.path.display());
        return ExitCode::FAILURE;
    }

    let mut ok = 0usize;
    let mut failed = 0usize;
    println!(
        "{:<48} {:<10} {:>10} {:>6}  maint",
        "file", "version", "size", "secs"
    );
    for f in &files {
        match DwgFile::open(f) {
            Ok(d) => {
                ok += 1;
                let maint = d
                    .r2004_header()
                    .map(|h| h.common.maint_version)
                    .or_else(|| d.r13_header().map(|h| h.common.maint_version))
                    .unwrap_or(0);
                println!(
                    "{:<48} {:<10} {:>10} {:>6}  {}",
                    f.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                    d.version().release(),
                    d.file_size(),
                    d.sections().len(),
                    maint
                );
            }
            Err(e) => {
                failed += 1;
                println!(
                    "{:<48} ERROR      {:>10} {:>6}  {}",
                    f.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                    "-",
                    "-",
                    e
                );
            }
        }
    }
    println!();
    println!("corpus: {ok} ok, {failed} failed ({} files)", files.len());
    if failed > 0 && args.strict {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn gather(path: &PathBuf, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|s| s.to_str()) == Some("dwg") {
            out.push(path.clone());
        }
        return;
    }
    if let Ok(rd) = std::fs::read_dir(path) {
        for entry in rd.flatten() {
            gather(&entry.path(), out);
        }
    }
}
