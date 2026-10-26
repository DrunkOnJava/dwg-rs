//! Runs the bounded §4.7 LZ77 decoder over every compressed page of an
//! R2004-family DWG (the §4.3 page map and section map, then every page
//! of every LZ77 section, §4.5) and prints one JSON line per page: where
//! its compressed bytes are, how many the decoder consumed up to the 0x11
//! terminator, how many it produced and their CRC-32.
//!
//! `stream_lz77 file.dwg [--chunks 1,7,256] [--out 1,4096]` sets the input
//! and output slice schedules. `--legacy` decodes the same pages with the
//! crate's whole-buffer `lz77::decompress` and prints the same lines
//! without `consumed`. The page list comes from the legacy section map;
//! only the decoding is under test.
//!
//! `--page <n>` decodes one page. With `--checkpoint-out <path>
//! --stop-after <calls>` it saves the decoder after that many `decode`
//! calls, plus the caller's own state (input position, unconsumed input,
//! output CRC and schedule position) in `<path>.caller`; a fresh
//! invocation with `--resume <path>` continues. `--trace <path>` records
//! actual source reads. `--truncate <bytes>` delivers only that many
//! compressed bytes and `--limit <bytes>` overrides the declared output
//! size, to exercise the typed truncation and overrun errors.
//! `--decode-checkpoint <path> --input <hex>` restores a decoder
//! checkpoint and feeds it those bytes directly, with no page or caller
//! state, so a forged checkpoint reaches the decoder itself. A bare
//! number is a one-size input schedule and `--pending` makes the source
//! report Pending before every read, the shape `streaming_schedules`
//! drives.

use dwg::byte_source::{ByteSource, FileSource, SourcePoll};
use dwg::crc::crc32;
use dwg::header::R2004Header;
use dwg::section_map::{self, DataPageHeader};
use dwg::streaming::{Lz77Decoder, Request};
use std::fs::{self, File};
use std::io::{self, Write};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone)]
struct Page {
    kind: &'static str,
    section: String,
    offset: u64,
    compressed: u64,
    limit: u64,
}

fn u32_at(bytes: &[u8], at: u64) -> Result<u32> {
    let at = usize::try_from(at)?;
    let field = bytes.get(at..at + 4).ok_or("system page header past end")?;
    Ok(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
}

/// §4.3 system page: type, decompressed size, compressed size, then data at +0x14.
fn system_page(bytes: &[u8], at: u64, kind: &'static str, tag: u32) -> Result<Page> {
    if u32_at(bytes, at)? != tag {
        return Err(format!("{kind} page type is not 0x{tag:08x}").into());
    }
    Ok(Page {
        kind,
        section: String::new(),
        offset: at + 0x14,
        compressed: u64::from(u32_at(bytes, at + 8)?),
        limit: u64::from(u32_at(bytes, at + 4)?),
    })
}

fn pages(bytes: &[u8]) -> Result<Vec<Page>> {
    let header = R2004Header::parse(bytes)?;
    let mut out = vec![system_page(
        bytes,
        header.section_page_map_addr + 0x100,
        "page_map",
        0x4163_0e3b,
    )?];
    let map = section_map::parse_page_map(bytes, &header)?;
    let sections = map
        .iter()
        .find(|p| !p.is_gap && p.number == header.section_map_id as i32)
        .ok_or("section map page not in the page map")?;
    out.push(system_page(
        bytes,
        sections.file_offset,
        "section_map",
        0x4163_003b,
    )?);
    for description in section_map::parse_section_info(bytes, &header, &map)? {
        if description.compressed != 2 {
            continue;
        }
        for page_ref in &description.pages {
            let page = map
                .iter()
                .find(|p| !p.is_gap && p.number == page_ref.page_number as i32)
                .ok_or("data page not in the page map")?;
            let data = DataPageHeader::parse(bytes, page.file_offset)?;
            out.push(Page {
                kind: "data",
                section: description.name.clone(),
                offset: page.file_offset + 0x20,
                compressed: u64::from(data.data_size),
                limit: u64::from(description.max_decomp_page_size),
            });
        }
    }
    Ok(out)
}

fn describe(index: usize, page: &Page) -> String {
    format!(
        "\"page\":{index},\"kind\":\"{}\",\"section\":\"{}\",\"offset\":{},\"compressed\":{},\"limit\":{}",
        page.kind, page.section, page.offset, page.compressed, page.limit
    )
}

#[derive(Debug)]
struct TracedSource {
    file: FileSource<File>,
    trace: Option<File>,
    inject_pending: bool,
    pending: bool,
}

impl TracedSource {
    /// Read exactly `len` bytes at `offset`, recording each read.
    fn read(&mut self, offset: u64, output: &mut [u8]) -> Result<()> {
        let mut at = 0;
        while at < output.len() {
            let request = Request {
                offset: offset + at as u64,
                len: output.len() - at,
            };
            if self.inject_pending && !self.pending {
                self.pending = true;
                continue;
            }
            self.pending = false;
            match self.file.poll_read(request, &mut output[at..])? {
                SourcePoll::Ready(n) => {
                    if let Some(trace) = self.trace.as_mut() {
                        writeln!(trace, "{{\"offset\":{},\"bytes\":{n}}}", request.offset)?;
                    }
                    at += n;
                }
                SourcePoll::Pending => (),
                SourcePoll::Eof => return Err("source ended inside a page".into()),
            }
        }
        Ok(())
    }
}

/// Everything the caller holds that the decoder does not.
#[derive(Debug, Default)]
struct Caller {
    page: usize,
    read: u64,
    crc: u32,
    produced: u64,
    calls: u64,
    pending: Vec<u8>,
}

impl Caller {
    fn save(&self, path: &str) -> io::Result<()> {
        let pending: String = self.pending.iter().map(|b| format!("{b:02x}")).collect();
        fs::write(
            path,
            format!(
                "{} {} {} {} {} {}\n",
                self.page,
                self.read,
                self.crc,
                self.produced,
                self.calls,
                if pending.is_empty() { "-" } else { &pending }
            ),
        )
    }

    fn load(path: &str) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        let fields: Vec<&str> = text.split_whitespace().collect();
        if fields.len() != 6 || fields[5].len() > 512 {
            return Err("malformed caller state".into());
        }
        let pending = if fields[5] == "-" {
            Vec::new()
        } else {
            (0..fields[5].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(fields[5].get(i..i + 2).unwrap_or("x"), 16))
                .collect::<std::result::Result<_, _>>()?
        };
        Ok(Self {
            page: fields[0].parse()?,
            read: fields[1].parse()?,
            crc: fields[2].parse()?,
            produced: fields[3].parse()?,
            calls: fields[4].parse()?,
            pending,
        })
    }
}

