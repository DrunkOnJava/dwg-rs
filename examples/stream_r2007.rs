//! Walks a whole R2007 (`AC1021`) file through the one bounded walker: the
//! file header, the page map, the section map, then every section's data
//! pages in the order the section lists them, each page's bytes handed out
//! only after its codewords and checksums passed. Prints one line per page
//! map entry, section, page, hole and the end of the walk, none of which
//! depends on how the source was chunked, and with `--out` a `section_done`
//! line per section (its size and CRC-32, read back from the assembled file).
//! `stream_r2007 file.dwg [chunk-bytes] [--chunks 1,7,3] [--pending]
//! [--would-block] [--trace <path>] [--out <dir>] [--report-steps]`.
//! `--would-block` puts a reader under `FileSource` that fails every other
//! read with `WouldBlock` and returns at most 512 bytes. `--legacy` assembles the
//! same sections with the legacy reader (`r2007::Container`) and prints the
//! same lines. `--identity <sha256> --checkpoint-out <path> --stop-after
//! <steps>` saves the walk after that many steps; a fresh invocation with
//! `--resume <path>` continues from the checkpoint alone, with the cursors
//! over the output saved in `<checkpoint>.caller`. `--trace` records each read
//! or gather the grammar demanded as `{offset, stride, bytes}`.

use dwg::byte_source::{ByteSource, FileSource, R2007Poll, SourcePoll, poll_r2007_walk};
use dwg::crc::crc32;
use dwg::r2007::{self, Container};
use dwg::streaming::{CHUNK_BYTES, R2007Step, R2007Walker, Request, SourceIdentity};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A file that, when `flaky`, refuses every other read with `WouldBlock`
/// and returns at most 512 bytes from the others.
#[derive(Debug)]
struct Flaky {
    file: File,
    flaky: bool,
    blocked: bool,
}

impl Read for Flaky {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.flaky {
            return self.file.read(buf);
        }
        self.blocked = !self.blocked;
        if self.blocked {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = buf.len().min(512);
        self.file.read(&mut buf[..n])
    }
}

impl Seek for Flaky {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}

#[derive(Debug)]
struct TracedSource {
    file: FileSource<Flaky>,
    trace: Option<File>,
    inject_pending: bool,
    pending: bool,
}

impl TracedSource {
    fn pend(&mut self) -> bool {
        if self.inject_pending && !self.pending {
            self.pending = true;
            return true;
        }
        self.pending = false;
        false
    }

    fn note(&mut self, offset: u64, stride: u64, poll: &SourcePoll) -> io::Result<()> {
        if let (SourcePoll::Ready(n), Some(trace)) = (poll, self.trace.as_mut()) {
            writeln!(
                trace,
                "{{\"offset\":{offset},\"stride\":{stride},\"bytes\":{n}}}"
            )?;
        }
        Ok(())
    }
}

impl ByteSource for TracedSource {
    type Error = io::Error;
    fn poll_read(&mut self, request: Request, output: &mut [u8]) -> io::Result<SourcePoll> {
        if self.pend() {
            return Ok(SourcePoll::Pending);
        }
        let result = self.file.poll_read(request, output)?;
        self.note(request.offset, 1, &result)?;
        Ok(result)
    }

    fn poll_gather(
        &mut self,
        offset: u64,
        stride: u64,
        output: &mut [u8],
    ) -> io::Result<SourcePoll> {
        if self.pend() {
            return Ok(SourcePoll::Pending);
        }
        let result = self.file.poll_gather(offset, stride, output)?;
        self.note(offset, stride, &result)?;
        Ok(result)
    }
}

fn name(units: impl Iterator<Item = u16>) -> String {
    char::decode_utf16(units)
        .map(|c| c.unwrap_or('\u{fffd}'))
        .collect()
}

fn esc(name: &str) -> String {
    name.replace('\\', "\\\\").replace('"', "\\\"")
}

fn header_line(fields: &[u64; 34]) -> String {
    let list: Vec<String> = fields.iter().map(u64::to_string).collect();
    format!("{{\"header\":[{}]}}", list.join(","))
}

fn entry_line(id: i64, size: i64, offset: u64) -> String {
    format!("{{\"page_entry\":{{\"id\":{id},\"size\":{size},\"offset\":{offset}}}}}")
}

