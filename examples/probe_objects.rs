//! Walk the object stream of a .dwg file and print a type histogram.

use dwg::DwgFile;
use std::collections::BTreeMap;
use std::env;

fn main() -> anyhow::Result<()> {
    let path = env::args().nth(1).expect("usage: probe_objects <file.dwg>");
    let f = DwgFile::open(&path)?;
    let objects = f
        .objects()
        .ok_or_else(|| anyhow::anyhow!("no object stream (not R2004-family?)"))??;
    println!("parsed {} objects", objects.len());
    let mut hist: BTreeMap<String, usize> = BTreeMap::new();
    let mut entity_count = 0usize;
    let mut control_count = 0usize;
    for o in &objects {
        *hist.entry(o.kind.to_string()).or_insert(0) += 1;
        if o.kind.is_entity() {
            entity_count += 1;
        }
        if o.kind.is_control() {
            control_count += 1;
        }
    }
    println!("entities: {entity_count}, control objects: {control_count}");
    println!();
    println!("type histogram:");
    for (t, n) in &hist {
        println!("  {n:>6}  {t}");
    }
    if let Some(first) = objects.first() {
        println!();
        println!("first object:");
        println!("  kind:       {}", first.kind);
        println!("  type_code:  0x{:x}", first.type_code);
        println!("  size_bytes: {}", first.size_bytes);
        println!(
            "  handle:     code={} counter={} value=0x{:x}",
            first.handle.code, first.handle.counter, first.handle.value
        );
    }
    Ok(())
}
