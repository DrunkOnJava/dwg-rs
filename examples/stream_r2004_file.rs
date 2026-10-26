//! Walks a whole R2004-family file through the one bounded walker: the
//! header, the page map and the section map, then every data page in the
//! order the page map lists them, tagged with its section and offset. Prints
//! one line per section, page and the end of the walk, none of which depends
//! on how the source was chunked, and with `--out` a `section_done` line per
//! section (its size and CRC-32, read back from the assembled file).
//! `stream_r2004_file file.dwg [chunk-bytes] [--chunks 1,7,3] [--pending]
//! [--trace <path>] [--out <dir>] [--report-steps] [--report-header]`.
//! `--report-header` prints the verified file header to stderr when the walk
//! ends, resumed or not. `--out` writes each
//! assembled section to `<dir>/<index>.bin` (sparse: holes are whatever no
//! page wrote). `--legacy` assembles the sections with the legacy reader and
//! prints the same `section_done` lines. `--identity <sha256>
//! --checkpoint-out <path> --stop-after <steps>` saves the walk after that
//! many steps; a fresh invocation with `--resume <path>` continues from the
//! checkpoint alone (the walker holds the section table and needs no page
//! index), with the cursors over the output saved in `<checkpoint>.caller`.

use dwg::byte_source::{ByteSource, FilePoll, FileSource, SourcePoll, poll_r2004_walk};
use dwg::crc::crc32;
use dwg::header::R2004Header;
use dwg::section_map;
use dwg::streaming::{CHUNK_BYTES, R2004Walker, Request, SourceIdentity, WalkStep};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
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

/// A section as the walker described it; the caller keeps these to name
/// the files and to print the end of the walk.
struct Section {
    index: u32,
    id: u32,
    size: u64,
    pages: u32,
    name: String,
}

/// What the caller keeps beside the walker's checkpoint.
struct Cursor {
    steps: u64,
    page_crc: u32,
    page_len: u64,
    number: u32,
}

