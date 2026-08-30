//! Scratch: which handles do records reference that no record answers?

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::string_stream;
use dwg::DwgFile;
use std::collections::{BTreeMap, BTreeSet};

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg>");
    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file.all_objects().unwrap()?;
    let present: BTreeSet<u64> = objects.iter().map(|o| o.handle.value).collect();
    let mut kinds = BTreeMap::new();
    for o in &objects {
        kinds.insert(o.handle.value, (o.kind, o.type_code, o.size_bytes));
    }

    let mut dangling: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for object in &objects {
        let Some(start) = string_stream::data_section_end(&object.raw, version) else {
            continue;
        };
        let mut c = BitCursor::new(&object.raw);
        while c.position_bits() < start {
            if c.read_b().is_err() {
                break;
            }
        }
        for _ in 0..512 {
            if c.remaining_bits() < 8 {
                break;
            }
            let Ok(h) = c.read_handle() else { break };
            if !h.is_absolute() || h.value == 0 {
                continue;
            }
            if !present.contains(&h.value) {
                dangling
                    .entry(object.handle.value)
                    .or_default()
                    .push(h.value);
            }
        }
    }

    println!("records: {}", objects.len());
    println!("records with dangling refs: {}", dangling.len());
    let total: usize = dangling.values().map(|v| v.len()).sum();
    println!("dangling refs total: {total}");
    for (from, to) in &dangling {
        let k = kinds.get(from).map(|k| format!("{:?}", k.0)).unwrap_or_default();
        println!("  {from} ({k}) -> {to:?}");
    }

    println!();
    println!("sizes by class of interest:");
    for o in &objects {
        if matches!(o.type_code, 523 | 524 | 525) {
            println!(
                "  handle {} type {} size {} bytes",
                o.handle.value, o.type_code, o.size_bytes
            );
        }
    }
    Ok(())
}
