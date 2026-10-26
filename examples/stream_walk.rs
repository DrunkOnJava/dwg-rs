//! Runs the bounded R13–R2000 container walk and prints one JSON line per
//! committed header, section and object record.
//! `stream_walk file.dwg [chunk-bytes] [--chunks 1,7,3] [--pending]`
//! `--identity <sha256> --checkpoint-out <path> --stop-after <feeds>`
//! saves the walk after an exact number of feeds, plus the caller's own
//! provisional payload digest in `<path>.caller`; a fresh invocation with
//! `--resume <path>` continues. `--trace <path>` saves actual source reads.
//! `--length <bytes>` takes the source length from a trusted manifest
//! instead of the file, so a short transport ends in a typed EOF.
//! `--legacy` prints the legacy reader's handle-map walk in the same shape.
//! `--url http://host:port/path` reads through `byte_source::AsyncSource`
//! and an HTTP range worker instead of the file (the file argument is then
//! only a label, and `--length` is required); on completion it prints the
//! Pending polls and fetches to stderr. With `--url`, a checkpoint also
//! saves the adapter's fetched but undelivered bytes in `<path>.source`,
//! which `--resume` restores, so nothing is fetched twice.

#[path = "support/http_range.rs"]
mod http_range;

use dwg::byte_source::{AsyncSource, ByteSource, FileSource, SourcePoll, WalkPoll, poll_walk};
use dwg::crc::crc32;
use dwg::reader::DwgFile;
use dwg::streaming::{CHUNK_BYTES, FlatWalker, Part, Request, SourceIdentity, Step};
use http_range::{Answer, HttpWorker};
use std::fs::{self, File};
use std::io::{self, Write};

#[derive(Debug)]
enum Inner {
    File(FileSource<File>),
    Remote(Box<AsyncSource>),
}

#[derive(Debug)]
struct TracedSource {
    inner: Inner,
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
        let result = match &mut self.inner {
            Inner::File(file) => file.poll_read(request, output)?,
            Inner::Remote(remote) => remote.poll_read(request, output)?,
        };
        if let (SourcePoll::Ready(n), Some(trace)) = (result, self.trace.as_mut()) {
            writeln!(trace, "{{\"offset\":{},\"bytes\":{n}}}", request.offset)?;
        }
        Ok(result)
    }
}

/// Everything the caller holds that the walker does not: the running
/// digest of uncommitted payload and its own counters.
#[derive(Debug, Default)]
struct Caller {
    digest: u32,
    objects: u64,
    feeds: u64,
}

impl Caller {
    fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let text = fs::read_to_string(path)?;
        let mut fields = text.split_whitespace().map(str::parse::<u64>);
        let mut next = || fields.next().ok_or("short caller state");
        Ok(Self {
            digest: u32::try_from(next()??)?,
            objects: next()??,
            feeds: next()??,
        })
    }

    fn save(&self, path: &str) -> io::Result<()> {
        fs::write(
            path,
            format!("{} {} {}\n", self.digest, self.objects, self.feeds),
        )
    }
}

fn legacy(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file = DwgFile::open(path)?;
    let (objects, summary) = file.all_objects_lossy().ok_or("no object stream")??;
    for skipped in &summary.skipped {
        println!(
            "{{\"skipped\":{},\"offset\":{}}}",
            skipped.handle, skipped.offset
        );
    }
    for object in &objects {
        println!(
            "{{\"object\":{},\"offset\":{},\"size\":{},\"type\":{},\"crc32\":{}}}",
            object.handle.value,
            object.stream_offset,
            object.size_bytes,
            object.type_code,
            crc32(0, &object.raw)
        );
    }
    println!("{{\"finished\":true,\"objects\":{}}}", objects.len());
    Ok(())
}

