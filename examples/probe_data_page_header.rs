//! Measure the §4.6 data page header of R2004-family drawings (#205).
//!
//! # What this proves
//!
//! `section_map::DataPageHeader` names eight 4-byte fields after the §4.6
//! table. For every data page of every R2004-family drawing given, this
//! probe unmasks the 0x20 header (Sec_Mask, `0x4164536B` ^ the page's file
//! offset, word by word) and counts how many pages agree with each reading
//! of it, against the page map (§4.4) and section map (§4.5):
//!
//! - the layout the streaming walker verifies: `0x00` type, `0x04` section
//!   number, `0x08` data size (the stored bytes after the header, equal to
//!   the section map's reference), `0x0C` page size (equal to the page map's
//!   size for the page, not the decompressed size), `0x10` start offset
//!   (8 bytes, equal to the reference's), `0x18` header checksum and `0x1C`
//!   data checksum;
//! - the data checksum as §4.2 over the stored bytes with seed 0, and the
//!   header checksum as §4.2 over the header with its own field zeroed,
//!   seeded with the data checksum;
//! - the readings the legacy `DataPageHeader` labels imply, which should
//!   agree with no page: a 4-byte start offset with a checksum at `0x14`,
//!   a data checksum at `0x18`, and a page size that is the decompressed
//!   size.
//!
//! It also counts the page-length facts the streaming walker's section
//! assembly relies on (a page's start offset is its index times the
//! section's page size; a compressed page decodes to a whole page or to
//! exactly the bytes the section has left; a stored page is padded to a
//! whole page), and sections that declare bytes but list no page.
//!
//! # How to verify
//!
//! ```sh
//! cargo run --release --example probe_data_page_header -- [--strict] <file-or-dir> ...
//! ```
//!
//! Directories are searched recursively for `.dwg` files; with no argument
//! it reads `../../samples`. Files that are not R2004-family are skipped;
//! with no such file it prints `NOT MEASURED` and exits 0. With `--strict` a
//! page that disagrees with the measured layout exits 1.

use dwg::cipher::section_page_mask;
use dwg::crc::section_page_checksum;
use dwg::header::R2004Header;
use dwg::section_map::{self, SectionPage};
use dwg::{DwgFile, lz77};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const PAGE_TYPE: u32 = 0x4163_043B;

#[derive(Default)]
struct Tally {
    pages: usize,
    layout: [usize; 7],
    legacy_labels: [usize; 3],
    starts_at_index: usize,
    compressed: usize,
    whole_page: usize,
    exact_remainder: usize,
    neither: usize,
    stored: usize,
    stored_whole_page: usize,
    sections: usize,
    bytes_without_pages: usize,
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for entry in entries {
            files(&entry, out);
        }
    } else if path.extension().is_some_and(|e| e == "dwg") {
        out.push(path.to_path_buf());
    }
}

