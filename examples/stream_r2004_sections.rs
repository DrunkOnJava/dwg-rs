//! Assembles every section of an R2004-family file from its data pages
//! (§4.6) through the bounded streaming walkers: the page map and section
//! map walk first (the page index and section list are this caller's),
//! then one `R2004SectionWalker` per section, offered each page in turn.
//! Prints one JSON line per page and section, none that depend on how
//! the source was chunked.
//! `stream_r2004_sections file.dwg [chunk-bytes] [--chunks 1,7,3]
//! [--pending] [--trace <path>] [--out <dir>] [--report-steps]`.
//! `--report-steps` prints the data-page walk's step count to stderr after
//! each page, hole and section, and in total. `--out` writes each
//! assembled section to `<dir>/<index>.bin`. `--legacy` assembles the
//! sections with the legacy reader and prints the same `section_done`
//! lines. `--identity <sha256> --checkpoint-out <path> --stop-after <steps>`
//! saves the data-page walk after that many of its steps (feeds and pumps;
//! the map walk before it is not counted and not interrupted); a fresh
//! invocation with `--resume <path>` continues from the checkpoint and
//! the page index saved beside it.

use dwg::byte_source::{
    ByteSource, FileSource, MapPoll, SectionPoll, SourcePoll, poll_r2004_maps, poll_r2004_section,
};
use dwg::crc::crc32;
use dwg::header::R2004Header;
use dwg::section_map;
use dwg::streaming::{
    CHUNK_BYTES, MapStep, PageEntry, R2004MapWalker, R2004SectionWalker, Request, SectionEntry,
    SectionPage, SectionStep, SourceIdentity,
};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

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

/// A name as the body of a JSON string. The map walker admits only printable
/// ASCII, so the quote and the backslash are all that need escaping.
fn esc(name: &str) -> String {
    name.replace('\\', "\\\\").replace('"', "\\\"")
}

fn done_line(index: u32, name: &str, size: u64, pages: u32, crc: u32) -> String {
    format!(
        "{{\"section_done\":{{\"index\":{index},\"name\":\"{}\",\"size\":{size},\"pages\":{pages},\"crc32\":{crc}}}}}",
        esc(name)
    )
}

fn zeros_crc(mut crc: u32, mut len: u64) -> u32 {
    let zeros = [0u8; 4096];
    while len > 0 {
        let n = len.min(zeros.len() as u64) as usize;
        crc = crc32(crc, &zeros[..n]);
        len -= n as u64;
    }
    crc
}

fn legacy(path: &str, out: Option<&PathBuf>) -> Result<()> {
    let bytes = fs::read(path)?;
    let header = R2004Header::parse(&bytes)?;
    let pages = section_map::parse_page_map(&bytes, &header)?;
    let sections = section_map::parse_section_info(&bytes, &header, &pages)?;
    for (index, s) in sections.iter().enumerate() {
        let data = section_map::read_section_payload(&bytes, &pages, s)?;
        if let Some(dir) = out {
            fs::write(dir.join(format!("{index}.bin")), &data)?;
        }
        println!(
            "{}",
            done_line(index as u32, &s.name, s.size, s.page_count, crc32(0, &data))
        );
    }
    Ok(())
}

/// Everything the map walk produced that the section walkers are offered.
#[derive(Default)]
struct Index {
    pages: HashMap<u32, PageEntry>,
    sections: Vec<(SectionEntry, Vec<SectionPage>)>,
}

impl Index {
    fn save(&self) -> String {
        let mut text = String::new();
        let mut numbers: Vec<_> = self.pages.values().collect();
        numbers.sort_by_key(|p| p.address);
        for p in numbers {
            text += &format!("P {} {} {}\n", p.number, p.size, p.address);
        }
        for (s, refs) in &self.sections {
            text += &format!(
                "S {} {} {} {} {} {} {} {}\n",
                s.index,
                s.size,
                s.page_count,
                s.max_page_size,
                s.compressed,
                s.section_id,
                s.encrypted,
                name(&s.name)
            );
            for r in refs {
                text += &format!(
                    "R {} {} {} {}\n",
                    r.section_id, r.page_number, r.data_size, r.start_offset
                );
            }
        }
        text
    }