struct Schedule {
    input: Vec<usize>,
    output: Vec<usize>,
    stop_after: Option<u64>,
    checkpoint_out: Option<String>,
    truncate: Option<u64>,
}

/// Decode one page through the streaming decoder. Returns `None` when it
/// stopped at a checkpoint.
fn decode(
    page: &Page,
    decoder: &mut Lz77Decoder,
    caller: &mut Caller,
    source: &mut TracedSource,
    schedule: &Schedule,
) -> Result<Option<u64>> {
    let available = schedule
        .truncate
        .unwrap_or(page.compressed)
        .min(page.compressed);
    let mut input = [0u8; 256];
    let mut output = vec![0u8; schedule.output.iter().copied().max().unwrap_or(1)];
    loop {
        if schedule.stop_after == Some(caller.calls) {
            let path = schedule
                .checkpoint_out
                .as_deref()
                .ok_or("missing checkpoint path")?;
            fs::write(path, decoder.checkpoint()?.as_bytes())?;
            caller.save(&format!("{path}.caller"))?;
            println!("{{\"checkpoint\":true,\"calls\":{}}}", caller.calls);
            return Ok(None);
        }
        if decoder.is_done() {
            return Ok(Some(decoder.consumed()));
        }
        let step = usize::try_from(caller.calls)?;
        let want_in = schedule.input[step % schedule.input.len()];
        let want_out = schedule.output[step % schedule.output.len()];
        caller.calls += 1;
        // Read new bytes only once the caller's unconsumed input is used up,
        // so no source byte is read twice.
        if caller.pending.is_empty() && !decoder.has_pending_output() {
            let n = (want_in as u64).min(available - caller.read) as usize;
            if n == 0 {
                decoder.end_of_input()?;
                return Ok(Some(decoder.consumed()));
            }
            source.read(page.offset + caller.read, &mut input[..n])?;
            caller.read += n as u64;
            caller.pending.extend_from_slice(&input[..n]);
        }
        let progress = decoder.decode(&caller.pending, &mut output[..want_out])?;
        caller.pending.drain(..progress.consumed);
        caller.crc = crc32(caller.crc, &output[..progress.produced]);
        caller.produced += progress.produced as u64;
    }
}