#[allow(clippy::too_many_arguments)]
fn section_line(
    index: u32,
    label: &str,
    size: u64,
    max: u64,
    encryption: u64,
    hash: u64,
    encoding: u64,
    pages: u64,
) -> String {
    format!(
        "{{\"section\":{{\"index\":{index},\"name\":\"{}\",\"size\":{size},\"max\":{max},\"encryption\":{encryption},\"hash\":{hash},\"encoding\":{encoding},\"pages\":{pages}}}}}",
        esc(label)
    )
}

#[allow(clippy::too_many_arguments)]
fn page_line(
    section: u32,
    id: i64,
    address: u64,
    data_offset: u64,
    size: u64,
    unc: u64,
    comp: u64,
    checksum: u64,
    crc: u64,
    codewords: u64,
) -> String {
    format!(
        "{{\"page\":{{\"section\":{section},\"id\":{id},\"address\":{address},\"data_offset\":{data_offset},\"size\":{size},\"uncompressed\":{unc},\"compressed\":{comp},\"checksum\":{checksum},\"crc\":{crc},\"codewords\":{codewords}}}}}"
    )
}

fn page_end_line(section: u32, id: i64, plain: u64, emitted: u64, crc: u32) -> String {
    format!(
        "{{\"page_end\":{{\"section\":{section},\"id\":{id},\"plain\":{plain},\"emitted\":{emitted},\"crc32\":{crc}}}}}"
    )
}

fn hole_line(section: u32, offset: u64, len: u64) -> String {
    format!("{{\"hole\":{{\"section\":{section},\"offset\":{offset},\"len\":{len}}}}}")
}

fn section_end_line(index: u32, covered: u64) -> String {
    format!("{{\"section_end\":{{\"index\":{index},\"covered\":{covered}}}}}")
}

fn done_line(index: u32, label: &str, size: u64, pages: u64, crc: u32) -> String {
    format!(
        "{{\"section_done\":{{\"index\":{index},\"name\":\"{}\",\"size\":{size},\"pages\":{pages},\"crc32\":{crc}}}}}",
        esc(label)
    )
}

fn finished_line(sections: u32, pages: u32) -> String {
    format!("{{\"finished\":{{\"sections\":{sections},\"pages\":{pages}}}}}")
}

fn legacy(path: &str, out: Option<&PathBuf>) -> Result<()> {
    let bytes = fs::read(path)?;
    let c = Container::parse(&bytes)?;
    let h = &c.header;
    println!(
        "{}",
        header_line(&[
            h.header_size,
            h.file_size,
            h.pages_map_crc_compressed,
            h.pages_map_correction_factor,
            h.pages_map_crc_seed,
            h.pages_map2_offset,
            h.pages_map2_id,
            h.pages_map_offset,
            h.pages_map_id,
            h.header2_offset,
            h.pages_map_size_compressed,
            h.pages_map_size_uncompressed,
            h.pages_amount,
            h.pages_max_id,
            h.unknown_0x70,
            h.unknown_0x78,
            h.pages_map_crc_uncompressed,
            h.unknown_0x88,
            h.unknown_0x90,
            h.unknown_0x98,
            h.sections_amount,
            h.sections_map_crc_uncompressed,
            h.sections_map_size_compressed,
            h.sections_map2_id,
            h.sections_map_id,
            h.sections_map_size_uncompressed,
            h.sections_map_crc_compressed,
            h.sections_map_correction_factor,
            h.sections_map_crc_seed,
            h.stream_version,
            h.crc_seed,
            h.crc_seed_encoded,
            h.random_seed,
            h.header_crc64,
        ])
    );
    for e in &c.page_map.entries {
        println!("{}", entry_line(e.id, e.size, e.offset));
    }
    let mut pages = 0u32;
    for (index, s) in c.section_map.sections.iter().enumerate() {
        let index = index as u32;
        println!(
            "{}",
            section_line(
                index,
                &s.name,
                s.data_size,
                s.max_size,
                s.encryption,
                s.hash_code,
                s.encoding,
                s.pages.len() as u64
            )
        );
        let data = c.read_section(&bytes, &s.name)?;
        let covered = data.len() as u64;
        let mut end = 0u64;
        for p in &s.pages {
            let address = c
                .page_map
                .file_offset_of(p.id)
                .ok_or("page the page map does not list")? as u64;
            let rs = s.encoding == 4;
            let codewords = if rs {
                p.compressed.div_ceil(251).max(1)
            } else {
                0
            };
            println!(
                "{}",
                page_line(
                    index,
                    p.id,
                    address,
                    p.data_offset,
                    p.size,
                    p.uncompressed,
                    p.compressed,
                    p.checksum,
                    p.crc,
                    codewords
                )
            );
            let from = end.min(s.data_size);
            let to = p.data_offset.min(s.data_size);
            if to > from {
                println!("{}", hole_line(index, from, to - from));
            }
            let emitted = if p.data_offset >= covered {
                0
            } else {
                p.uncompressed.min(covered - p.data_offset)
            };
            let slice = &data[p.data_offset.min(covered) as usize..][..emitted as usize];
            println!(
                "{}",
                page_end_line(index, p.id, p.uncompressed, emitted, crc32(0, slice))
            );
            end = p.data_offset + p.uncompressed;
            pages += 1;
        }
        println!("{}", section_end_line(index, covered));
        if let Some(dir) = out {
            fs::write(dir.join(format!("{index}.bin")), &data)?;
            println!(
                "{}",
                done_line(
                    index,
                    &s.name,
                    covered,
                    s.pages.len() as u64,
                    crc32(0, &data)
                )
            );
        }
    }
    println!(
        "{}",
        finished_line(c.section_map.sections.len() as u32, pages)
    );
    let _ = r2007::PAGE_BASE;
    Ok(())
}

