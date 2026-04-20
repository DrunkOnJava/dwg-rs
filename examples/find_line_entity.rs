//! One-shot RE script — find the LINE entity in line_2013.dwg.
//!
//! For every object in the handle-driven walk: dump the handle
//! value, the on-stream offset, the raw type code, the classified
//! kind, and the first 16 bytes of payload in hex. The LINE has
//! type_code = 0x13. Its position in the handle walk + its
//! payload's first bytes tell us which object "owns" it and what
//! the cursor position looks like when the decoder hits it.

use dwg::DwgFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).expect("path arg");
    let file = DwgFile::open(&path)?;
    println!("version: {}", file.version());

    let objects = match file.all_objects() {
        Some(Ok(v)) => v,
        Some(Err(e)) => return Err(e.into()),
        None => {
            eprintln!("no handle-driven walk for this version");
            return Ok(());
        }
    };

    println!("total objects: {}", objects.len());
    println!();
    println!(
        "{:>3}  {:>10}  {:>6}  {:>5}  {:<20}  hex prefix",
        "idx", "handle", "offset", "tcode", "kind"
    );
    println!("{}", "-".repeat(100));
    let mut line_positions = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        let kind_str = format!("{:?}", obj.kind);
        let hex = obj
            .raw
            .iter()
            .take(16)
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        if obj.type_code == 0x13 {
            line_positions.push(i);
            println!(
                ">>> {:>2}  0x{:08X}  {:>6}  0x{:02X}  {:<20}  {}",
                i, obj.handle.value, obj.stream_offset, obj.type_code, kind_str, hex
            );
        } else {
            println!(
                "    {:>2}  0x{:08X}  {:>6}  0x{:02X}  {:<20}  {}",
                i, obj.handle.value, obj.stream_offset, obj.type_code, kind_str, hex
            );
        }
    }
    println!();
    println!(
        "found {} LINE entity/entities at handle walker index(es): {:?}",
        line_positions.len(),
        line_positions
    );
    Ok(())
}
