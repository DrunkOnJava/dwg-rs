//! Debug helper: try the full Phase B section-map walk and print step
//! results, including the first error encountered.

use dwg::header::R2004Header;
use dwg::lz77;
use dwg::section_map;
use std::env;

fn hex_dump(label: &str, data: &[u8], max: usize) {
    let n = data.len().min(max);
    eprintln!("[{label}] {n} of {} bytes:", data.len());
    for (i, chunk) in data[..n].chunks(16).enumerate() {
        eprint!("  {:04x}:", i * 16);
        for b in chunk {
            eprint!(" {:02x}", b);
        }
        eprintln!();
    }
}

fn main() -> anyhow::Result<()> {
    let path = env::args().nth(1).expect("usage: debug_section_map <file.dwg>");
    let bytes = std::fs::read(&path)?;

    let header = R2004Header::parse(&bytes)?;
    println!(
        "[hdr] section_page_map_addr=0x{:x} section_map_id={} section_page_amount={}",
        header.section_page_map_addr, header.section_map_id, header.section_page_amount
    );
    println!("[hdr] last_section_page_id={} last_section_page_end=0x{:x}",
        header.last_section_page_id, header.last_section_page_end);

    // Dump the raw and decompressed page-map payload for eyeballing.
    let page_offset = (header.section_page_map_addr + 0x100) as usize;
    // Header is 20 bytes; read comp_size from bytes 8-11.
    let comp_size = u32::from_le_bytes(
        bytes[page_offset + 8..page_offset + 12]
            .try_into()
            .unwrap(),
    ) as usize;
    let decomp_size = u32::from_le_bytes(
        bytes[page_offset + 4..page_offset + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    println!("[raw] comp_size={comp_size} decomp_size={decomp_size}");
    let raw_payload = &bytes[page_offset + 0x14..page_offset + 0x14 + comp_size];
    hex_dump("raw", raw_payload, 64);
    if let Ok(dec) = lz77::decompress(raw_payload, Some(decomp_size)) {
        hex_dump("decompressed", &dec, 64);
        println!("  decompressed={} bytes", dec.len());
    }

    let pages = match section_map::parse_page_map(&bytes, &header) {
        Ok(p) => {
            println!("[page_map] parsed {} entries", p.len());
            for (i, pg) in p.iter().enumerate().take(8) {
                println!(
                    "  page[{i}] num={} size=0x{:x} offset=0x{:x} gap={}",
                    pg.number, pg.size, pg.file_offset, pg.is_gap
                );
            }
            p
        }
        Err(e) => {
            println!("[page_map] ERROR: {e}");
            return Ok(());
        }
    };

    match section_map::parse_section_info(&bytes, &header, &pages) {
        Ok(descs) => {
            println!("[section_info] parsed {} descriptions", descs.len());
            for d in &descs {
                println!(
                    "  {:<24} size={} pages={} compr={} enc={} id={}",
                    d.name, d.size, d.page_count, d.compressed, d.encrypted, d.section_id
                );
            }
        }
        Err(e) => println!("[section_info] ERROR: {e}"),
    }
    Ok(())
}
