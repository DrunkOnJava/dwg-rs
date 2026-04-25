//! `dwg-write` — CLI for the DWG write pipeline (L12-14).
//!
//! # Honest scope
//!
//! The DWG writer is a five-stage pipeline (see [`dwg::file_writer`]
//! module docs). The pipeline can now assemble a complete R2004-family
//! byte buffer that this crate's reader round-trips section-by-section.
//! External CAD acceptance is still manual: CI does not run AutoCAD,
//! BricsCAD, or LibreCAD, so `--dwg` output must be treated as
//! experimental until independent application fixtures are added.
//!
//! What this CLI does today:
//!   1. Read one or more named sections from disk (`--section NAME=PATH`).
//!   2. Feed them through [`dwg::file_writer::WriterScaffold`] to
//!      produce framed pages (32-byte aligned, Sec_Mask-masked,
//!      LZ77-compressed).
//!   3. Emit a machine-readable JSON report describing each built
//!      page: name, section number, compressed/decompressed sizes,
//!      page offset, checksum.
//!   4. Optionally write either the concatenated section pages
//!      (`--bytes`) or the assembled R2004-family DWG byte buffer
//!      (`--dwg`).
//!
//! Section names must be in [`dwg::file_writer::KNOWN_SECTION_NAMES`]
//! (the 16 ODA-spec'd `AcDb:*` names). Unknown names are rejected at
//! section-add time by [`dwg::file_writer::validate_section_name`].
//!
//! # Usage
//!
//! ```text
//! dwg-write --version R2018 \
//!   --section AcDb:Header=header.bin \
//!   --section AcDb:SummaryInfo=summary.bin \
//!   --report stage1.json \
//!   [--bytes stage-pages.bin] \
//!   [--dwg assembled.dwg]
//! ```

use clap::Parser;
use dwg::file_writer::{self, WriterScaffold, assemble_dwg_bytes};
use dwg::version::Version;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "dwg-write",
    about = "CLI for DWG write pipeline (L12-14)",
    version,
    long_about = "Builds DWG section pages and can assemble an experimental \
                  complete R2004-family DWG byte buffer with --dwg. External \
                  CAD acceptance is not yet proven in CI; see module docs in \
                  src/bin/dwg_write.rs for honest scope."
)]
struct Args {
    /// Target DWG version. One of R14, R2000, R2004, R2007, R2010,
    /// R2013, R2018. Full byte-buffer assembly currently supports
    /// the R2004 family except R2007's second Sec_Mask layer.
    /// Default: R2018.
    #[arg(long, default_value = "R2018")]
    version: String,

    /// Repeatable section argument: `NAME=PATH`. NAME must be in the
    /// set of ODA-spec'd `AcDb:*` section names.
    #[arg(long = "section", value_name = "NAME=PATH")]
    sections: Vec<String>,

    /// Optional output path for the section-page JSON report. If
    /// absent, the report is written to stdout.
    #[arg(long)]
    report: Option<PathBuf>,

    /// Optional output path for concatenated Stage-1 bytes. WARNING:
    /// this is only the section-page buffer, not a standalone DWG.
    /// Useful for round-trip testing section framing.
    #[arg(long)]
    bytes: Option<PathBuf>,

    /// Optional output path for an assembled R2004-family DWG byte
    /// buffer. This crate's reader round-trips the sections; external
    /// CAD application acceptance is not yet automated.
    #[arg(long)]
    dwg: Option<PathBuf>,
}

fn parse_version(s: &str) -> Option<Version> {
    match s.to_ascii_uppercase().as_str() {
        "R14" | "AC1014" => Some(Version::R14),
        "R2000" | "AC1015" => Some(Version::R2000),
        "R2004" | "AC1018" => Some(Version::R2004),
        "R2007" | "AC1021" => Some(Version::R2007),
        "R2010" | "AC1024" => Some(Version::R2010),
        "R2013" | "AC1027" => Some(Version::R2013),
        "R2018" | "AC1032" => Some(Version::R2018),
        _ => None,
    }
}

