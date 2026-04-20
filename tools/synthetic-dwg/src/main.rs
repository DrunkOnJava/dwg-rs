//! `synthetic-dwg` — build a minimal valid DWG file from scratch.
//!
//! Useful as a test-fixture generator for unit tests and fuzz seeds
//! without requiring a real AutoCAD / Autodesk-produced corpus file
//! on disk. Produces a file that [`dwg::DwgFile::open`] accepts — it
//! is NOT guaranteed to open in AutoCAD.
//!
//! # Chosen version: R14 (AC1014)
//!
//! The binary emits an **R14 (AC1014)** file, the simplest of the
//! supported version families. R2004+ would require a correctly-
//! masked R2004 header block, a Section Page Map with valid page
//! numbers, LZ77-framed section payloads, and CRC-32 over the
//! decrypted header — all doable but ~10× the code for no extra
//! test coverage. R2007 has its own layout and isn't supported by
//! the reader yet anyway. R14's flat locator table matches what the
//! reader's `R13R15Header::parse` consumes (§3.2.6): magic + common
//! prefix + u32 count + N × 9-byte locator records.
//!
//! # Layout emitted
//!
//! ```text
//! 0x00  "AC1014"              magic (6 bytes)
//! 0x06  00 00 00 00 00        zero padding
//! 0x0B  maint_version (1)     arbitrary (0)
//! 0x0C  00
//! 0x0D  image_seeker u32      0 (no preview)
//! 0x11  00 00                 reserved
//! 0x13  codepage u16          30 (ANSI_1252, the common default)
//! 0x15  locator_count u32     5
//! 0x19  locator records       5 × 9 bytes
//! ...   a small "fake LINE entity" payload stored at the
//!       offset the OBJECT_MAP locator points at. The byte shape
//!       matches the entity-stream preamble expected by the LINE
//!       decoder (zflag=2D, start=(0,0,0), end=(100,100,0)).
//! ```

use dwg::DwgFile;
use dwg::bitwriter::BitWriter;
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

const MAGIC: &[u8; 6] = b"AC1014";

fn print_help() {
    println!(
        "synthetic-dwg — build a minimal valid DWG file\n\
         \n\
         USAGE:\n\
         \x20   synthetic-dwg <OUT_PATH> [--read-back]\n\
         \n\
         ARGS:\n\
         \x20   <OUT_PATH>    where to write the generated .dwg file\n\
         \n\
         FLAGS:\n\
         \x20   --read-back   after writing, re-open the file via DwgFile::open\n\
         \x20                 and print the parsed metadata, to verify round-trip\n\
         \x20   -h, --help    show this message\n\
         \n\
         VERSION EMITTED:\n\
         \x20   AC1014 (AutoCAD R14 / 1997). Chosen for minimal layout —\n\
         \x20   R2004+ files require encrypted headers + LZ77 section\n\
         \x20   frames which aren't needed for a test fixture.\n\
         \n\
         NOTE:\n\
         \x20   The output is a test fixture — it satisfies dwg-rs's reader\n\
         \x20   but is NOT guaranteed to open in AutoCAD.\n"
    );
}

