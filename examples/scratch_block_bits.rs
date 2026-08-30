//! Scratch: raw bit dump around a BLOCK_HEADER's string stream.

use dwg::error::Result;
use dwg::string_stream;
use dwg::{DwgFile, ObjectType};

fn bit_at(bytes: &[u8], bit: usize) -> u8 {
    (bytes[bit / 8] >> (7 - (bit % 8))) & 1
}

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg> [handle]");
    let want: Option<u64> = std::env::args().nth(2).map(|s| s.parse().unwrap());
    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file.all_objects().unwrap()?;

    for object in &objects {
        if !matches!(object.kind, ObjectType::BlockHeader | ObjectType::Block) {
            continue;
        }
        if let Some(w) = want {
            if object.handle.value != w {
                continue;
            }
        }
        let total = object.raw.len() * 8;
        let sec_end = string_stream::data_section_end(&object.raw, version);
        let located = string_stream::locate(&object.raw, version);
        println!(
            "{:?} handle {} payload_bits {} data_section_end {:?} located {:?}",
            object.kind, object.handle.value, total, sec_end, located
        );
        if let (Some(end), Some(stream)) = (sec_end, located) {
            let lo = end - 17;
            let mut v: u32 = 0;
            for i in 0..16 {
                v = (v << 1) | bit_at(&object.raw, lo + i) as u32;
            }
            // read_rs is little-endian over two RC bytes
            let mut b0: u32 = 0;
            let mut b1: u32 = 0;
            for i in 0..8 {
                b0 = (b0 << 1) | bit_at(&object.raw, lo + i) as u32;
            }
            for i in 8..16 {
                b1 = (b1 << 1) | bit_at(&object.raw, lo + i) as u32;
            }
            println!(
                "  trailer RS raw bits {v:#06x}  as LE {:#06x} = {}  flagbit {}",
                b0 | (b1 << 8),
                b0 | (b1 << 8),
                bit_at(&object.raw, end - 1)
            );
            let from = stream.start_bit.saturating_sub(48);
            let to = end.min(total);
            let mut s = String::new();
            for bit in from..to {
                if bit == stream.start_bit {
                    s.push('[');
                }
                if bit == stream.end_bit {
                    s.push(']');
                }
                s.push(if bit_at(&object.raw, bit) == 1 { '1' } else { '0' });
            }
            println!("  bits {from}..{to}: {s}");
        }
    }
    Ok(())
}
