//! Brute-force scan: for every candidate "preamble end" cursor
//! position from bit 30 to bit 130 against the LINE payload, run
//! line::decode and report which positions yield a successful
//! decode + the resulting field values.
//!
//! Whichever position decodes to plausible coordinates (e.g., values
//! in the [-1e6, 1e6] range, not 1.2e+225) tells us where the
//! preamble OUGHT to end, working backward from the answer.

use dwg::DwgFile;
use dwg::bitcursor::BitCursor;
use dwg::entities::line;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = DwgFile::open("../../samples/line_2013.dwg")?;
    let objects = file.all_objects().unwrap()?;
    let line_obj = objects.iter().find(|o| o.type_code == 0x13).unwrap();
    let payload = &line_obj.raw;
    println!(
        "payload: {} bits ({} bytes)",
        payload.len() * 8,
        payload.len()
    );
    println!("data stream ends at bit {}", payload.len() * 8 - 22);
    println!();

    let mut hits = 0;
    for start_bit in 30..=130usize {
        let mut c = BitCursor::new(payload);
        // Skip to start_bit by reading individual bits.
        for _ in 0..start_bit {
            if c.read_b().is_err() {
                break;
            }
        }
        if c.position_bits() != start_bit {
            continue;
        }
        // Try line::decode from here.
        match line::decode(&mut c) {
            Ok(line) => {
                let plausible_x = line.start.x.abs() < 1e6 && line.end.x.abs() < 1e6;
                let plausible_y = line.start.y.abs() < 1e6 && line.end.y.abs() < 1e6;
                let marker = if plausible_x && plausible_y {
                    ">>"
                } else {
                    "  "
                };
                println!(
                    "{marker} start_bit {start_bit:3}: 2d={} sx={:.3e} sy={:.3e} ex={:.3e} ey={:.3e} thick={:.3e}",
                    line.is_2d, line.start.x, line.start.y, line.end.x, line.end.y, line.thickness,
                );
                hits += 1;
            }
            Err(_) => {
                // Suppress — most positions will error.
            }
        }
    }
    println!("\n{hits} positions decoded successfully (most will be garbage)");
    Ok(())
}
