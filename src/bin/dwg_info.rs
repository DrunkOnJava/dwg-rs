//! `dwg-info` — command-line metadata + section inspector.
//!
//! Usage:
//! ```text
//! dwg-info file.dwg
//! dwg-info file.dwg --json
//! dwg-info file.dwg --crc
//! ```

use clap::Parser;
use dwg::{DwgFile, reader};
use serde::Serialize;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "dwg-info",
    about = "Show version + section list of a DWG file",
    version
)]
struct Args {
    /// Path to a .dwg file.
    path: PathBuf,

    /// Emit JSON instead of human-readable text.
    #[arg(long)]
    json: bool,

    /// For R2004+ files, verify the decrypted header CRC-32.
    #[arg(long)]
    crc: bool,

    /// Extract a named section's decompressed bytes and write them to
    /// the file at --out. Example: --extract AcDb:Preview --out preview.bin
    #[arg(long, value_name = "NAME")]
    extract: Option<String>,

    /// Destination path for --extract.
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,
}

#[derive(Serialize)]
struct Report<'a> {
    path: &'a str,
    file_size: u64,
    version: String,
    release: &'a str,
    year_introduced: u16,
    maint_version: u8,
    codepage: u16,
    sections: Vec<SectionReport<'a>>,
    r2004_header: Option<R2004HeaderReport>,
}

#[derive(Serialize)]
struct SectionReport<'a> {
    name: &'a str,
    kind: &'a str,
    offset: u64,
    size: u64,
    compressed: bool,
    encrypted: bool,
}

#[derive(Serialize)]
struct R2004HeaderReport {
    file_id: String,
    security_flags: u32,
    section_page_map_addr: u64,
    section_page_amount: u32,
    gap_amount: u32,
    crc32_stored: u32,
    crc32_valid: Option<bool>,
}

fn run(args: Args) -> anyhow::Result<()> {
    let file = DwgFile::open(&args.path)?;

    if let Some(section_name) = &args.extract {
        let out_path = args
            .out
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("--extract requires --out <PATH>"))?;
        let bytes = file.read_section(section_name).ok_or_else(|| {
            anyhow::anyhow!(
                "cannot extract: file is not R2004-family or section {section_name:?} absent"
            )
        })??;
        std::fs::write(out_path, &bytes)?;
        eprintln!(
            "wrote {} bytes from section {:?} to {}",
            bytes.len(),
            section_name,
            out_path.display()
        );
        return Ok(());
    }
    let sections: Vec<_> = file
        .sections()
        .iter()
        .map(|s| SectionReport {
            name: &s.name,
            kind: s.kind.short_label(),
            offset: s.offset,
            size: s.size,
            compressed: s.compressed,
            encrypted: s.encrypted,
        })
        .collect();

    let r2004_report = if let Some(h) = file.r2004_header() {
        let crc_valid = if args.crc {
            let bytes = file.raw_bytes();
            let (expected, actual) = reader::validate_r2004_header_crc(bytes)?;
            Some(expected == actual)
        } else {
            None
        };
        Some(R2004HeaderReport {
            file_id: String::from_utf8_lossy(&h.file_id)
                .trim_end_matches('\0')
                .to_owned(),
            security_flags: h.security_flags,
            section_page_map_addr: h.section_page_map_addr,
            section_page_amount: h.section_page_amount,
            gap_amount: h.gap_amount,
            crc32_stored: h.crc32_stored,
            crc32_valid: crc_valid,
        })
    } else {
        None
    };

    let version_display = file.version().to_string();
    let report = Report {
        path: args.path.to_str().unwrap_or("<non-utf8-path>"),
        file_size: file.file_size(),
        version: version_display,
        release: file.version().release(),
        year_introduced: file.version().year_introduced(),
        maint_version: file
            .r2004_header()
            .map(|h| h.common.maint_version)
            .or_else(|| file.r13_header().map(|h| h.common.maint_version))
            .unwrap_or(0),
        codepage: file
            .r2004_header()
            .map(|h| h.common.codepage)
            .or_else(|| file.r13_header().map(|h| h.common.codepage))
            .unwrap_or(0),
        sections,
        r2004_header: r2004_report,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human(&report);
    }
    Ok(())
}

fn print_human(r: &Report) {
    println!("file:            {}", r.path);
    println!("size:            {} bytes", r.file_size);
    println!("version:         {}", r.version);
    println!("release:         AutoCAD {}", r.release);
    println!("introduced:      {}", r.year_introduced);
    println!("maintenance:     {}", r.maint_version);
    println!("codepage:        {}", r.codepage);
    if let Some(h) = &r.r2004_header {
        println!("r2004 file id:   {}", h.file_id);
        println!("security flags:  {:#010x}", h.security_flags);
        println!("section pages:   {}", h.section_page_amount);
        println!("gap amount:      {}", h.gap_amount);
        println!("page map addr:   {:#x}", h.section_page_map_addr);
        println!("stored CRC-32:   {:#010x}", h.crc32_stored);
        if let Some(valid) = h.crc32_valid {
            println!("CRC-32 check:    {}", if valid { "PASS" } else { "FAIL" });
        }
    }
    println!();
    println!("sections ({}):", r.sections.len());
    println!(
        "  {:<28} {:<12} {:>10} {:>10}  flags",
        "name", "kind", "offset", "size"
    );
    for s in &r.sections {
        let flags = match (s.compressed, s.encrypted) {
            (true, true) => "C+E",
            (true, false) => "C",
            (false, true) => "E",
            (false, false) => "-",
        };
        println!(
            "  {:<28} {:<12} {:>10} {:>10}  {}",
            truncate(s.name, 28),
            s.kind,
            s.offset,
            s.size,
            flags
        );
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n - 1])
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dwg-info: {e}");
            ExitCode::FAILURE
        }
    }
}
