//! Runs the bounded §4.1 R2004-family file-header entry point, never a
//! page-map or object walk. `--legacy` prints the same fields from the
//! legacy whole-file `header::R2004Header`, for comparison.
//! `stream_r2004 file.dwg [chunk-bytes] [--chunks 1,7,3]`
//! `--identity <sha256> --checkpoint-out <path> --stop-after <offset>`
//! suspends at an exact byte; a fresh invocation with `--resume <path>`
//! continues without reading earlier source bytes. `--trace <path>` saves
//! actual source reads. Identity must come from a trusted upstream manifest.

use dwg::byte_source::{ByteSource, FileSource, HeaderPoll, SourcePoll, poll_r2004_header};
use dwg::streaming::{
    CHUNK_BYTES, R2004_HEADER_BYTES, R2004Header, R2004HeaderParser, SourceIdentity,
};
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

fn print_header(h: R2004Header) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{{\"magic\":\"{}\",\"maintenance\":{},\"preview_address\":{},\"app_version\":{},\"app_maintenance\":{},\"codepage\":{},\"security_flags\":{},\"summary_info_address\":{},\"vba_project_address\":{},\"last_page_id\":{},\"last_page_end\":{},\"second_header_address\":{},\"gap_amount\":{},\"page_amount\":{},\"page_map_id\":{},\"page_map_address\":{},\"section_map_id\":{},\"page_array_size\":{},\"gap_array_size\":{}}}",
        std::str::from_utf8(&h.magic)?,
        h.maintenance,
        h.preview_address,
        h.app_version,
        h.app_maintenance,
        h.codepage,
        h.security_flags,
        h.summary_info_address,
        h.vba_project_address,
        h.last_page_id,
        h.last_page_end,
        h.second_header_address,
        h.gap_amount,
        h.page_amount,
        h.page_map_id,
        h.page_map_address,
        h.section_map_id,
        h.page_array_size,
        h.gap_array_size
    );
    Ok(())
}

/// The legacy reader's fields, under the same keys where it has them.
fn legacy(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(path)?;
    let h = dwg::header::R2004Header::parse(&bytes)?;
    println!(
        "{{\"magic\":\"{}\",\"maintenance\":{},\"preview_address\":{},\"codepage\":{},\"security_flags\":{},\"summary_info_address\":{},\"vba_project_address\":{},\"last_page_id\":{},\"last_page_end\":{},\"second_header_address\":{},\"gap_amount\":{},\"page_amount\":{},\"page_map_id\":{},\"page_map_address\":{},\"section_map_id\":{},\"page_array_size\":{},\"gap_array_size\":{}}}",
        std::str::from_utf8(&bytes[..6])?,
        h.common.maint_version,
        h.common.image_seeker,
        h.common.codepage,
        h.security_flags,
        h.summary_info_addr,
        h.vba_project_addr,
        h.last_section_page_id,
        h.last_section_page_end,
        h.second_header_addr,
        h.gap_amount,
        h.section_page_amount,
        h.section_page_map_id,
        h.section_page_map_addr + 0x100,
        h.section_map_id,
        h.section_page_array_size,
        h.gap_array_size
    );
    Ok(())
}

fn save(parser: &R2004HeaderParser, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    File::create(path)?.write_all(parser.checkpoint().as_bytes())?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: stream_r2004 file.dwg [chunk-bytes] [options]")?;
    let mut chunks = vec![CHUNK_BYTES];
    let mut stop_after = None;
    let mut checkpoint_out = None;
    let mut resume = None;
    let mut trace = None;
    let mut digest = None;
    let mut inject_pending = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--legacy" => return legacy(&path),
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
        if length > R2004_HEADER_BYTES + 60 {
            return Err("checkpoint exceeds its bound".into());
        }
        let mut bytes = [0; R2004_HEADER_BYTES + 60];
        checkpoint.read_exact(&mut bytes[..length])?;
        R2004HeaderParser::restore(&bytes[..length], identity)?
    } else {
        R2004HeaderParser::new(identity)
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
        match poll_r2004_header(&mut parser, &mut source, &mut scratch[..n]) {
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
