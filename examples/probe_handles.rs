//! Dump the raw handle map for a .dwg file.

use dwg::DwgFile;
use std::env;

fn main() -> anyhow::Result<()> {
    let path = env::args()
        .nth(1)
        .expect("usage: probe_handles <file.dwg>");
    let f = DwgFile::open(&path)?;
    let hmap = f
        .handle_map()
        .ok_or_else(|| anyhow::anyhow!("no handle map"))??;
    println!("{} handle entries", hmap.entries.len());
    for (i, e) in hmap.entries.iter().take(40).enumerate() {
        println!("  [{i:3}] h={} off={}", e.handle, e.offset);
    }
    if hmap.entries.len() > 40 {
        println!("  ...");
        for (i, e) in hmap.entries.iter().enumerate().skip(hmap.entries.len() - 5)
        {
            println!("  [{i:3}] h={} off={}", e.handle, e.offset);
        }
    }
    Ok(())
}
