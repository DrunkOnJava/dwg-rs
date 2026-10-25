//! Runs the actual bounded §3.2 header entry point on a DWG file.
//! `cargo run --example stream_header -- file.dwg [chunk-bytes]`
//! Output is deterministic across every legal chunk schedule. This reads
//! only the header; success says nothing about the drawing's object coverage.

use dwg::streaming::{CHUNK_BYTES, FlatHeaderParser, SourceIdentity};
use std::fs::File;
use std::io::Read;

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
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut parser = FlatHeaderParser::new(SourceIdentity {
        digest: [0; 32],
        length,
    });
    let mut scratch = [0; CHUNK_BYTES];
    while let Some(request) = parser.request()? {
        let n = file.read(&mut scratch[..request.len.min(chunk)])?;
        if n == 0 {
            parser.end_of_input()?;
        }
        parser.feed(request.offset, &scratch[..n])?;
    }
    println!("{:?}", parser.header().ok_or("missing header")?);
    Ok(())
}
