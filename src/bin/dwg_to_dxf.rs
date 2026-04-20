//! `dwg-to-dxf` — convert a DWG file to ASCII DXF (L11-08).
//!
//! Thin CLI wrapper around [`dwg::dxf_convert::convert_file_to_dxf`].
//!
//! # Usage
//!
//! ```text
//! dwg-to-dxf INPUT.dwg                        # write DXF to stdout
//! dwg-to-dxf INPUT.dwg OUTPUT.dxf             # write DXF to OUTPUT.dxf
//! dwg-to-dxf INPUT.dwg OUTPUT.dxf --version R12
//! dwg-to-dxf INPUT.dwg --version AC1032
//! ```
//!
//! `--version` accepts either the short release name (`R12`, `R14`,
//! `R2000`, `R2004`, `R2007`, `R2010`, `R2013`, `R2018`) or the raw
//! `$ACADVER` magic (`AC1009`, `AC1014`, `AC1015`, `AC1018`, `AC1021`,
//! `AC1024`, `AC1027`, `AC1032`). Default: `R2018`.
//!
//! # Audit-honest scope
//!
//! The writer emits spec-compliant DXF group-code pairs per the public
//! AutoCAD DXF reference. Actual acceptance against AutoCAD /
//! BricsCAD / LibreCAD is **untested** (no Autodesk product in CI).
//! Automated validation is limited to `cargo test`; real
//! round-trip-via-AutoCAD validation is a manual step documented in
//! `tests/integration_dxf_roundtrip.rs`.

use clap::Parser;
use dwg::dxf::DxfVersion;
use dwg::dxf_convert::convert_file_to_dxf;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "dwg-to-dxf",
    about = "Convert a DWG file to ASCII DXF (R12..R2018)",
    version
)]
struct Args {
    /// Path to a .dwg file.
    input: PathBuf,

    /// Optional output path. If omitted, DXF is written to stdout.
    output: Option<PathBuf>,

    /// DXF target version: R12 | R14 | R2000 | R2004 | R2007 | R2010
    /// | R2013 | R2018, or the equivalent `$ACADVER` magic. Default:
    /// R2018 (AC1032).
    #[arg(long, default_value = "R2018")]
    version: String,
}

fn run(args: Args) -> anyhow::Result<()> {
    let version = DxfVersion::parse_cli(&args.version).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown --version {:?}; accepted: R12, R14, R2000, R2004, R2007, R2010, R2013, R2018 \
             (or AC1009/AC1014/AC1015/AC1018/AC1021/AC1024/AC1027/AC1032)",
            args.version
        )
    })?;

    let dxf = convert_file_to_dxf(&args.input, version)?;

    match args.output.as_deref() {
        Some(path) => {
            let mut f = File::create(path)?;
            f.write_all(dxf.as_bytes())?;
            eprintln!(
                "dwg-to-dxf: wrote {} bytes ({}) to {}",
                dxf.len(),
                version.acadver(),
                path.display()
            );
        }
        None => {
            std::io::stdout().write_all(dxf.as_bytes())?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dwg-to-dxf: {e}");
            ExitCode::FAILURE
        }
    }
}
