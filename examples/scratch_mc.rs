//! Scratch: does each inter-record gap equal that record's leading MC width?
use dwg::error::Result;
use dwg::DwgFile;

fn mc_len(raw: &[u8]) -> usize {
    for (i, b) in raw.iter().enumerate().take(10) {
        if b & 0x80 == 0 {
            return i + 1;
        }
    }
    0
}

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg>");
    let file = DwgFile::open(&path)?;
    let objects = file.all_objects().unwrap()?;
    let stream = file.read_section("AcDb:AcDbObjects").unwrap()?;
    let mut spans: Vec<(usize, usize, usize)> = objects
        .iter()
        .map(|o| {
            let ms_len = if o.size_bytes < 0x8000 { 2 } else { 4 };
            (
                o.stream_offset,
                o.stream_offset + ms_len + o.size_bytes as usize + 2,
                mc_len(&o.raw),
            )
        })
        .collect();
    spans.sort_unstable();
    let mut alt_agree = 0;
    let mut alt_disagree = 0;
    let mut agree = 0;
    let mut disagree = 0;
    let mut leading = 0usize;
    let mut end = 0usize;
    for (i, (s, e, mc)) in spans.iter().enumerate() {
        if i == 0 {
            leading = *s;
        }
        if i > 0 {
            let gap = s.saturating_sub(end);
            let prev_mc = spans[i - 1].2;
            let next_mc = *mc;
            if gap == next_mc { alt_agree += 1; } else { alt_disagree += 1; }
            if gap == prev_mc {
                agree += 1;
            } else {
                disagree += 1;
                if disagree < 6 {
                    println!("  disagree at record {i}: gap {gap} prev_mc {prev_mc}");
                }
            }
        }
        end = end.max(*e);
    }
    let last_gap = stream.len().saturating_sub(end);
    println!("records {} leading bytes {leading}", spans.len());
    println!("gap == previous record's MC width: agree {agree}, disagree {disagree}");
    println!("ALT (gap == following record's MC width): agree {alt_agree}, disagree {alt_disagree}");
    println!("trailing gap after last record: {last_gap} (last record MC {})", spans.last().unwrap().2);
    Ok(())
}