    fn load(text: &str) -> Result<Self> {
        let mut index = Index::default();
        for line in text.lines() {
            let mut f = line.split(' ');
            let kind = f.next().ok_or("empty index line")?;
            let n: Vec<&str> = f.collect();
            match kind {
                "P" => {
                    let p = PageEntry {
                        number: n[0].parse()?,
                        size: n[1].parse()?,
                        address: n[2].parse()?,
                    };
                    index.pages.entry(p.number as u32).or_insert(p);
                }
                "S" => {
                    let mut label = [0u8; 64];
                    let text = n.get(7).copied().unwrap_or("").as_bytes();
                    label[..text.len()].copy_from_slice(text);
                    index.sections.push((
                        SectionEntry {
                            index: n[0].parse()?,
                            size: n[1].parse()?,
                            page_count: n[2].parse()?,
                            max_page_size: n[3].parse()?,
                            compressed: n[4].parse()?,
                            section_id: n[5].parse()?,
                            encrypted: n[6].parse()?,
                            name: label,
                        },
                        Vec::new(),
                    ));
                }
                "R" => index
                    .sections
                    .last_mut()
                    .ok_or("reference before its section")?
                    .1
                    .push(SectionPage {
                        section_id: n[0].parse()?,
                        page_number: n[1].parse()?,
                        data_size: n[2].parse()?,
                        start_offset: n[3].parse()?,
                    }),
                _ => return Err("unknown index line".into()),
            }
        }
        Ok(index)
    }
}

fn map_walk(
    source: &mut TracedSource,
    identity: SourceIdentity,
    chunks: &[usize],
) -> Result<Index> {
    let mut walker = R2004MapWalker::new(identity);
    let mut index = Index::default();
    let mut scratch = [0; CHUNK_BYTES];
    let mut poll = 0;
    loop {
        let n = chunks[poll % chunks.len()];
        poll += 1;
        match poll_r2004_maps(&mut walker, source, &mut scratch[..n])? {
            MapPoll::Complete => return Ok(index),
            MapPoll::Pending => (),
            MapPoll::Fed { step, .. } | MapPoll::Pumped(step) => match step {
                MapStep::Page(p) if p.number > 0 => {
                    index.pages.entry(p.number as u32).or_insert(p);
                }
                MapStep::Section(s) => index.sections.push((s, Vec::new())),
                MapStep::SectionPage(r) => index
                    .sections
                    .last_mut()
                    .ok_or("reference before its section")?
                    .1
                    .push(r),
                _ => (),
            },
        }
    }
}

/// What the caller keeps beside a section walker's checkpoint.
struct Cursor {
    steps: u64,
    section: usize,
    next_ref: usize,
    page_crc: u32,
    page_len: u64,
    section_crc: u32,
}

