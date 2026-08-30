//! Scratch: flat decode of a record's trailing handle stream.

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::string_stream;
use dwg::{DwgFile, ObjectType};

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg>");
    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file.all_objects().unwrap()?;
    let mut kinds = std::collections::BTreeMap::new();
    for o in &objects {
        kinds.insert(o.handle.value, o.kind);
    }

    for object in &objects {
        if object.kind != ObjectType::BlockHeader {
            continue;
        }
        let Some(start) = string_stream::data_section_end(&object.raw, version) else {
            continue;
        };
        let mut c = BitCursor::new(&object.raw);
        while c.position_bits() < start { let _ = c.read_b()?; }
        let own = object.handle.value;
        let mut refs = Vec::new();
        for _ in 0..40 {
            if c.remaining_bits() < 8 {
                break;
            }
            let Ok(h) = c.read_handle() else { break };
            let v = match h.code {
                0x6 => own + 1,
                0x8 => own.wrapping_sub(1),
                0xA => own + h.value,
                0xC => own.wrapping_sub(h.value),
                _ => h.value,
            };
            let k = kinds.get(&v).map(|k| format!("{k:?}")).unwrap_or_default();
            refs.push(format!("{}.{}:{v}{}", h.code, h.counter, if k.is_empty() { String::new() } else { format!("({k})") }));
        }
        println!("BLOCK_HEADER {own}: {}", refs.join(" "));
    }
    Ok(())
}
