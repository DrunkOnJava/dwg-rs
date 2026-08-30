//! Print the whole R2007 (`AC1021`) container: file header, page map,
//! section map, and the first bytes of every section (ODA spec §5.1-§5.4).
//!
//! # What this proves
//!
//! The R2007 container is four mechanisms stacked — Reed-Solomon
//! interleaving, a compressor that is not the R2004 one, a page map and
//! a section map — and none of them can be checked in isolation. This
//! probe prints the checkable constants at each layer so a reader can
//! see that the stack came apart correctly rather than take the section
//! bytes on trust:
//!
//! - **File header** — §5.2 says what several fields must hold
//!   ("normally 0x70", "normally 0x20", "normally 0x40", "normally
//!   0xf800", "normally 4", "normally 1", "normally 0x60100") and one
//!   field must equal the file's own byte count. The probe prints each
//!   with a `ok` / `MISMATCH` verdict.
//! - **Page map** — the accumulated offsets must be 0x20-aligned and
//!   land inside the file.
//! - **Section map** — §5.2 tabulates a `hashcode` per section name.
//!   The probe prints the decoded hash beside the tabulated one.
//! - **Sections** — `AcDb:Classes` must open with the §21 sentinel,
//!   `AcDb:AcDbObjects` with the `RL` `0x0dca`, and `AcDb:Handles`
//!   with a big-endian section size no larger than the section itself.
//!
//! ```sh
//! cargo run --release --example probe_r2007_container -- samples/line_2007.dwg
//! ```

use dwg::DwgFile;
use dwg::r2007::Container;
use std::process::ExitCode;

/// The `hashcode` values §5.2 tabulates, by section name. Sections the
/// table does not list (`AcDb:AppInfoHistory` is written by AutoCAD but
/// not by ODA) print `--`.
const SPEC_HASHCODES: &[(&str, u64)] = &[
    ("AcDb:Security", 0x4a02_04ea),
    ("AcDb:FileDepList", 0x6c42_05ca),
    ("AcDb:VBAProject", 0x586e_0544),
    ("AcDb:AppInfo", 0x3fa0_043e),
    ("AcDb:Preview", 0x40aa_0473),
    ("AcDb:SummaryInfo", 0x717a_060f),
    ("AcDb:RevHistory", 0x60a2_05b3),
    ("AcDb:AcDbObjects", 0x674c_05a9),
    ("AcDb:ObjFreeSpace", 0x77e2_061f),
    ("AcDb:Template", 0x4a14_04ce),
    ("AcDb:Handles", 0x3f6e_0450),
    ("AcDb:Classes", 0x3f54_045f),
    ("AcDb:AuxHeader", 0x54f0_050a),
    ("AcDb:Header", 0x32b8_03d9),
];

