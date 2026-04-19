//! Minimal "hello world" — open a DWG, print its version and section
//! list. Run with:
//!
//! ```bash
//! cargo run --example basic_open -- path/to/file.dwg
//! ```

use dwg::DwgFile;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: basic_open <file.dwg>");
        return ExitCode::FAILURE;
    };

    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("failed to open {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("file:     {path}");
    println!("version:  {}", file.version());
    println!("sections: {}", file.sections().len());

    for section in file.sections() {
        println!(
            "  {:<32} {:>10} bytes at 0x{:x}",
            section.name, section.size, section.offset
        );
    }

    ExitCode::SUCCESS
}
