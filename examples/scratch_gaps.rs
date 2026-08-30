//! Scratch: what is in the unclaimed inter-record bytes?
use dwg::error::Result;
use dwg::DwgFile;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg>");
    let file = DwgFile::open(&path)?;
    let objects = file.all_objects().unwrap()?;
    let stream = file.read_section("AcDb:AcDbObjects").unwrap()?;
    let mut spans: Vec<(usize, usize, u64, u32)> = objects
        .iter()
        .map(|o| {
            let ms_len = if o.size_bytes < 0x8000 { 2 } else { 4 };
            (
                o.stream_offset,
                o.stream_offset + ms_len + o.size_bytes as usize + 2,
                o.handle.value,
                o.size_bytes,
            )
        })
        .collect();
    spans.sort_unstable();
    let mut end = 0usize;
    let mut shown = 0;
    let mut parity = [0usize; 2];
    for (s, e, h, sz) in &spans {
        if *s > end {
            let gap = &stream[end..*s];
            parity[end % 2] += 1;
            if shown < 20 {
                println!(
                    "gap {end}..{s} ({} bytes) {:02x?} before handle {h} size {sz}; \
                     next start {s} even={}",
                    gap.len(),
                    gap,
                    s % 2 == 0
                );
                shown += 1;
            }
        }
        end = end.max(*e);
    }
    println!("gap-start parity counts (even, odd): {parity:?}");
    println!("stream len {} last end {end}", stream.len());
    Ok(())
}