fn parse_section_arg(arg: &str) -> anyhow::Result<(String, PathBuf)> {
    let (name, path) = arg
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("--section expected NAME=PATH, got {arg:?}"))?;
    Ok((name.to_string(), PathBuf::from(path)))
}

fn run(args: Args) -> anyhow::Result<()> {
    let version = parse_version(&args.version).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown --version {:?}; accepted: R14, R2000, R2004, R2007, R2010, R2013, R2018",
            args.version
        )
    })?;

    if args.sections.is_empty() {
        anyhow::bail!(
            "at least one --section NAME=PATH is required. Try \
             --section AcDb:Header=/path/to/header.bin"
        );
    }

    let mut scaffold = WriterScaffold::new(version);
    for arg in &args.sections {
        let (name, path) = parse_section_arg(arg)?;
        file_writer::validate_section_name(&name).map_err(|e| anyhow::anyhow!("{e}"))?;
        let bytes =
            fs::read(&path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        scaffold.add_section(name, bytes);
    }

    let built = scaffold
        .build_sections()
        .map_err(|e| anyhow::anyhow!("build_sections: {e}"))?;

    // JSON report (hand-written, no serde_json dep needed).
    let mut report = String::from("{\n  \"stage\": 1,\n  \"version\": \"");
    report.push_str(&args.version.to_uppercase());
    report.push_str(
        "\",\n  \"note\": \"section pages built; use --dwg for assembled R2004-family bytes\",\n",
    );
    report.push_str("  \"sections\": [\n");
    for (i, b) in built.iter().enumerate() {
        report.push_str("    {");
        report.push_str(&format!(" \"name\": \"{}\",", b.name));
        report.push_str(&format!(" \"number\": {},", b.number));
        report.push_str(&format!(" \"page_offset\": {},", b.page_offset));
        report.push_str(&format!(
            " \"compressed_size\": {},",
            b.built.compressed_size
        ));
        report.push_str(&format!(
            " \"decompressed_size\": {},",
            b.built.decompressed_size
        ));
        report.push_str(&format!(" \"checksum\": {}", b.built.checksum));
        report.push_str(" }");
        if i + 1 < built.len() {
            report.push(',');
        }
        report.push('\n');
    }
    report.push_str("  ]\n}\n");

    match args.report.as_deref() {
        Some(path) => {
            file_writer::atomic_write(path, report.as_bytes())
                .map_err(|e| anyhow::anyhow!("writing report to {}: {e}", path.display()))?;
            eprintln!(
                "dwg-write: wrote {}-section page report to {}",
                built.len(),
                path.display()
            );
        }
        None => {
            print!("{report}");
        }
    }

    if let Some(bytes_path) = args.bytes.as_deref() {
        let mut concat = Vec::new();
        for b in &built {
            concat.extend_from_slice(&b.built.bytes);
        }
        file_writer::atomic_write(bytes_path, &concat)
            .map_err(|e| anyhow::anyhow!("writing bytes to {}: {e}", bytes_path.display()))?;
        eprintln!(
            "dwg-write: wrote {} Stage-1 bytes (NOT A VALID DWG) to {}",
            concat.len(),
            bytes_path.display()
        );
    }

    if let Some(dwg_path) = args.dwg.as_deref() {
        let bytes = assemble_dwg_bytes(&built, version)
            .map_err(|e| anyhow::anyhow!("assemble_dwg_bytes: {e}"))?;
        file_writer::atomic_write(dwg_path, &bytes)
            .map_err(|e| anyhow::anyhow!("writing DWG to {}: {e}", dwg_path.display()))?;
        eprintln!(
            "dwg-write: wrote {} assembled DWG bytes to {}",
            bytes.len(),
            dwg_path.display()
        );
    }

    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dwg-write: {e}");
            ExitCode::FAILURE
        }
    }
}