fn file_crc(path: &Path) -> Result<u32> {
    let mut file = File::open(path)?;
    let (mut crc, mut buffer) = (0, [0u8; 65536]);
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            return Ok(crc);
        }
        crc = crc32(crc, &buffer[..n]);
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: stream_r2004_file file.dwg [chunk-bytes] [options]")?;
    let mut chunks = vec![CHUNK_BYTES];
    let (mut trace, mut inject_pending, mut out_dir, mut as_legacy) = (None, false, None, false);
    let (mut report_steps, mut report_header) = (false, false);
    let (mut digest, mut checkpoint_out, mut stop_after, mut resume) = (None, None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--legacy" => as_legacy = true,
            "--out" => out_dir = Some(PathBuf::from(args.next().ok_or("missing directory")?)),
            "--trace" => trace = Some(args.next().ok_or("missing trace path")?),
            "--pending" => inject_pending = true,
            "--report-steps" => report_steps = true,
            "--report-header" => report_header = true,
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
    let (mut walker, mut cursor, mut sections) = match &resume {
        Some(path) => {
            let saved = fs::read_to_string(format!("{path}.caller"))?;
            let (head, rest) = saved.split_once('\n').ok_or("caller state is empty")?;
            let n: Vec<&str> = head.split(' ').collect();
            let cursor = Cursor {
                steps: n[0].parse()?,
                page_crc: n[1].parse()?,
                page_len: n[2].parse()?,
                number: n[3].parse()?,
            };
            let mut sections = Vec::new();
            for line in rest.lines() {
                let f: Vec<&str> = line.splitn(6, ' ').collect();
                sections.push(Section {
                    index: f[1].parse()?,
                    id: f[2].parse()?,
                    size: f[3].parse()?,
                    pages: f[4].parse()?,
                    name: f.get(5).copied().unwrap_or("").to_owned(),
                });
            }
            (
                R2004Walker::restore(&fs::read(path)?, identity)?,
                cursor,
                sections,
            )
        }
        None => (
            R2004Walker::new(identity),
            Cursor {
                steps: 0,
                page_crc: 0,
                page_len: 0,
                number: 0,
            },
            Vec::new(),
        ),
    };
    let open = |dir: &Path, index: u32, truncate: bool| -> Result<File> {
        Ok(OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(truncate)
            .open(dir.join(format!("{index}.bin")))?)
    };
    let mut files: HashMap<u32, File> = HashMap::new();
    let mut scratch = [0; CHUNK_BYTES];
    let mut poll = 0;
    loop {
        if stop_after == Some(cursor.steps) {
            let to = checkpoint_out.as_deref().ok_or("missing checkpoint path")?;
            fs::write(to, walker.checkpoint()?.as_bytes())?;
            let mut text = format!(
                "{} {} {} {}\n",
                cursor.steps, cursor.page_crc, cursor.page_len, cursor.number
            );
            for s in &sections {
                text += &format!("S {} {} {} {} {}\n", s.index, s.id, s.size, s.pages, s.name);
            }
            fs::write(format!("{to}.caller"), text)?;
            println!("{{\"checkpoint\":true,\"steps\":{}}}", cursor.steps);
            return Ok(());
        }
        let n = chunks[poll % chunks.len()];
        poll += 1;
        let before = walker.demand();
        match poll_r2004_walk(&mut walker, &mut source, &mut scratch[..n])? {
            FilePoll::Complete => break,
            FilePoll::Pending => {
                if walker.demand() != before {
                    return Err("pending source mutated grammar".into());
                }
            }
            FilePoll::Fed { step, .. } | FilePoll::Pumped(step) => {
                cursor.steps += 1;
                if report_steps {
                    let event = match step {
                        WalkStep::Section(_) => Some("section"),
                        WalkStep::Page(_) => Some("page"),
                        WalkStep::PageEnd { .. } => Some("page_end"),
                        WalkStep::Finished { .. } => Some("finished"),
                        _ => None,
                    };
                    if let Some(event) = event {
                        eprintln!("{{\"step\":{},\"event\":\"{event}\"}}", cursor.steps);
                    }
                }
                match step {
                    WalkStep::Framing | WalkStep::Header(_) => (),
                    WalkStep::Section(s) => {
                        let label = name(&s.name);
                        println!(
                            "{{\"section\":{{\"index\":{},\"name\":\"{}\",\"size\":{},\"pages\":{},\"id\":{}}}}}",
                            s.index,
                            esc(&label),
                            s.size,
                            s.page_count,
                            s.section_id
                        );
                        // A section described by this process is new: its
                        // file starts empty, whether or not the walk was resumed.
                        if let Some(dir) = &out_dir {
                            files.insert(s.section_id, open(dir, s.index, true)?);
                        }
                        sections.push(Section {
                            index: s.index,
                            id: s.section_id,
                            size: s.size,
                            pages: s.page_count,
                            name: label,
                        });
                    }
                    WalkStep::Page(p) => {
                        cursor.page_crc = 0;
                        cursor.page_len = 0;
                        cursor.number = p.page_number;
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
                    WalkStep::Data {
                        section_id,
                        offset,
                        len,
                    } => {
                        let bytes = walker.data();
                        if bytes.len() != len {
                            return Err("data step and data() disagree".into());
                        }
                        cursor.page_crc = crc32(cursor.page_crc, bytes);
                        cursor.page_len += len as u64;
                        if let Some(dir) = &out_dir {
                            if let Entry::Vacant(slot) = files.entry(section_id) {
                                let index = sections
                                    .iter()
                                    .find(|s| s.id == section_id)
                                    .ok_or("data for a section that was not described")?
                                    .index;
                                slot.insert(open(dir, index, false)?);
                            }
                            let file = files.get_mut(&section_id).ok_or("section file")?;
                            file.seek(SeekFrom::Start(offset))?;
                            file.write_all(bytes)?;
                        }
                    }
                    WalkStep::PageEnd {
                        decompressed,
                        emitted,
                    } => {
                        if u64::from(emitted) != cursor.page_len {
                            return Err("page end disagrees with the bytes handed out".into());
                        }
                        println!(
                            "{{\"page_end\":{{\"number\":{},\"decompressed\":{decompressed},\"emitted\":{emitted},\"crc32\":{}}}}}",
                            cursor.number, cursor.page_crc
                        );
                    }
                    WalkStep::Finished { sections: n, pages } => {
                        println!("{{\"finished\":{{\"sections\":{n},\"pages\":{pages}}}}}");
                    }
                }
            }
        }
    }
    if report_steps {
        eprintln!("{{\"steps\":{}}}", cursor.steps);
    }
    if report_header {
        let h = walker
            .header()
            .ok_or("the walk ended without a verified header")?;
        eprintln!(
            "{{\"header\":\"{}\",\"page_map_id\":{},\"page_map_address\":{},\"section_map_id\":{}}}",
            String::from_utf8_lossy(&h.magic),
            h.page_map_id,
            h.page_map_address,
            h.section_map_id
        );
    }
    if let Some(dir) = &out_dir {
        for s in &sections {
            let path = dir.join(format!("{}.bin", s.index));
            if let Entry::Vacant(slot) = files.entry(s.id) {
                slot.insert(open(dir, s.index, false)?);
            }
            files.get(&s.id).ok_or("section file")?.set_len(s.size)?;
            println!(
                "{}",
                done_line(s.index, &s.name, s.size, s.pages, file_crc(&path)?)
            );
        }
    }
    Ok(())
}