/// Build a synthetic R14 DWG file in memory. Returns the raw bytes.
fn build_minimal_r14() -> Vec<u8> {
    // --- Fake LINE entity payload (matches entities/line.rs::decode) ---
    //
    // zflag=true (2D), start.x=0, dx=100 (so end.x=100),
    // start.y=0, dy=100 (end.y=100), thickness=default, extrusion=default.
    let mut w = BitWriter::new();
    w.write_b(true); // zflag: 2D line
    w.write_rd(0.0); // start.x
    w.write_bd(100.0); // delta to end.x (=> end.x=100)
    w.write_rd(0.0); // start.y
    w.write_bd(100.0); // delta to end.y (=> end.y=100)
    w.write_b(true); // thickness: default (0.0)
    w.write_b(true); // extrusion: default (0,0,1)
    let line_payload = w.into_bytes();

    // --- Compute file layout offsets ---
    //
    // Header prefix is 0x19 bytes, then 5 × 9-byte locator records =
    // 45 bytes, ending at 0x19 + 45 = 0x46.
    const HEADER_END: usize = 0x19 + 5 * 9; // 0x46
    let object_map_offset: u32 = HEADER_END as u32; // 0x46
    let object_map_size: u32 = line_payload.len() as u32;

    // --- Build the 0x19-byte header prefix ---
    let mut bytes = Vec::with_capacity(256);
    // 0x00: magic
    bytes.extend_from_slice(MAGIC);
    // 0x06-0x0A: zero padding (5 bytes)
    bytes.extend_from_slice(&[0u8; 5]);
    // 0x0B: maint_version
    bytes.push(0x00);
    // 0x0C: zero
    bytes.push(0x00);
    // 0x0D-0x10: image_seeker u32 (little-endian) — 0, no preview
    bytes.extend_from_slice(&0u32.to_le_bytes());
    // 0x11-0x12: reserved
    bytes.extend_from_slice(&[0u8; 2]);
    // 0x13-0x14: codepage u16 — 30 = ANSI_1252 (common default)
    bytes.extend_from_slice(&30u16.to_le_bytes());
    debug_assert_eq!(bytes.len(), 0x15);

    // 0x15-0x18: locator_count u32 = 5
    bytes.extend_from_slice(&5u32.to_le_bytes());
    debug_assert_eq!(bytes.len(), 0x19);

    // 0x19+: 5 × 9-byte locator records. Record format:
    //   u8 number | u32 seeker | u32 size    (9 bytes total)
    // Records 0-4 are HEADER, CLASSES, OBJECT_MAP, UNKNOWN_C3, MEASUREMENT.
    for i in 0u8..5 {
        let (seeker, size): (u32, u32) = match i {
            // Point OBJECT_MAP (record 2) at the line entity payload we
            // append after the header. The other records are stubs with
            // zero size — valid for the parser, which doesn't traverse
            // the pointed-at byte range during open.
            2 => (object_map_offset, object_map_size),
            _ => (0, 0),
        };
        bytes.push(i);
        bytes.extend_from_slice(&seeker.to_le_bytes());
        bytes.extend_from_slice(&size.to_le_bytes());
    }
    debug_assert_eq!(bytes.len(), HEADER_END);

    // --- Append the fake entity payload + trailing CRC sentinel ---
    bytes.extend_from_slice(&line_payload);
    // Trailing 4-byte zero "CRC sentinel". R14 stores a per-section
    // CRC-8 + a file-level CRC-32, neither of which the reader
    // verifies at open time (the parser only uses them for strict
    // validation paths). Write zeros so file size looks plausible.
    bytes.extend_from_slice(&[0u8; 4]);

    bytes
}

fn run(out_path: PathBuf, read_back: bool) -> Result<(), Box<dyn Error>> {
    let bytes = build_minimal_r14();
    fs::write(&out_path, &bytes)?;
    eprintln!(
        "synthetic-dwg: wrote {} bytes to {}",
        bytes.len(),
        out_path.display()
    );

    if read_back {
        let file = DwgFile::open(&out_path)
            .map_err(|e| format!("read-back failed: DwgFile::open errored: {e}"))?;
        println!();
        println!("round-trip OK:");
        println!("  version:       {}", file.version());
        println!("  release:       AutoCAD {}", file.version().release());
        println!("  file_size:     {} bytes", file.file_size());
        println!("  section_count: {}", file.sections().len());
        for s in file.sections() {
            println!(
                "    - {:<18} kind={:<12} offset={:>6} size={:>6}",
                s.name,
                s.kind.short_label(),
                s.offset,
                s.size
            );
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    // Minimal arg parsing — we intentionally don't depend on clap so
    // this binary stays tiny and doesn't drag in the dwg crate's
    // `cli` feature set.
    let args = env::args().skip(1);
    let mut out_path: Option<PathBuf> = None;
    let mut read_back = false;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            "--read-back" => read_back = true,
            other if other.starts_with("--") => {
                eprintln!("synthetic-dwg: unknown flag: {other}");
                print_help();
                return ExitCode::FAILURE;
            }
            other => {
                if out_path.is_some() {
                    eprintln!("synthetic-dwg: unexpected extra arg: {other}");
                    return ExitCode::FAILURE;
                }
                out_path = Some(PathBuf::from(other));
            }
        }
    }
    let Some(out_path) = out_path else {
        eprintln!("synthetic-dwg: missing <OUT_PATH>");
        print_help();
        return ExitCode::FAILURE;
    };
    match run(out_path, read_back) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("synthetic-dwg: {e}");
            ExitCode::FAILURE
        }
    }
}