fn list(spec: &str, max: usize) -> Result<Vec<usize>> {
    if spec.len() > 1024 {
        return Err("schedule too long".into());
    }
    let values = spec
        .split(',')
        .map(str::parse)
        .collect::<std::result::Result<Vec<usize>, _>>()?;
    if values.is_empty() || values.iter().any(|&n| n == 0 || n > max) {
        return Err(format!("schedule values must be 1..={max}").into());
    }
    Ok(values)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: stream_lz77 file.dwg [options]")?;
    let mut schedule = Schedule {
        input: vec![256],
        output: vec![4096],
        stop_after: None,
        checkpoint_out: None,
        truncate: None,
    };
    let (mut only, mut resume, mut trace, mut limit) = (None, None, None, None);
    let (mut use_legacy, mut inject_pending) = (false, false);
    let (mut decode_checkpoint, mut direct_input) = (None, None);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("missing value for {arg}"));
        match arg.as_str() {
            "--legacy" => use_legacy = true,
            "--chunks" => schedule.input = list(&value()?, 256)?,
            "--out" => schedule.output = list(&value()?, 1 << 16)?,
            "--page" => only = Some(value()?.parse::<usize>()?),
            "--stop-after" => schedule.stop_after = Some(value()?.parse()?),
            "--checkpoint-out" => schedule.checkpoint_out = Some(value()?),
            "--resume" => resume = Some(value()?),
            "--trace" => trace = Some(value()?),
            "--truncate" => schedule.truncate = Some(value()?.parse()?),
            "--limit" => limit = Some(value()?.parse::<u64>()?),
            "--pending" => inject_pending = true,
            "--decode-checkpoint" => decode_checkpoint = Some(value()?),
            "--input" => direct_input = Some(value()?),
            number if !number.starts_with('-') => schedule.input = list(number, 256)?,
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    if schedule.stop_after.is_some() != schedule.checkpoint_out.is_some() {
        return Err("--stop-after and --checkpoint-out go together".into());
    }
    if let Some(state) = decode_checkpoint {
        let hex = direct_input.ok_or("--decode-checkpoint needs --input")?;
        if hex.len() > 512 || hex.len() % 2 != 0 {
            return Err("--input is at most 256 bytes of hex".into());
        }
        let input = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<std::result::Result<Vec<u8>, _>>()?;
        let mut decoder = Lz77Decoder::restore(&fs::read(state)?)?;
        let mut output = [0u8; 4096];
        let progress = decoder.decode(&input, &mut output)?;
        println!(
            "{{\"consumed\":{},\"produced\":{},\"total_consumed\":{},\"total_produced\":{}}}",
            progress.consumed,
            progress.produced,
            decoder.consumed(),
            decoder.produced()
        );
        return Ok(());
    }
    let bytes = fs::read(&path)?;
    let mut all = pages(&bytes)?;
    if let Some(limit) = limit {
        for page in &mut all {
            page.limit = limit;
        }
    }
    let mut source = TracedSource {
        file: FileSource::new(File::open(&path)?),
        trace: trace.map(File::create).transpose()?,
        inject_pending,
        pending: false,
    };

    if let Some(state) = resume {
        let mut caller = Caller::load(&format!("{state}.caller"))?;
        let page = all.get(caller.page).ok_or("checkpoint page out of range")?;
        let mut decoder = Lz77Decoder::restore(&fs::read(&state)?)?;
        if decoder.limit() != page.limit
            || decoder.produced() != caller.produced
            || caller.read > page.compressed
            || decoder.consumed().checked_add(caller.pending.len() as u64) != Some(caller.read)
        {
            return Err("checkpoint does not match the caller state".into());
        }
        if let Some(consumed) = decode(page, &mut decoder, &mut caller, &mut source, &schedule)? {
            println!(
                "{{{},\"consumed\":{consumed},\"produced\":{},\"crc32\":{},\"calls\":{}}}",
                describe(caller.page, page),
                caller.produced,
                caller.crc,
                caller.calls
            );
        }
        return Ok(());
    }

    let mut total = 0u64;
    for (index, page) in all.iter().enumerate() {
        if only.is_some_and(|n| n != index) {
            continue;
        }
        let start = usize::try_from(page.offset)?;
        let end = start + usize::try_from(page.compressed)?;
        if use_legacy {
            let data = bytes.get(start..end).ok_or("page past end of file")?;
            let out = dwg::lz77::decompress(data, None)?;
            total += out.len() as u64;
            println!(
                "{{{},\"produced\":{},\"crc32\":{}}}",
                describe(index, page),
                out.len(),
                crc32(0, &out)
            );
            continue;
        }
        let mut decoder = Lz77Decoder::new(page.limit);
        let mut caller = Caller {
            page: index,
            ..Caller::default()
        };
        let Some(consumed) = decode(page, &mut decoder, &mut caller, &mut source, &schedule)?
        else {
            return Ok(());
        };
        total += caller.produced;
        let calls = if only.is_some() {
            format!(",\"calls\":{}", caller.calls)
        } else {
            String::new()
        };
        println!(
            "{{{},\"consumed\":{consumed},\"produced\":{},\"crc32\":{}{calls}}}",
            describe(index, page),
            caller.produced,
            caller.crc
        );
    }
    if only.is_none() {
        println!("{{\"pages\":{},\"produced\":{total}}}", all.len());
    }
    Ok(())
}
