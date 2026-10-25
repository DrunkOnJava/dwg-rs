//! Runs the bounded §3.2 file-header entry point, never an object walk.
//! `stream_header file.dwg [chunk-bytes] [--chunks 1,7,3]`
//! `--identity <sha256> --checkpoint-out <path> --stop-after <offset>`
//! suspends at an exact byte; a fresh invocation with `--resume <path>`
//! continues without reading earlier source bytes. `--trace <path>` saves
//! actual source reads. Identity must come from a trusted upstream manifest.

use dwg::byte_source::{ByteSource, FileSource, HeaderPoll, SourcePoll, poll_header};
use dwg::streaming::{CHUNK_BYTES, FlatHeader, FlatHeaderParser, SourceIdentity};
use std::fs::File;
use std::io::{self, Read, Write};

#[derive(Debug)]
struct TracedSource {
    file: FileSource<File>,
    trace: Option<File>,
    inject_pending: bool,
    pending: bool,
}

impl ByteSource for TracedSource {
    type Error = io::Error;
    fn poll_read(
        &mut self,
        request: dwg::streaming::Request,
        output: &mut [u8],
    ) -> io::Result<SourcePoll> {
        if self.inject_pending && !self.pending {
            self.pending = true;
            return Ok(SourcePoll::Pending);
        }
        self.pending = false;
        let result = self.file.poll_read(request, output)?;
        if let (SourcePoll::Ready(n), Some(trace)) = (result, self.trace.as_mut()) {
            writeln!(trace, "{{\"offset\":{},\"bytes\":{n}}}", request.offset)?;
        }
        Ok(result)
    }
}

fn print_header(header: FlatHeader) -> Result<(), Box<dyn std::error::Error>> {
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

fn save(parser: &FlatHeaderParser, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    File::create(path)?.write_all(parser.checkpoint()?.as_bytes())?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: stream_header file.dwg [chunk-bytes] [options]")?;
    let mut chunks = vec![CHUNK_BYTES];
    let mut stop_after = None;
    let mut checkpoint_out = None;
    let mut resume = None;
    let mut trace = None;
    let mut digest = None;
    let mut inject_pending = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--stop-after" => {
                stop_after = Some(args.next().ok_or("missing stop offset")?.parse::<u64>()?)
            }
            "--checkpoint-out" => {
                checkpoint_out = Some(args.next().ok_or("missing checkpoint path")?)
            }
            "--resume" => resume = Some(args.next().ok_or("missing resume path")?),
            "--trace" => trace = Some(args.next().ok_or("missing trace path")?),
            "--pending" => inject_pending = true,
            "--chunks" => {
                let value = args.next().ok_or("missing chunk schedule")?;
                if value.len() > 1024 {
                    return Err("chunk schedule too long".into());
                }
                chunks = value
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<Vec<usize>, _>>()?;
            }
            "--identity" => {
                let hex = args.next().ok_or("missing source digest")?;
                if hex.len() != 64 || !hex.is_ascii() {
                    return Err("identity must be 64 hex characters".into());
                }
                let mut decoded = [0; 32];
                for (index, byte) in decoded.iter_mut().enumerate() {
                    *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)?;
                }
                digest = Some(decoded);
            }
            value if !value.starts_with('-') => chunks = vec![value.parse()?],
            _ => return Err("unknown option".into()),
        }
    }
    if chunks.is_empty() || chunks.iter().any(|&n| n == 0 || n > CHUNK_BYTES) {
        return Err("chunk sizes must be 1..=256".into());
    }
    if (resume.is_some() || checkpoint_out.is_some()) && digest.is_none() {
        return Err("checkpointing requires --identity from a trusted immutable source".into());
    }
    if stop_after.is_some() && checkpoint_out.is_none() {
        return Err("--stop-after requires --checkpoint-out".into());
    }
    let file = File::open(path)?;
    let identity = SourceIdentity {
        digest: digest.unwrap_or([0; 32]),
        length: file.metadata()?.len(),
    };
    let mut parser = if let Some(path) = resume {
        let mut checkpoint = File::open(path)?;
        let length = usize::try_from(checkpoint.metadata()?.len())?;
        if length > CHUNK_BYTES {
            return Err("checkpoint exceeds one chunk".into());
        }
        let mut bytes = [0; CHUNK_BYTES];
        checkpoint.read_exact(&mut bytes[..length])?;
        FlatHeaderParser::restore(&bytes[..length], identity)?
    } else {
        FlatHeaderParser::new(identity)
    };
    let mut source = TracedSource {
        file: FileSource::new(file),
        trace: trace.map(File::create).transpose()?,
        inject_pending,
        pending: false,
    };
    let mut scratch = [0; CHUNK_BYTES];
    let mut poll = 0;
    loop {
        let position = parser.position();
        if stop_after.is_some_and(|at| position >= at) {
            save(
                &parser,
                checkpoint_out.as_deref().ok_or("missing checkpoint path")?,
            )?;
            println!("{{\"checkpoint\":true,\"offset\":{position}}}");
            return Ok(());
        }
        let mut n = chunks[poll % chunks.len()];
        if let Some(stop) = stop_after {
            n = n.min(usize::try_from(stop - position).unwrap_or(CHUNK_BYTES));
        }
        poll += 1;
        let request_before = parser.request();
        match poll_header(&mut parser, &mut source, &mut scratch[..n]) {
            Ok(HeaderPoll::Complete) => break,
            Ok(HeaderPoll::Pending) => {
                if parser.request() != request_before {
                    return Err("pending source mutated grammar".into());
                }
            }
            Ok(HeaderPoll::Progress(_)) => (),
            Err(error) => {
                if let Some(path) = checkpoint_out {
                    save(&parser, &path)?;
                }
                return Err(error.into());
            }
        }
    }
    if let Some(path) = checkpoint_out {
        save(&parser, &path)?;
    }
    print_header(parser.header().ok_or("missing verified header")?)
}
