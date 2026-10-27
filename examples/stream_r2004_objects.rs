//! Streams the object records of an R2004-family file: the whole-file walk
//! with `AcDb:AcDbObjects` framed into records in section order. Prints one
//! line per committed record (its handle, section offset, `MS` size, type,
//! payload length and the CRC-32 of the payload, the bytes the legacy
//! `RawObject::raw` holds), then the end of the object stream and of the
//! walk, none of which depends on how the source was chunked.
//! `stream_r2004_objects file.dwg [chunk-bytes] [--chunks 1,7,3] [--pending]
//! [--trace <path>] [--report-steps]`. `--legacy` prints the same record
//! lines from the legacy reader's handle-map walk, in map order. `--identity
//! <sha256> --checkpoint-out <path> --stop-after <steps>` saves the walk
//! after that many steps; a fresh invocation with `--resume <path>` continues
//! from the checkpoint alone, with the cursors over the payload in
//! `<checkpoint>.caller`. `--report-steps` prints each record, the end of the
//! stream and the walk's first and last step numbers to stderr.

use dwg::byte_source::{ByteSource, FileSource, ObjectPoll, SourcePoll, poll_r2004_objects};
use dwg::crc::crc32;
use dwg::reader::DwgFile;
use dwg::streaming::{
    CHUNK_BYTES, ObjectStep, R2004ObjectWalker, Request, SourceIdentity, WalkStep,
};
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

fn legacy(path: &str) -> Result<()> {
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
            "{{\"object\":{},\"offset\":{},\"size\":{},\"type\":{},\"len\":{},\"crc32\":{}}}",
            object.handle.value,
            object.stream_offset,
            object.size_bytes,
            object.type_code,
            object.raw.len(),
            crc32(0, &object.raw)
        );
    }
    println!("{{\"objects\":{{\"records\":{}}}}}", objects.len());
    Ok(())
}

/// What the caller keeps beside the walker's checkpoint: the steps taken and
/// the digest and length of the open record's payload so far.
struct Cursor {
    steps: u64,
    digest: u32,
    len: u64,
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: stream_r2004_objects file.dwg [chunk-bytes] [options]")?;
    let mut chunks = vec![CHUNK_BYTES];
    let (mut trace, mut inject_pending, mut as_legacy, mut report_steps) =
        (None, false, false, false);
    let (mut digest, mut checkpoint_out, mut stop_after, mut resume) = (None, None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--legacy" => as_legacy = true,
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
        return legacy(&path);
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
    let (mut walker, mut cursor) = match &resume {
        Some(path) => {
            let saved = fs::read_to_string(format!("{path}.caller"))?;
            let n: Vec<&str> = saved.split_whitespace().collect();
            (
                R2004ObjectWalker::restore(&fs::read(path)?, identity)?,
                Cursor {
                    steps: n[0].parse()?,
                    digest: n[1].parse()?,
                    len: n[2].parse()?,
                },
            )
        }
        None => (
            R2004ObjectWalker::new(identity),
            Cursor {
                steps: 0,
                digest: 0,
                len: 0,
            },
        ),
    };
    let mut scratch = [0; CHUNK_BYTES];
    let mut poll = 0;
    loop {
        if stop_after == Some(cursor.steps) {
            let to = checkpoint_out.as_deref().ok_or("missing checkpoint path")?;
            fs::write(to, walker.checkpoint()?.as_bytes())?;
            fs::write(
                format!("{to}.caller"),
                format!("{} {} {}\n", cursor.steps, cursor.digest, cursor.len),
            )?;
            println!("{{\"checkpoint\":true,\"steps\":{}}}", cursor.steps);
            return Ok(());
        }
        let n = chunks[poll % chunks.len()];
        poll += 1;
        let before = walker.demand();
        let step = match poll_r2004_objects(&mut walker, &mut source, &mut scratch[..n])? {
            ObjectPoll::Complete => break,
            ObjectPoll::Pending => {
                if walker.demand() != before {
                    return Err("pending source mutated grammar".into());
                }
                continue;
            }
            ObjectPoll::Fed { step, .. } | ObjectPoll::Pumped(step) => step,
        };
        cursor.steps += 1;
        match step {
            ObjectStep::Walk(WalkStep::Finished { sections, pages }) => {
                println!("{{\"finished\":{{\"sections\":{sections},\"pages\":{pages}}}}}");
            }
            ObjectStep::Walk(_) => (),
            ObjectStep::Payload { len } => {
                let bytes = walker.data();
                if bytes.len() != len {
                    return Err("payload step and data() disagree".into());
                }
                cursor.digest = crc32(cursor.digest, bytes);
                cursor.len += len as u64;
            }
            ObjectStep::Object(record) => {
                println!(
                    "{{\"object\":{},\"offset\":{},\"size\":{},\"type\":{},\"len\":{},\"crc32\":{}}}",
                    record.handle,
                    record.offset,
                    record.size,
                    record.type_code,
                    cursor.len,
                    cursor.digest
                );
                if report_steps {
                    eprintln!("{{\"step\":{},\"event\":\"object\"}}", cursor.steps);
                }
                cursor.digest = 0;
                cursor.len = 0;
            }
            ObjectStep::Objects { records, bytes } => {
                println!("{{\"objects\":{{\"records\":{records},\"bytes\":{bytes}}}}}");
                if report_steps {
                    eprintln!("{{\"step\":{},\"event\":\"objects\"}}", cursor.steps);
                }
            }
        }
    }
    if report_steps {
        eprintln!("{{\"steps\":{}}}", cursor.steps);
    }
    Ok(())
}
