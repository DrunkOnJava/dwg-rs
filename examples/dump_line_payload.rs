//! Bit-walk one LINE entity's object record against ODA §19.1 /
//! §19.4.1 and report where the data stream actually ends.
//!
//! # What this proves
//!
//! §19.1 gives the object-record prologue as
//!
//! ```text
//! MS        object size in bytes            (stripped by the walker)
//! R2010+:   MC   handle-stream size in bits
//! Common:   OT   object type
//! R2000-R2007: RL  object data size in bits   <-- "obj_size"
//! Common:   H    object handle
//! ```
//!
//! The `RL obj_size` field exists for the whole R2000..R2007 band, not
//! for R2000 alone. This probe reads the prologue both ways and then
//! walks the common entity preamble plus the LINE body, printing the
//! bit at which the body ends.
//!
//! # How to verify
//!
//! ```sh
//! cargo run --release --example dump_line_payload -- samples/line_2004.dwg
//! cargo run --release --example dump_line_payload -- samples/line_2013.dwg
//! ```
//!
//! On `line_2004.dwg` the "with obj_size" walk ends exactly on the
//! `obj_size` boundary and recovers the authored geometry, while the
//! "without obj_size" walk runs off the end of the record. On
//! `line_2013.dwg` (R2010+, no `RL`) the walk is unchanged.

use dwg::bitcursor::BitCursor;
use dwg::{DwgFile, Version};

fn read_mc_unsigned(c: &mut BitCursor<'_>) -> Result<u64, Box<dyn std::error::Error>> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for _ in 0..10 {
        let b = c.read_rc()? as u64;
        value |= (b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
    Err("MC length exceeded 10 bytes".into())
}

fn read_object_type(
    c: &mut BitCursor<'_>,
    version: Version,
) -> Result<u16, Box<dyn std::error::Error>> {
    if version.is_r2010_plus() {
        let tag = c.read_bb()?;
        Ok(match tag {
            0 => c.read_rc()? as u16,
            1 => (c.read_rc()? as u16) + 0x1F0,
            _ => {
                let lsb = c.read_rc()? as u16;
                let msb = c.read_rc()? as u16;
                (msb << 8) | lsb
            }
        })
    } else {
        Ok(c.read_bs_u()?)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).expect("path arg");
    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file.all_objects().expect("version has a handle walk")?;

    let line = objects
        .iter()
        .find(|o| o.type_code == 0x13)
        .expect("no LINE entity found");

    println!("=== LINE entity (type 0x13) in {path} ===");
    println!("version: {}", version.release());
    println!(
        "handle: 0x{:X}, stream_offset: {}, size_bytes: {}, payload len: {} ({} bits)",
        line.handle.value,
        line.stream_offset,
        line.size_bytes,
        line.raw.len(),
        line.raw.len() * 8
    );
    println!();
    println!("full payload (hex):");
    for (i, chunk) in line.raw.chunks(16).enumerate() {
        print!("  {:04X}:", i * 16);
        for b in chunk {
            print!(" {b:02X}");
        }
        println!();
    }
    println!();

    for read_obj_size in [false, true] {
        // R2010+ has no RL obj_size at all, so only one arm is meaningful.
        if version.is_r2010_plus() && read_obj_size {
            continue;
        }
        println!(
            "=== bit-walk, RL obj_size {} ===",
            if read_obj_size { "READ" } else { "SKIPPED" }
        );
        let mut c = BitCursor::new(&line.raw);

        if version.is_r2010_plus() {
            let mc = read_mc_unsigned(&mut c)?;
            println!(
                "  MC handle-stream-bits = {mc}  -> bit {}",
                c.position_bits()
            );
        }
        let type_code = read_object_type(&mut c, version)?;
        println!(
            "  OT type_code = 0x{type_code:04X}  -> bit {}",
            c.position_bits()
        );

        let mut obj_size = None;
        if read_obj_size {
            let v = c.read_rl()?;
            obj_size = Some(v as usize);
            println!("  RL obj_size = {v} bits  -> bit {}", c.position_bits());
        }

        let handle = c.read_handle()?;
        println!(
            "  H handle code={} counter={} value=0x{:X}  -> bit {}",
            handle.code,
            handle.counter,
            handle.value,
            c.position_bits()
        );

        match dwg::common_entity::read_common_entity_data(&mut c, version) {
            Ok(ce) => println!(
                "  common preamble ok: mode={:?} reactors={} lw=0x{:02X}  -> bit {}",
                ce.mode,
                ce.num_reactors,
                ce.lineweight,
                c.position_bits()
            ),
            Err(e) => {
                println!("  common preamble FAILED: {e}");
                println!();
                continue;
            }
        }

        match dwg::entities::line::decode(&mut c) {
            Ok(l) => println!(
                "  LINE body ok: start=({}, {}, {}) end=({}, {}, {})  -> bit {}",
                l.start.x,
                l.start.y,
                l.start.z,
                l.end.x,
                l.end.y,
                l.end.z,
                c.position_bits()
            ),
            Err(e) => {
                println!("  LINE body FAILED: {e}");
                println!();
                continue;
            }
        }

        if let Some(obj_size) = obj_size {
            println!(
                "  data-stream end check: body ended at bit {}, obj_size says {} (delta {})",
                c.position_bits(),
                obj_size,
                c.position_bits() as isize - obj_size as isize
            );
        }
        println!(
            "  payload has {} bits total; {} unread",
            line.raw.len() * 8,
            line.raw.len() * 8 - c.position_bits()
        );
        println!();
    }

    Ok(())
}