fn open_section(dir: &Path, index: u32, truncate: bool) -> Result<File> {
    Ok(OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(truncate)
        .open(dir.join(format!("{index}.bin")))?)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: stream_r2004_sections file.dwg [chunk-bytes] [options]")?;
    let mut chunks = vec![CHUNK_BYTES];
    let (mut trace, mut inject_pending, mut out_dir, mut as_legacy) = (None, false, None, false);
    let mut report_steps = false;
    let (mut digest, mut checkpoint_out, mut stop_after, mut resume) = (None, None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--legacy" => as_legacy = true,
            "--out" => out_dir = Some(PathBuf::from(args.next().ok_or("missing directory")?)),
            "--trace" => trace = Some(args.next().ok_or("missing trace path")?),
            "--pending" => inject_pending = true,
            "--report-steps" => report_steps = true,
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
    if as_legacy {
        return legacy(&path, out_dir.as_ref());
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
    let mut source = TracedSource {
        file: FileSource::new(file),
        trace: trace.map(File::create).transpose()?,
        inject_pending,
        pending: false,
    };
    let (index, mut cursor, mut restored) = match &resume {
        Some(path) => {
            let saved = fs::read_to_string(format!("{path}.caller"))?;
            let (head, rest) = saved.split_once('\n').ok_or("caller state is empty")?;
            let n: Vec<&str> = head.split(' ').collect();
            let cursor = Cursor {
                steps: n[0].parse()?,
                section: n[1].parse()?,
                next_ref: n[2].parse()?,
                page_crc: n[3].parse()?,
                page_len: n[4].parse()?,
                section_crc: n[5].parse()?,
            };
            let walker = R2004SectionWalker::restore(&fs::read(path)?, identity)?;
            (Index::load(rest)?, cursor, Some(walker))
        }
        None => (
            map_walk(&mut source, identity, &chunks)?,
            Cursor {
                steps: 0,
                section: 0,
                next_ref: 0,
                page_crc: 0,
                page_len: 0,
                section_crc: 0,
            },
            None,
        ),
    };
    let mut scratch = [0; CHUNK_BYTES];
    let mut poll = 0;
    while cursor.section < index.sections.len() {
        let (entry, refs) = &index.sections[cursor.section];
        let label = name(&entry.name);
        let (mut walker, fresh) = match restored.take() {
            Some(walker) => (walker, false),
            None => {
                cursor.next_ref = 0;
                cursor.section_crc = 0;
                (R2004SectionWalker::new(identity, *entry)?, true)
            }
        };
        let mut out = out_dir
            .as_ref()
            .map(|dir| open_section(dir, entry.index, fresh))
            .transpose()?;
        loop {
            if stop_after == Some(cursor.steps) {
                let to = checkpoint_out.as_deref().ok_or("missing checkpoint path")?;
                fs::write(to, walker.checkpoint()?.as_bytes())?;
                fs::write(
                    format!("{to}.caller"),
                    format!(
                        "{} {} {} {} {} {}\n{}",
                        cursor.steps,
                        cursor.section,
                        cursor.next_ref,
                        cursor.page_crc,
                        cursor.page_len,
                        cursor.section_crc,
                        index.save()
                    ),
                )?;
                println!("{{\"checkpoint\":true,\"steps\":{}}}", cursor.steps);
                return Ok(());
            }
            let n = chunks[poll % chunks.len()];
            poll += 1;
            let before = walker.demand();
            match poll_r2004_section(&mut walker, &mut source, &mut scratch[..n])? {
                SectionPoll::Complete => break,
                SectionPoll::Pending => {
                    if walker.demand() != before {
                        return Err("pending source mutated grammar".into());
                    }
                }
                SectionPoll::NextPage => {
                    let reference = *refs
                        .get(cursor.next_ref)
                        .ok_or("walker wants a page the section map does not list")?;
                    let page = *index.pages.get(&reference.page_number).ok_or_else(|| {
                        format!(
                            "section {label:?} references page {} not in page map",
                            reference.page_number
                        )
                    })?;
                    walker.begin_page(page, reference)?;
                    cursor.next_ref += 1;
                }
                SectionPoll::Fed { step, .. } | SectionPoll::Pumped(step) => {
                    cursor.steps += 1;
                    if report_steps {
                        let event = match step {
                            SectionStep::Page(_) => Some("page"),
                            SectionStep::Hole { .. } => Some("hole"),
                            SectionStep::PageEnd { .. } => Some("page_end"),
                            SectionStep::Finished { .. } => Some("section_done"),
                            _ => None,
                        };
                        if let Some(event) = event {
                            eprintln!("{{\"step\":{},\"event\":\"{event}\"}}", cursor.steps);
                        }
                    }
                    match step {
                        SectionStep::Framing => (),
                        SectionStep::Page(p) => {
                            cursor.page_crc = 0;
                            cursor.page_len = 0;
                            println!(
                                "{{\"page\":{{\"section\":{},\"number\":{},\"address\":{},\"start\":{},\"data\":{},\"page_size\":{}}}}}",
                                p.section_id,
                                p.page_number,
                                p.address,
                                p.start_offset,
                                p.data_size,
                                p.page_size
                            );
                        }
                        SectionStep::Hole { offset, len } => {
                            cursor.section_crc = zeros_crc(cursor.section_crc, len);
                            println!(
                                "{{\"hole\":{{\"section\":{},\"offset\":{offset},\"len\":{len}}}}}",
                                entry.section_id
                            );
                        }
                        SectionStep::Data { offset, len } => {
                            let bytes = walker.data();
                            if bytes.len() != len {
                                return Err("data step and data() disagree".into());
                            }
                            cursor.page_crc = crc32(cursor.page_crc, bytes);
                            cursor.page_len += len as u64;
                            cursor.section_crc = crc32(cursor.section_crc, bytes);
                            if let Some(file) = out.as_mut() {
                                file.seek(SeekFrom::Start(offset))?;
                                file.write_all(bytes)?;
                            }
                        }
                        SectionStep::PageEnd {
                            decompressed,
                            emitted,
                        } => {
                            if u64::from(emitted) != cursor.page_len {
                                return Err("page end disagrees with the bytes handed out".into());
                            }
                            println!(
                                "{{\"page_end\":{{\"section\":{},\"number\":{},\"decompressed\":{decompressed},\"emitted\":{emitted},\"crc32\":{}}}}}",
                                entry.section_id,
                                refs[cursor.next_ref - 1].page_number,
                                cursor.page_crc
                            );
                        }
                        SectionStep::Finished { size, pages } => {
                            if let Some(file) = out.as_mut() {
                                file.set_len(size)?;
                            }
                            println!(
                                "{}",
                                done_line(entry.index, &label, size, pages, cursor.section_crc)
                            );
                        }
                    }
                }
            }
        }
        cursor.section += 1;
    }
    if report_steps {
        eprintln!("{{\"steps\":{}}}", cursor.steps);
    }
    Ok(())
}
