//! `dwg-convert` — round-trip / section-extraction tool.
//!
//! # Usage
//!
//! ```text
//! dwg-convert --extract SECTION -o OUTPUT INPUT.dwg
//! dwg-convert --verify INPUT.dwg
//! ```
//!
//! Two modes:
//!
//! - **Extract** — decompresses and writes a single section's
//!   contents to `OUTPUT`. Common names: `AcDb:Preview`,
//!   `AcDb:SummaryInfo`, `AcDb:Header`, `AcDb:Classes`,
//!   `AcDb:Handles`, `AcDb:AcDbObjects`.
//! - **Verify** — opens the file, reads every section, reports
//!   pass/fail per section without writing anything.
//!
//! # Future
//!
//! A full DWG *writer* (round-trip Read → decode → Encode → Write)
//! is deferred to Phase H (the section encoder + system header
//! writer; currently only the bit-writer primitives and LZ77
//! encoder are shipped).

use dwg::DwgFile;
use std::env;
use std::fs::File;
use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let mut mode: Option<Mode> = None;
    let mut section: Option<String> = None;
    let mut output: Option<String> = None;
    let mut input: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--extract" => {
                mode = Some(Mode::Extract);
                if i + 1 >= args.len() {
                    eprintln!("--extract requires a SECTION argument");
                    return ExitCode::FAILURE;
                }
                section = Some(args[i + 1].clone());
                i += 2;
            }
            "--verify" => {
                mode = Some(Mode::Verify);
                i += 1;
            }
            "-o" => {
                if i + 1 >= args.len() {
                    eprintln!("-o requires a path argument");
                    return ExitCode::FAILURE;
                }
                output = Some(args[i + 1].clone());
                i += 2;
            }
            "--help" | "-h" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            p if !p.starts_with("--") => {
                if let Some(prev) = &input {
                    eprintln!("multiple inputs not supported; got {} and {}", prev, p);
                    return ExitCode::FAILURE;
                }
                input = Some(p.to_string());
                i += 1;
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_usage();
                return ExitCode::FAILURE;
            }
        }
    }

    let mode = match mode {
        Some(m) => m,
        None => {
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let input = match input {
        Some(i) => i,
        None => {
            print_usage();
            return ExitCode::FAILURE;
        }
    };

    let f = match DwgFile::open(&input) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("dwg-convert: failed to open {input}: {e}");
            return ExitCode::FAILURE;
        }
    };

    match mode {
        Mode::Extract => {
            let section = section.expect("extract implies a section");
            let output = match output {
                Some(o) => o,
                None => {
                    eprintln!("--extract requires -o OUTPUT");
                    return ExitCode::FAILURE;
                }
            };
            match f.read_section(&section) {
                Some(Ok(bytes)) => {
                    match File::create(&output).and_then(|mut w| w.write_all(&bytes)) {
                        Ok(_) => {
                            println!(
                                "dwg-convert: extracted {} ({} bytes) → {}",
                                section,
                                bytes.len(),
                                output
                            );
                            ExitCode::SUCCESS
                        }
                        Err(e) => {
                            eprintln!("dwg-convert: failed to write {output}: {e}");
                            ExitCode::FAILURE
                        }
                    }
                }
                Some(Err(e)) => {
                    eprintln!("dwg-convert: section {section} read failed: {e}");
                    ExitCode::FAILURE
                }
                None => {
                    eprintln!("dwg-convert: section {section} not found");
                    eprintln!("available sections:");
                    for s in f.sections() {
                        eprintln!("  {}", s.name);
                    }
                    ExitCode::FAILURE
                }
            }
        }
        Mode::Verify => {
            println!("dwg-convert: verifying {}", input);
            println!("  version:  {}", f.version());
            let mut ok = 0usize;
            let mut fail = 0usize;
            let names: Vec<String> = f.sections().iter().map(|s| s.name.clone()).collect();
            for name in &names {
                match f.read_section(name) {
                    Some(Ok(bytes)) => {
                        println!("  [ok]   {:<32} {} bytes", name, bytes.len());
                        ok += 1;
                    }
                    Some(Err(e)) => {
                        println!("  [fail] {:<32} {}", name, e);
                        fail += 1;
                    }
                    None => {
                        println!("  [skip] {:<32} not readable", name);
                    }
                }
            }
            println!("\n{} ok, {} failed out of {}", ok, fail, ok + fail);
            if fail == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

enum Mode {
    Extract,
    Verify,
}

fn print_usage() {
    println!(
        "dwg-convert --extract SECTION -o OUTPUT INPUT.dwg\n\
         dwg-convert --verify INPUT.dwg\n\
         \n\
         Modes:\n\
           --extract SECTION -o OUTPUT  write a section's decompressed\n\
                                        bytes to OUTPUT\n\
           --verify                     open file, decompress every\n\
                                        section, report per-section\n\
                                        pass/fail\n\
         \n\
         Examples:\n\
           dwg-convert --extract AcDb:Preview -o preview.bmp t.dwg\n\
           dwg-convert --verify sample.dwg"
    );
}
