//! Runs the bounded R2004-family system-page walk (§4.1 header, §4.4 page
//! map, §4.5 section map) and prints one JSON line per committed step.
//! `stream_r2004_maps file.dwg [chunk-bytes] [--chunks 1,7,3] [--pending]`
//! `--trace <path>` saves actual source reads. `--legacy` prints the legacy
//! reader's page map and section info in the same shape (pages, sections
//! and section pages only). `--identity <sha256> --checkpoint-out <path>
//! --stop-after <steps>` saves the walk after that many steps (feeds and
//! pumps); a fresh invocation with `--resume <path>` continues.

use dwg::byte_source::{ByteSource, FileSource, MapPoll, SourcePoll, poll_r2004_maps};
use dwg::header::R2004Header;
use dwg::section_map;
use dwg::streaming::{CHUNK_BYTES, MapStep, R2004MapWalker, Request, SourceIdentity, SystemKind};
use std::fs::{self, File};
use std::io::{self, Write};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug)]
struct TracedSource {
    file: FileSource<File>,
    trace: Option<File>,
    inject_pending: bool,
    pending: bool,
}

impl ByteSource for TracedSource {
    type Error = io::Error;
    fn poll_read(&mut self, request: Request, output: &mut [u8]) -> io::Result<SourcePoll> {
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

fn name(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn page_line(number: i32, size: u32, address: u64) -> String {
    format!("{{\"page\":{number},\"size\":{size},\"address\":{address}}}")
}

/// `fields` is size, page count, max page size, compressed, section id,
/// encrypted, in §4.5 order.
fn section_line(index: u32, name: &str, fields: [u64; 6]) -> String {
    let [size, pages, max, compressed, id, encrypted] = fields;
    format!(
        "{{\"section\":{index},\"name\":\"{name}\",\"size\":{size},\"page_count\":{pages},\"max_page_size\":{max},\"compressed\":{compressed},\"section_id\":{id},\"encrypted\":{encrypted}}}"
    )
}

fn ref_line(id: u32, page: u32, data: u32, start: u64) -> String {
    format!(
        "{{\"section_page\":{page},\"section_id\":{id},\"data_size\":{data},\"start_offset\":{start}}}"
    )
}

fn print(step: MapStep) {
    match step {
        MapStep::Framing => (),
        MapStep::Header(h) => println!(
            "{{\"header\":\"{}\",\"page_map_id\":{},\"page_map_address\":{},\"section_map_id\":{}}}",
            String::from_utf8_lossy(&h.magic),
            h.page_map_id,
            h.page_map_address,
            h.section_map_id
        ),
        MapStep::SystemPage {
            kind,
            address,
            compressed,
            decompressed,
        } => println!(
            "{{\"system_page\":\"{}\",\"address\":{address},\"compressed\":{compressed},\"decompressed\":{decompressed}}}",
            match kind {
                SystemKind::PageMap => "page_map",
                SystemKind::SectionMap => "section_map",
            }
        ),
        MapStep::Page(p) => println!("{}", page_line(p.number, p.size, p.address)),
        MapStep::Sections(n) => println!("{{\"sections\":{n}}}"),
        MapStep::Section(s) => println!(
            "{}",
            section_line(
                s.index,
                &name(&s.name),
                [
                    s.size,
                    s.page_count.into(),
                    s.max_page_size.into(),
                    s.compressed.into(),
                    s.section_id.into(),
                    s.encrypted.into(),
                ],
            )
        ),
        MapStep::SectionPage(r) => println!(
            "{}",
            ref_line(r.section_id, r.page_number, r.data_size, r.start_offset)
        ),
        MapStep::Finished => println!("{{\"finished\":true}}"),
    }
}

fn legacy(path: &str) -> Result<()> {
    let bytes = fs::read(path)?;
    let header = R2004Header::parse(&bytes)?;
    let pages = section_map::parse_page_map(&bytes, &header)?;
    for p in &pages {
        println!("{}", page_line(p.number, p.size, p.file_offset));
    }
    let sections = section_map::parse_section_info(&bytes, &header, &pages)?;
    for (index, s) in sections.iter().enumerate() {
        println!(
            "{}",
            section_line(
                index as u32,
                &s.name,
                [
                    s.size,
                    s.page_count.into(),
                    s.max_decomp_page_size.into(),
                    s.compressed.into(),
                    s.section_id.into(),
                    s.encrypted.into(),
                ],
            )
        );
        for r in &s.pages {
            println!(
                "{}",
                ref_line(s.section_id, r.page_number, r.data_size, r.start_offset)
            );
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: stream_r2004_maps file.dwg [chunk-bytes] [options]")?;
    let mut chunks = vec![CHUNK_BYTES];
    let (mut trace, mut inject_pending) = (None, false);
    let (mut digest, mut checkpoint_out, mut stop_after, mut resume) = (None, None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--legacy" => return legacy(&path),
            "--trace" => trace = Some(args.next().ok_or("missing trace path")?),
            "--pending" => inject_pending = true,
            "--checkpoint-out" => checkpoint_out = Some(args.next().ok_or("missing path")?),
            "--stop-after" => {
                stop_after = Some(args.next().ok_or("missing steps")?.parse::<u64>()?)
            }
            "--resume" => resume = Some(args.next().ok_or("missing path")?),
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
            "--chunks" => {
                let value = args.next().ok_or("missing chunk schedule")?;
                if value.len() > 1024 {
                    return Err("chunk schedule too long".into());
                }
                chunks = value
                    .split(',')
                    .map(str::parse)
                    .collect::<std::result::Result<Vec<usize>, _>>()?;
            }
            value if !value.starts_with('-') => chunks = vec![value.parse()?],
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    if chunks.is_empty() || chunks.iter().any(|&n| n == 0 || n > CHUNK_BYTES) {
        return Err("chunk sizes must be 1..=256".into());
    }
    let file = File::open(&path)?;
    if (resume.is_some() || checkpoint_out.is_some()) && digest.is_none() {
        return Err("checkpointing requires --identity from a trusted immutable source".into());
    }
    if stop_after.is_some() != checkpoint_out.is_some() {
        return Err("--stop-after and --checkpoint-out go together".into());
    }
    let identity = SourceIdentity {
        digest: digest.unwrap_or([0; 32]),
        length: file.metadata()?.len(),
    };
    let (mut walker, mut steps) = match &resume {
        Some(path) => {
            let bytes = fs::read(path)?;
            let steps: u64 = fs::read_to_string(format!("{path}.caller"))?
                .trim()
                .parse()?;
            (R2004MapWalker::restore(&bytes, identity)?, steps)
        }
        None => (R2004MapWalker::new(identity), 0),
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
        if stop_after == Some(steps) {
            let out = checkpoint_out.as_deref().ok_or("missing checkpoint path")?;
            fs::write(out, walker.checkpoint()?.as_bytes())?;
            fs::write(format!("{out}.caller"), format!("{steps}\n"))?;
            println!("{{\"checkpoint\":true,\"steps\":{steps}}}");
            return Ok(());
        }
        let n = chunks[poll % chunks.len()];
        poll += 1;
        let before = walker.demand();
        match poll_r2004_maps(&mut walker, &mut source, &mut scratch[..n])? {
            MapPoll::Complete => return Ok(()),
            MapPoll::Pending => {
                if walker.demand() != before {
                    return Err("pending source mutated grammar".into());
                }
            }
            MapPoll::Fed { step, .. } | MapPoll::Pumped(step) => {
                steps += 1;
                print(step);
            }
        }
    }
}