/// A section as the walker described it; the caller keeps these to name
/// the files and to print the end of the walk.
struct Section {
    index: u32,
    pages: u64,
    covered: u64,
    name: String,
}

/// What the caller keeps beside the walker's checkpoint.
struct Cursor {
    steps: u64,
    page_crc: u32,
    page_len: u64,
    section: u32,
    id: i64,
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
        .ok_or("usage: stream_r2007 file.dwg [chunk-bytes] [options]")?;
    let mut chunks = vec![CHUNK_BYTES];
    let (mut trace, mut inject_pending, mut out_dir, mut as_legacy) = (None, false, None, false);
    let (mut report_steps, mut would_block) = (false, false);
    let (mut digest, mut checkpoint_out, mut stop_after, mut resume) = (None, None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--legacy" => as_legacy = true,
            "--out" => out_dir = Some(PathBuf::from(args.next().ok_or("missing directory")?)),
            "--trace" => trace = Some(args.next().ok_or("missing trace path")?),
            "--pending" => inject_pending = true,
            "--would-block" => would_block = true,
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
        file: FileSource::new(Flaky {
            file,
            flaky: would_block,
            blocked: false,
        }),
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
                section: n[3].parse()?,
                id: n[4].parse()?,
            };
            let mut sections = Vec::new();
            for line in rest.lines() {
                let f: Vec<&str> = line.splitn(5, ' ').collect();
                sections.push(Section {
                    index: f[1].parse()?,
                    pages: f[2].parse()?,
                    covered: f[3].parse()?,
                    name: f.get(4).copied().unwrap_or("").to_owned(),
                });
            }
            (
                R2007Walker::restore(&fs::read(path)?, identity)?,
                cursor,
                sections,
            )
        }
        None => (
            R2007Walker::new(identity),
            Cursor {
                steps: 0,
                page_crc: 0,
                page_len: 0,
                section: 0,
                id: 0,
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
                "{} {} {} {} {}\n",
                cursor.steps, cursor.page_crc, cursor.page_len, cursor.section, cursor.id
            );
            for s in &sections {
                text += &format!("S {} {} {} {}\n", s.index, s.pages, s.covered, s.name);
            }
            fs::write(format!("{to}.caller"), text)?;
            println!("{{\"checkpoint\":true,\"steps\":{}}}", cursor.steps);
            return Ok(());
        }
        let n = chunks[poll % chunks.len()];
        poll += 1;
        let before = walker.demand();
        let step = match poll_r2007_walk(&mut walker, &mut source, &mut scratch[..n])? {
            R2007Poll::Complete => break,
            R2007Poll::Pending => {
                if walker.demand() != before {
                    return Err("pending source mutated grammar".into());
                }
                continue;
            }
            R2007Poll::Fed { step, .. } | R2007Poll::Pumped(step) => step,
        };
        cursor.steps += 1;
        if report_steps {
            let event = match step {
                R2007Step::Section(_) => Some("section"),
                R2007Step::Page(_) => Some("page"),
                R2007Step::Hole { .. } => Some("hole"),
                R2007Step::PageEnd { .. } => Some("page_end"),
                R2007Step::SectionEnd { .. } => Some("section_end"),
                R2007Step::Finished { .. } => Some("finished"),
                _ => None,
            };
            if let Some(event) = event {
                eprintln!("{{\"step\":{},\"event\":\"{event}\"}}", cursor.steps);
            }
        }
        match step {
            R2007Step::Framing => (),
            R2007Step::Header(h) => println!("{}", header_line(&h.fields)),
            R2007Step::Entry(e) => println!("{}", entry_line(e.id, e.size, e.offset)),
            R2007Step::Section(s) => {
                let label = name(s.name_units());
                println!(
                    "{}",
                    section_line(
                        s.index,
                        &label,
                        s.size,
                        s.max_size,
                        s.encryption,
                        s.hash_code,
                        s.encoding,
                        s.page_count
                    )
                );
                // A section described by this process is new: its file
                // starts empty, whether or not the walk was resumed.
                if let Some(dir) = &out_dir {
                    files.insert(s.index, open(dir, s.index, true)?);
                }
                cursor.section = s.index;
                sections.push(Section {
                    index: s.index,
                    pages: 0,
                    covered: 0,
                    name: label,
                });
            }
            R2007Step::Page(p) => {
                cursor.page_crc = 0;
                cursor.page_len = 0;
                cursor.id = p.id;
                println!(
                    "{}",
                    page_line(
                        p.section,
                        p.id,
                        p.address,
                        p.data_offset,
                        p.size,
                        p.uncompressed,
                        p.compressed,
                        p.checksum,
                        p.crc,
                        p.codewords
                    )
                );
            }
            R2007Step::Hole { offset, len } => {
                println!("{}", hole_line(cursor.section, offset, len));
            }
            R2007Step::Data { offset, len } => {
                let bytes = walker.data();
                if bytes.len() != len {
                    return Err("data step and data() disagree".into());
                }
                cursor.page_crc = crc32(cursor.page_crc, bytes);
                cursor.page_len += len as u64;
                if let Some(dir) = &out_dir {
                    if let Entry::Vacant(slot) = files.entry(cursor.section) {
                        slot.insert(open(dir, cursor.section, false)?);
                    }
                    let file = files.get_mut(&cursor.section).ok_or("section file")?;
                    file.seek(SeekFrom::Start(offset))?;
                    file.write_all(bytes)?;
                }
            }
            R2007Step::PageEnd { plain, emitted } => {
                if emitted != cursor.page_len {
                    return Err("page end disagrees with the bytes handed out".into());
                }
                println!(
                    "{}",
                    page_end_line(cursor.section, cursor.id, plain, emitted, cursor.page_crc)
                );
                if let Some(s) = sections.iter_mut().find(|s| s.index == cursor.section) {
                    s.pages += 1;
                }
            }
            R2007Step::SectionEnd { index, covered } => {
                println!("{}", section_end_line(index, covered));
                if let Some(s) = sections.iter_mut().find(|s| s.index == index) {
                    s.covered = covered;
                }
            }
            R2007Step::Finished { sections: n, pages } => {
                println!("{}", finished_line(n, pages));
            }
        }
    }
    if report_steps {
        eprintln!("{{\"steps\":{}}}", cursor.steps);
    }
    if let Some(dir) = &out_dir {
        for s in &sections {
            let path = dir.join(format!("{}.bin", s.index));
            if let Entry::Vacant(slot) = files.entry(s.index) {
                slot.insert(open(dir, s.index, false)?);
            }
            files
                .get(&s.index)
                .ok_or("section file")?
                .set_len(s.covered)?;
            println!(
                "{}",
                done_line(s.index, &s.name, s.covered, s.pages, file_crc(&path)?)
            );
        }
    }
    Ok(())
}
