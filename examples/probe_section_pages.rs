//! Per-page audit of an R2004-family section's page assembly.
//!
//! Prints, for one named section (default `AcDb:AcDbObjects`), the
//! section description fields, then one row per page entry showing the
//! section-info page reference, the matching global page-map row, the
//! decrypted 0x20-byte data-page header (spec §4.6), the number of
//! bytes LZ77 actually produced, and the running high-water mark of
//! decompressed coverage.
//!
//! It also reads `AcDb:Handles` and reports the highest offset the
//! handle map addresses, so a mismatch between "what the map points at"
//! and "what the section assembles to" is visible in one place.
//!
//! ```bash
//! cargo run --release --example probe_section_pages -- file.dwg [SectionName]
//! ```

use dwg::error::Result;
use dwg::section_map::{self, DataPageHeader};
use dwg::{DwgFile, HandleMap};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_section_pages <file.dwg> [SectionName]");
        return ExitCode::FAILURE;
    };
    let want = args
        .next()
        .unwrap_or_else(|| "AcDb:AcDbObjects".to_string());
    match run(&path, &want) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("probe failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str, want: &str) -> Result<()> {
    let file = DwgFile::open(path)?;
    let bytes = file.raw_bytes();
    println!("file      : {path}");
    println!("version   : {}", file.version());
    println!("file bytes: {}", bytes.len());

    let Some(header) = file.r2004_header() else {
        println!("not an R2004-family file — nothing to probe");
        return Ok(());
    };

    let page_map = section_map::parse_page_map(bytes, header)?;
    let descriptions = section_map::parse_section_info(bytes, header, &page_map)?;
    println!(
        "page map  : {} entries ({} gaps)",
        page_map.len(),
        page_map.iter().filter(|p| p.is_gap).count()
    );
    println!("sections  : {}", descriptions.len());
    println!();

    println!(
        "{:<26} {:>12} {:>7} {:>10} {:>5} {:>5} {:>5}",
        "section", "size", "pages", "maxdecomp", "unk", "comp", "encr"
    );
    for d in &descriptions {
        println!(
            "{:<26} {:>12} {:>7} {:>10} {:>5} {:>5} {:>5}",
            d.name,
            d.size,
            d.page_count,
            d.max_decomp_page_size,
            d.unknown,
            d.compressed,
            d.encrypted
        );
    }
    println!();

    let Some(desc) = descriptions.iter().find(|d| d.name == want) else {
        println!("section {want:?} not present");
        return Ok(());
    };

    println!("=== {} ===", desc.name);
    println!("declared decompressed size : {}", desc.size);
    println!("declared page count        : {}", desc.page_count);
    println!("page refs present          : {}", desc.pages.len());
    println!("max_decomp_page_size       : {}", desc.max_decomp_page_size);
    println!("compressed flag            : {}", desc.compressed);
    println!("encrypted flag             : {}", desc.encrypted);
    println!("unknown field              : {}", desc.unknown);
    println!();

    println!(
        "{:>4} {:>6} {:>10} {:>12} {:>12} {:>10} {:>10} {:>10} {:>12} {:>10}",
        "idx",
        "page#",
        "map size",
        "file off",
        "ref start",
        "ref dsize",
        "hdr dsize",
        "hdr psize",
        "hdr start",
        "produced"
    );

    let mut sorted: Vec<_> = desc.pages.iter().enumerate().collect();
    sorted.sort_by_key(|(_, p)| p.start_offset);
    let mut high_water: u64 = 0;
    let mut gaps: Vec<(u64, u64)> = Vec::new();
    let mut total_produced: u64 = 0;

    for (idx, page_ref) in &sorted {
        let page = page_map
            .iter()
            .find(|p| !p.is_gap && p.number == page_ref.page_number as i32);
        let (map_size, file_off) = match page {
            Some(p) => (p.size, p.file_offset),
            None => {
                println!(
                    "{:>4} {:>6} {:>10} {:>12} {:>12} {:>10} {:>10} {:>10} {:>12} {:>10}",
                    idx,
                    page_ref.page_number,
                    "MISSING",
                    "-",
                    page_ref.start_offset,
                    page_ref.data_size,
                    "-",
                    "-",
                    "-",
                    "-"
                );
                continue;
            }
        };
        let hdr = DataPageHeader::parse(bytes, file_off)?;
        let payload_start = (file_off + 0x20) as usize;
        let payload_end = payload_start + hdr.data_size as usize;
        let produced = if payload_end > bytes.len() {
            None
        } else {
            let payload = &bytes[payload_start..payload_end];
            match desc.compressed {
                1 => Some(payload.len()),
                _ => dwg::lz77::decompress(payload, Some(desc.max_decomp_page_size as usize))
                    .map(|v| v.len())
                    .ok(),
            }
        };
        println!(
            "{:>4} {:>6} {:>10} {:>12} {:>12} {:>10} {:>10} {:>10} {:>12} {:>10}",
            idx,
            page_ref.page_number,
            map_size,
            file_off,
            page_ref.start_offset,
            page_ref.data_size,
            hdr.data_size,
            hdr.page_size,
            hdr.start_offset,
            produced
                .map(|p| p.to_string())
                .unwrap_or_else(|| "ERR".to_string())
        );
        if let Some(p) = produced {
            total_produced += p as u64;
            if page_ref.start_offset > high_water {
                gaps.push((high_water, page_ref.start_offset));
            }
            let end = page_ref.start_offset + p as u64;
            if end > high_water {
                high_water = end;
            }
        }
    }

    println!();
    println!("sum of produced page bytes : {total_produced}");
    println!("assembled high-water mark  : {high_water}");
    println!("declared section size      : {}", desc.size);
    if !gaps.is_empty() {
        println!("coverage gaps              : {gaps:?}");
    }

    // Cross-check against the handle map's address space.
    if let Some(Ok(hbytes)) = file.read_section("AcDb:Handles") {
        let map = HandleMap::parse(&hbytes)?;
        let max = map.iter().map(|e| e.offset).max().unwrap_or(0);
        let past = map.iter().filter(|e| e.offset >= desc.size).count();
        println!();
        println!("handle map entries         : {}", map.len());
        println!("handle map max offset      : {max}");
        println!("entries at/past decl size  : {past}");
        if max >= desc.size {
            println!("SHORTFALL                  : {}", max - desc.size + 1);
        }
    }

    Ok(())
}
