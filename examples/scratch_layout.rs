//! Scratch: where does raw sit inside the stream?
use dwg::error::Result;
use dwg::DwgFile;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg>");
    let file = DwgFile::open(&path)?;
    let objects = file.all_objects().unwrap()?;
    let stream = file.read_section("AcDb:AcDbObjects").unwrap()?;
    for o in objects.iter().take(6) {
        let off = o.stream_offset;
        println!(
            "handle {} off {} size {} stream[off..off+10] {:02x?} raw[0..6] {:02x?}",
            o.handle.value,
            off,
            o.size_bytes,
            &stream[off..(off + 10).min(stream.len())],
            &o.raw[..6.min(o.raw.len())]
        );
        // find where raw begins inside the stream
        for cand in 0..8usize {
            if stream[off + cand..].starts_with(&o.raw[..8.min(o.raw.len())]) {
                println!("   raw begins at off+{cand}; record end = off+{} ; next?", cand + o.raw.len());
                break;
            }
        }
    }
    Ok(())
}
