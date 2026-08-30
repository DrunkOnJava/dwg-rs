//! Scratch: full hex dump of one object's payload.

use dwg::error::Result;
use dwg::DwgFile;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg> <handle>");
    let want: u64 = std::env::args().nth(2).unwrap().parse().unwrap();
    let file = DwgFile::open(&path)?;
    let objects = file.all_objects().unwrap()?;
    for object in &objects {
        if object.handle.value != want {
            continue;
        }
        println!(
            "handle {} kind {:?} type_code 0x{:04X} size {} raw {}",
            object.handle.value,
            object.kind,
            object.type_code,
            object.size_bytes,
            object.raw.len()
        );
        for (i, chunk) in object.raw.chunks(16).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let ascii: String = chunk
                .iter()
                .map(|b| {
                    if b.is_ascii_graphic() {
                        *b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            println!("{:04}  {:<48} {ascii}", i * 16, hex.join(" "));
        }
    }
    Ok(())
}
