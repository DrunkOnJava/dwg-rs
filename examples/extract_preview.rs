//! Extract the embedded thumbnail from a DWG file. Output is a BMP,
//! WMF, or PNG depending on what the writing application stored.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example extract_preview -- input.dwg preview.bmp
//! ```

use dwg::DwgFile;
use std::env;
use std::fs::File;
use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: extract_preview <input.dwg> <output.bmp>");
        return ExitCode::FAILURE;
    };
    let Some(output) = args.next() else {
        eprintln!("usage: extract_preview <input.dwg> <output.bmp>");
        return ExitCode::FAILURE;
    };

    let file = match DwgFile::open(&input) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("failed to open {input}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let preview = match file.preview() {
        Some(Ok(p)) => p,
        Some(Err(e)) => {
            eprintln!("preview parse failed: {e}");
            return ExitCode::FAILURE;
        }
        None => {
            eprintln!("no preview section in file");
            return ExitCode::FAILURE;
        }
    };

    let bytes = match (&preview.bmp, &preview.wmf) {
        (Some(bmp), _) => bmp.as_slice(),
        (None, Some(wmf)) => wmf.as_slice(),
        (None, None) => {
            eprintln!("preview section present but held no bitmap data");
            return ExitCode::FAILURE;
        }
    };

    match File::create(&output).and_then(|mut w| w.write_all(bytes)) {
        Ok(_) => {
            println!("wrote {} bytes → {}", bytes.len(), output);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("failed to write {output}: {e}");
            ExitCode::FAILURE
        }
    }
}