fn print_step(step: Step, caller: &mut Caller) {
    match step {
        Step::Header(header) => {
            print!(
                "{{\"header\":\"{}\",\"locators\":[",
                String::from_utf8_lossy(&header.magic)
            );
            for (index, l) in header.locators[..usize::from(header.locator_count)]
                .iter()
                .enumerate()
            {
                print!(
                    "{}[{},{},{}]",
                    if index == 0 { "" } else { "," },
                    l.number,
                    l.offset,
                    l.size
                );
            }
            println!("]}}");
        }
        Step::Section(section) => {
            let name = if section.part == Part::HeaderVars {
                "header_vars"
            } else {
                "classes"
            };
            println!(
                "{{\"section\":\"{name}\",\"offset\":{},\"size\":{},\"crc32\":{}}}",
                section.data_offset, section.size, caller.digest
            );
            caller.digest = 0;
        }
        Step::Object(object) => {
            println!(
                "{{\"object\":{},\"offset\":{},\"size\":{},\"type\":{},\"crc32\":{}}}",
                object.handle, object.offset, object.size, object.type_code, caller.digest
            );
            caller.digest = 0;
            caller.objects += 1;
        }
        Step::Finished => println!("{{\"finished\":true,\"objects\":{}}}", caller.objects),
        Step::Framing | Step::Payload(_) => (),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: stream_walk file.dwg [chunk-bytes] [options]")?;
    let mut chunks = vec![CHUNK_BYTES];
    let mut stop_after = None;
    let mut checkpoint_out = None;
    let mut resume = None;
    let mut trace = None;
    let mut digest = None;
    let mut inject_pending = false;
    let mut length = None;
    let mut url = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--legacy" => return legacy(&path),
            "--stop-after" => {
                stop_after = Some(args.next().ok_or("missing feed count")?.parse::<u64>()?)
            }
            "--checkpoint-out" => {
                checkpoint_out = Some(args.next().ok_or("missing checkpoint path")?)
            }
            "--resume" => resume = Some(args.next().ok_or("missing resume path")?),
            "--trace" => trace = Some(args.next().ok_or("missing trace path")?),
            "--pending" => inject_pending = true,
            "--length" => length = Some(args.next().ok_or("missing length")?.parse::<u64>()?),
            "--url" => url = Some(args.next().ok_or("missing URL")?),
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
    let worker = url.as_deref().map(HttpWorker::spawn).transpose()?;
    let held = match (&worker, &resume) {
        (Some(_), Some(path)) => {
            let text = fs::read_to_string(format!("{path}.source"))?;
            let (offset, hex) = text
                .trim()
                .split_once(' ')
                .ok_or("malformed source state")?;
            if hex.len() > 2 * CHUNK_BYTES || hex.len() % 2 != 0 && hex != "-" {
                return Err("malformed source state".into());
            }
            let bytes = if hex == "-" {
                Vec::new()
            } else {
                (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
                    .collect::<Result<Vec<u8>, _>>()?
            };
            AsyncSource::with_held(offset.parse()?, &bytes)?
        }
        _ => AsyncSource::new(),
    };
    let (inner, file_length) = match &worker {
        Some(_) => (Inner::Remote(Box::new(held)), None),
        None => {
            let file = File::open(&path)?;
            let len = file.metadata()?.len();
            (Inner::File(FileSource::new(file)), Some(len))
        }
    };
    let identity = SourceIdentity {
        digest: digest.unwrap_or([0; 32]),
        length: length
            .or(file_length)
            .ok_or("--url needs --length from a trusted manifest")?,
    };
    let (mut walker, mut caller) = match resume {
        Some(path) => {
            let bytes = fs::read(&path)?;
            if bytes.len() > CHUNK_BYTES {
                return Err("checkpoint exceeds one chunk".into());
            }
            (
                FlatWalker::restore(&bytes, identity)?,
                Caller::load(&format!("{path}.caller"))?,
            )
        }
        None => (FlatWalker::new(identity), Caller::default()),
    };
    let mut source = TracedSource {
        inner,
        trace: trace.map(File::create).transpose()?,
        inject_pending,
        pending: false,
    };
    let mut scratch = [0; CHUNK_BYTES];
    let mut poll = 0;
    let (mut pending_polls, mut fetches) = (0u64, 0u64);
    loop {
        if stop_after == Some(caller.feeds) {
            let out = checkpoint_out.as_deref().ok_or("missing checkpoint path")?;
            fs::write(out, walker.checkpoint()?.as_bytes())?;
            caller.save(&format!("{out}.caller"))?;
            if let Inner::Remote(remote) = &source.inner {
                if remote.in_flight() {
                    return Err("a fetch is in flight; no checkpoint".into());
                }
                let (offset, bytes) = remote.held();
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                fs::write(
                    format!("{out}.source"),
                    format!("{offset} {}\n", if hex.is_empty() { "-" } else { &hex }),
                )?;
            }
            println!("{{\"checkpoint\":true,\"feeds\":{}}}", caller.feeds);
            return Ok(());
        }
        let n = chunks[poll % chunks.len()];
        poll += 1;
        let request_before = walker.request();
        match poll_walk(&mut walker, &mut source, &mut scratch[..n])? {
            WalkPoll::Complete => {
                if worker.is_some() {
                    eprintln!("{{\"pending_polls\":{pending_polls},\"fetches\":{fetches}}}");
                }
                return Ok(());
            }
            WalkPoll::Pending => {
                if walker.request() != request_before {
                    return Err("pending source mutated grammar".into());
                }
                pending_polls += 1;
                if let (Some(worker), Inner::Remote(remote)) = (&worker, &mut source.inner) {
                    if let Some(request) = remote.take_request() {
                        worker.fetch(request)?;
                        fetches += 1;
                    }
                    if remote.in_flight() {
                        // The host's wake-up: block until the worker answers.
                        let delivered = match worker.wait() {
                            Answer::Bytes(offset, bytes) => remote.complete(offset, &bytes),
                            Answer::Eof(offset) => remote.complete_eof(offset),
                            Answer::Failed(error) => Err(error),
                        };
                        if let Err(error) = delivered {
                            remote.fail(error);
                        }
                    }
                }
            }
            WalkPoll::Fed { step, len, .. } => {
                caller.feeds += 1;
                if let Step::Payload(_) = step {
                    caller.digest = crc32(caller.digest, &scratch[..len]);
                }
                print_step(step, &mut caller);
            }
        }
    }
}