fn verdict(actual: u64, expected: u64) -> &'static str {
    if actual == expected { "ok" } else { "MISMATCH" }
}

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: probe_r2007_container <file.dwg>");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    if bytes.len() < 6 || &bytes[..6] != b"AC1021" {
        eprintln!("{path} is not an AC1021 (R2007) file");
        return ExitCode::FAILURE;
    }
    let container = match Container::parse(&bytes) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("R2007 container parse failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("== {path} ({} bytes on disk) ==", bytes.len());
    let h = &container.header;
    println!("\n-- file header (§5.2, Reed-Solomon + §5.10 decompressed) --");
    println!(
        "  header size          {:>12}   spec says 0x70            {}",
        h.header_size,
        verdict(h.header_size, 0x70)
    );
    println!(
        "  file size            {:>12}   file is {:<12}      {}",
        h.file_size,
        bytes.len(),
        verdict(h.file_size, bytes.len() as u64)
    );
    println!(
        "  unknown @0x70        {:>12}   spec says 0x20            {}",
        h.unknown_0x70,
        verdict(h.unknown_0x70, 0x20)
    );
    println!(
        "  unknown @0x78        {:>12}   spec says 0x40            {}",
        h.unknown_0x78,
        verdict(h.unknown_0x78, 0x40)
    );
    println!(
        "  unknown @0x88        {:>12}   spec says 0xf800          {}",
        h.unknown_0x88,
        verdict(h.unknown_0x88, 0xF800)
    );
    println!(
        "  unknown @0x90        {:>12}   spec says 4               {}",
        h.unknown_0x90,
        verdict(h.unknown_0x90, 4)
    );
    println!(
        "  unknown @0x98        {:>12}   spec says 1               {}",
        h.unknown_0x98,
        verdict(h.unknown_0x98, 1)
    );
    println!(
        "  stream version       {:>12}   spec says 0x60100         {}",
        h.stream_version,
        verdict(h.stream_version, 0x6_0100)
    );
    println!(
        "  pages amount {} / max id {} / sections amount {}",
        h.pages_amount, h.pages_max_id, h.sections_amount
    );
    println!(
        "  page map:    offset 0x{:X} compressed {} uncompressed {} factor {}",
        h.pages_map_offset,
        h.pages_map_size_compressed,
        h.pages_map_size_uncompressed,
        h.pages_map_correction_factor
    );
    println!(
        "  section map: id {} compressed {} uncompressed {} factor {}",
        h.sections_map_id,
        h.sections_map_size_compressed,
        h.sections_map_size_uncompressed,
        h.sections_map_correction_factor
    );

    println!("\n-- page map (§5.2) --");
    for e in &container.page_map.entries {
        let file_offset = dwg::r2007::PAGE_BASE as u64 + e.offset;
        println!(
            "  id {:>4}  size {:>8}  offset 0x{:<8X} file 0x{:<8X} {}{}",
            e.id,
            e.size,
            e.offset,
            file_offset,
            if file_offset % 0x20 == 0 {
                "aligned"
            } else {
                "UNALIGNED"
            },
            if file_offset < bytes.len() as u64 {
                ""
            } else {
                " PAST-EOF"
            },
        );
    }

    println!("\n-- section map (§5.2) --");
    println!(
        "  {:<22} {:>8} {:>8} {:>4} {:>4} {:>5}  {:<10} {:<10}",
        "name", "size", "maxsize", "enc", "code", "pages", "hashcode", "spec"
    );
    for s in &container.section_map.sections {
        let spec = SPEC_HASHCODES
            .iter()
            .find(|(n, _)| *n == s.name)
            .map(|(_, v)| format!("0x{v:08X}"))
            .unwrap_or_else(|| "--".to_string());
        println!(
            "  {:<22} {:>8} {:>8} {:>4} {:>4} {:>5}  0x{:08X} {:<10}",
            s.name,
            s.data_size,
            s.max_size,
            s.encryption,
            s.encoding,
            s.pages.len(),
            s.hash_code & 0xFFFF_FFFF,
            spec
        );
    }

    println!("\n-- section payloads --");
    let file = match DwgFile::from_bytes(bytes) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("DwgFile::from_bytes failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    for s in &container.section_map.sections {
        match file.read_section(&s.name) {
            Some(Ok(data)) => {
                let head: String = data
                    .iter()
                    .take(16)
                    .map(|b| format!("{b:02X} "))
                    .collect::<String>();
                let note = match s.name.as_str() {
                    "AcDb:Classes" => {
                        if data.len() >= 16 && data[..16] == dwg::ClassMap::SENTINEL_START {
                            "  <- §21 class sentinel"
                        } else {
                            "  <- SENTINEL MISSING"
                        }
                    }
                    "AcDb:AcDbObjects" => {
                        if data.len() >= 4 && data[..4] == [0xCA, 0x0D, 0x00, 0x00] {
                            "  <- RL 0x0dca object-stream prefix"
                        } else {
                            "  <- 0x0dca PREFIX MISSING"
                        }
                    }
                    "AcDb:Handles" => {
                        if data.len() >= 2 {
                            let first = u16::from_be_bytes([data[0], data[1]]) as usize;
                            if first + 2 <= data.len() {
                                "  <- big-endian handle-section size fits"
                            } else {
                                "  <- HANDLE SECTION SIZE OVERRUNS"
                            }
                        } else {
                            "  <- too short"
                        }
                    }
                    _ => "",
                };
                println!("  {:<22} {:>8} bytes  {head}{note}", s.name, data.len());
            }
            Some(Err(e)) => println!("  {:<22} read failed: {e}", s.name),
            None => println!("  {:<22} not readable", s.name),
        }
    }

    ExitCode::SUCCESS
}
