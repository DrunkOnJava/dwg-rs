//! Runs the actual bounded §3.2 header entry point on a DWG file.
//! `cargo run --example stream_header -- file.dwg [chunk-bytes]`
//! Output is deterministic across every legal chunk schedule. This reads
//! only the header; success says nothing about the drawing's object coverage.

use dwg::byte_source::{FileSource, HeaderPoll, poll_header};
use dwg::streaming::{CHUNK_BYTES, FlatHeaderParser, SourceIdentity};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: stream_header file.dwg [chunk-bytes]")?;
    let chunk: usize = args
        .next()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(CHUNK_BYTES);
    if chunk == 0 || chunk > CHUNK_BYTES {
        return Err("chunk must be 1..=256".into());
    }
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut parser = FlatHeaderParser::new(SourceIdentity {
        digest: [0; 32],
        length,
    });
    let mut scratch = [0; CHUNK_BYTES];
    let mut source = FileSource::new(file);
    loop {
        match poll_header(&mut parser, &mut source, &mut scratch[..chunk])? {
            HeaderPoll::Complete => break,
            HeaderPoll::Pending => return Err("source pending; reschedule this poll".into()),
            HeaderPoll::Progress(_) => (),
        }
    }
    let header = parser.header().ok_or("missing header")?;
    print!(
        "{{\"magic\":\"{}\",\"maintenance\":{},\"image_seeker\":{},\"codepage\":{},\"locator_count\":{},\"locators\":[",
        std::str::from_utf8(&header.magic)?,
        header.maintenance,
        header.image_seeker,
        header.codepage,
        header.locator_count
    );
    for (index, locator) in header.locators[..header.locator_count as usize]
        .iter()
        .enumerate()
    {
        if index != 0 {
            print!(",");
        }
        print!("[{},{},{}]", locator.number, locator.offset, locator.size);
    }
    println!("]}}");
    Ok(())
}
