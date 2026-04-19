//! Quick smoke test for the Phase D-1 metadata parsers.

use dwg::DwgFile;
use std::env;

fn hex_dump(label: &str, data: &[u8]) {
    println!("[{label}] {} bytes", data.len());
    for (i, chunk) in data.chunks(16).enumerate().take(8) {
        print!("  {:04x}:", i * 16);
        for b in chunk {
            print!(" {:02x}", b);
        }
        print!("  ");
        for b in chunk {
            print!(
                "{}",
                if (0x20..0x7F).contains(b) { *b as char } else { '.' }
            );
        }
        println!();
    }
}

fn main() -> anyhow::Result<()> {
    let path = env::args()
        .nth(1)
        .expect("usage: probe_metadata <file.dwg>");
    let f = DwgFile::open(&path)?;
    println!("version: {}", f.version());
    for section in [
        "AcDb:SummaryInfo",
        "AcDb:AppInfo",
        "AcDb:AppInfoHistory",
        "AcDb:FileDepList",
        "AcDb:Preview",
    ] {
        match f.read_section(section) {
            Some(Ok(bytes)) => hex_dump(section, &bytes),
            Some(Err(e)) => println!("[{section}] error: {e}"),
            None => println!("[{section}] not present"),
        }
    }
    Ok(())
}