fn measure(bytes: &[u8], tally: &mut Tally) -> Option<()> {
    let header = R2004Header::parse(bytes).ok()?;
    let pages = section_map::parse_page_map(bytes, &header).ok()?;
    let sections = section_map::parse_section_info(bytes, &header, &pages).ok()?;
    for section in &sections {
        tally.sections += 1;
        tally.bytes_without_pages += usize::from(section.size > 0 && section.pages.is_empty());
        for (index, reference) in section.pages.iter().enumerate() {
            let page: &SectionPage = pages
                .iter()
                .find(|p| !p.is_gap && p.number == reference.page_number as i32)?;
            let at = page.file_offset as usize;
            let mask = section_page_mask(at as u32);
            let mut head = [0u8; 0x20];
            for (i, chunk) in bytes.get(at..at + 0x20)?.chunks_exact(4).enumerate() {
                let plain = word(chunk, 0) ^ mask;
                head[i * 4..i * 4 + 4].copy_from_slice(&plain.to_le_bytes());
            }
            let data_size = word(&head, 8) as usize;
            let stored = bytes.get(at + 0x20..at + 0x20 + data_size)?;
            let start = u64::from(word(&head, 0x10)) | u64::from(word(&head, 0x14)) << 32;
            let mut zeroed = head;
            zeroed[0x18..0x1c].fill(0);
            tally.pages += 1;
            let checks = [
                word(&head, 0) == PAGE_TYPE,
                word(&head, 4) == section.section_id,
                data_size as u32 == reference.data_size,
                word(&head, 0x0c) == page.size,
                start == reference.start_offset,
                section_page_checksum(0, stored) == word(&head, 0x1c),
                section_page_checksum(word(&head, 0x1c), &zeroed) == word(&head, 0x18),
            ];
            for (slot, ok) in tally.layout.iter_mut().zip(checks) {
                *slot += usize::from(ok);
            }
            let room = section.size.saturating_sub(reference.start_offset);
            let page_size = u64::from(section.max_decomp_page_size);
            tally.starts_at_index +=
                usize::from(reference.start_offset == index as u64 * page_size);
            let decompressed = if section.compressed == 2 {
                lz77::decompress(stored, None).ok().map(|d| d.len() as u32)
            } else {
                Some(data_size as u32)
            };
            if let (2, Some(length)) = (section.compressed, decompressed) {
                tally.compressed += 1;
                tally.whole_page += usize::from(u64::from(length) == page_size);
                tally.exact_remainder += usize::from(u64::from(length) == room);
                tally.neither +=
                    usize::from(u64::from(length) != page_size && u64::from(length) != room);
            } else if section.compressed == 1 {
                tally.stored += 1;
                tally.stored_whole_page += usize::from(data_size as u64 == page_size);
            }
            let legacy = [
                word(&head, 0x14) == section_page_checksum(0, &zeroed),
                word(&head, 0x18) == section_page_checksum(0, stored),
                decompressed == Some(word(&head, 0x0c)),
            ];
            for (slot, ok) in tally.legacy_labels.iter_mut().zip(legacy) {
                *slot += usize::from(ok);
            }
        }
    }
    Some(())
}

fn main() -> ExitCode {
    let mut strict = false;
    let mut paths = Vec::new();
    for arg in std::env::args().skip(1) {
        if arg == "--strict" {
            strict = true;
        } else {
            files(Path::new(&arg), &mut paths);
        }
    }
    if paths.is_empty() {
        files(Path::new("../../samples"), &mut paths);
    }
    let mut tally = Tally::default();
    let mut drawings = 0;
    for path in &paths {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(file) = DwgFile::from_bytes(bytes.clone()) else {
            continue;
        };
        if !file.version().is_r2004_family() {
            continue;
        }
        if measure(&bytes, &mut tally).is_some() {
            drawings += 1;
        }
    }
    if tally.pages == 0 {
        println!("NOT MEASURED: no R2004-family drawing with data pages");
        return ExitCode::SUCCESS;
    }
    let n = tally.pages;
    println!("{drawings} drawings, {n} data pages");
    let layout = [
        "type 0x4163043B at 0x00",
        "section number at 0x04 = the section's id",
        "data size at 0x08 = the section map's reference",
        "page size at 0x0C = the page map's size for the page",
        "8-byte start offset at 0x10 = the reference's",
        "data checksum at 0x1C = §4.2 over the stored bytes, seed 0",
        "header checksum at 0x18 = §4.2 over the header, its field zeroed, seeded with 0x1C",
    ];
    for (label, count) in layout.iter().zip(tally.layout) {
        println!("{count:>6} / {n}  {label}");
    }
    let legacy = [
        "legacy label: checksum at 0x14 = §4.2 over the header, seed 0",
        "legacy label: data checksum at 0x18 = §4.2 over the stored bytes, seed 0",
        "legacy label: page size at 0x0C = the decompressed size",
    ];
    for (label, count) in legacy.iter().zip(tally.legacy_labels) {
        println!("{count:>6} / {n}  {label}");
    }
    println!(
        "{:>6} / {n}  start offset = the page's index in its section x the section's page size",
        tally.starts_at_index
    );
    println!(
        "{:>6} / {}  compressed pages decode to a whole page; {} to exactly the bytes the section has left; {} to neither",
        tally.whole_page, tally.compressed, tally.exact_remainder, tally.neither
    );
    println!(
        "{:>6} / {}  stored pages are padded to a whole page",
        tally.stored_whole_page, tally.stored
    );
    println!(
        "{:>6} / {}  sections declare bytes but list no page",
        tally.bytes_without_pages, tally.sections
    );
    if strict && (tally.layout.iter().any(|&c| c != n) || tally.neither > 0) {
        println!("FAILED: a data page disagrees with the measured layout");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
