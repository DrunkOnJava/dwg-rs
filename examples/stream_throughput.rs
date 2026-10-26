//! Times the streaming walks against the legacy reader on in-memory copies of
//! the same drawings, so disk speed is not measured. For each file it times
//! `--runs` (default 5, after one warm-up) and reports the best and the median;
//! MB/s is from the best, which a loaded machine disturbs least:
//!
//! - the streaming walk to its end: `FlatWalker` for R13 to R2000 (header,
//!   header variables, classes, object map, every record verified), and
//!   `R2004ObjectWalker` for the R2004 family (header, page map, section map,
//!   every data page checksummed and decoded, every object record framed and
//!   CRC-checked), in reads of 256 bytes from a slice;
//! - the legacy reader's `DwgFile::from_bytes` followed by
//!   `all_objects` (every record's bytes copied out, as the streaming walk hands
//!   them out; `all_objects_lossy` also resolves every handle reference and is
//!   a different job), and, for the R2004 family, followed by a read of every
//!   section (the work the streaming walk does on the way, minus the framing and
//!   the checksums).
//!
//! It prints one JSON line per file with the times and MB/s of the file's bytes,
//! and fails if the streaming walk and the legacy reader disagree on the number
//! of records. Build with `--release`; the numbers are the machine's and the
//! day's. `stream_throughput file.dwg... [--runs N]`.

use dwg::byte_source::{ObjectPoll, SliceSource, WalkPoll, poll_r2004_objects, poll_walk};
use dwg::reader::DwgFile;
use dwg::streaming::{
    CHUNK_BYTES, FlatWalker, ObjectStep, R2004ObjectWalker, SourceIdentity, Step,
};
use std::fs;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Timing {
    best: Duration,
    median: Duration,
}

fn summarize(mut times: Vec<Duration>) -> Timing {
    times.sort();
    Timing {
        best: times[0],
        median: times[times.len() / 2],
    }
}

fn stream_flat(bytes: &[u8]) -> Result<u64> {
    let identity = SourceIdentity {
        digest: [0; 32],
        length: bytes.len() as u64,
    };
    let mut walker = FlatWalker::new(identity);
    let mut source = SliceSource::new(bytes);
    let mut scratch = [0; CHUNK_BYTES];
    let mut records = 0;
    loop {
        match poll_walk(&mut walker, &mut source, &mut scratch)? {
            WalkPoll::Complete => return Ok(records),
            WalkPoll::Fed {
                step: Step::Object(_),
                ..
            } => records += 1,
            _ => (),
        }
    }
}

fn stream_family(bytes: &[u8]) -> Result<u64> {
    let identity = SourceIdentity {
        digest: [0; 32],
        length: bytes.len() as u64,
    };
    let mut walker = R2004ObjectWalker::new(identity);
    let mut source = SliceSource::new(bytes);
    let mut scratch = [0; CHUNK_BYTES];
    let mut records = 0;
    loop {
        match poll_r2004_objects(&mut walker, &mut source, &mut scratch)? {
            ObjectPoll::Complete => return Ok(records),
            ObjectPoll::Fed {
                step: ObjectStep::Object(_),
                ..
            }
            | ObjectPoll::Pumped(ObjectStep::Object(_)) => records += 1,
            _ => (),
        }
    }
}

fn legacy_objects(bytes: &[u8]) -> Result<u64> {
    let file = DwgFile::from_bytes(bytes.to_vec())?;
    let objects = file.all_objects().ok_or("no object stream")??;
    Ok(objects.len() as u64)
}

fn legacy_sections(bytes: &[u8]) -> Result<u64> {
    let file = DwgFile::from_bytes(bytes.to_vec())?;
    let mut total = 0;
    for section in file.sections() {
        if let Some(data) = file.read_section(&section.name) {
            total += data?.len() as u64;
        }
    }
    Ok(total)
}

fn time<T>(runs: usize, mut work: impl FnMut() -> Result<T>) -> Result<(Timing, T)> {
    let mut last = work()?;
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        last = work()?;
        times.push(start.elapsed());
    }
    Ok((summarize(times), last))
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut files, mut runs) = (Vec::new(), 5usize);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--runs" => runs = args.next().ok_or("missing run count")?.parse()?,
            _ => files.push(arg),
        }
    }
    if files.is_empty() || runs == 0 {
        return Err("usage: stream_throughput file.dwg... [--runs N]".into());
    }
    for path in files {
        let bytes = fs::read(&path)?;
        let release = String::from_utf8_lossy(&bytes[..6.min(bytes.len())]).into_owned();
        let family = matches!(release.as_str(), "AC1018" | "AC1024" | "AC1027" | "AC1032");
        let (streaming, streamed) = if family {
            time(runs, || stream_family(&bytes))?
        } else {
            time(runs, || stream_flat(&bytes))?
        };
        let (objects, listed) = time(runs, || legacy_objects(&bytes))?;
        if listed != streamed {
            return Err(
                format!("{path}: streaming framed {streamed} records, legacy {listed}").into(),
            );
        }
        let mb = bytes.len() as f64 / 1e6;
        let rate = |t: &Timing| mb / t.best.as_secs_f64();
        let ms = |t: &Timing| {
            format!(
                "{:.2},\"median_ms\":{:.2}",
                t.best.as_secs_f64() * 1e3,
                t.median.as_secs_f64() * 1e3
            )
        };
        let sections = if family {
            let (t, read) = time(runs, || legacy_sections(&bytes))?;
            format!(
                ",\"legacy_sections\":{{\"best_ms\":{},\"mb_s\":{:.1},\"bytes\":{read}}}",
                ms(&t),
                rate(&t)
            )
        } else {
            String::new()
        };
        println!(
            "{{\"file\":\"{}\",\"release\":\"{release}\",\"bytes\":{},\"records\":{streamed},\"runs\":{runs},\"streaming\":{{\"best_ms\":{},\"mb_s\":{:.1}}},\"legacy_objects\":{{\"best_ms\":{},\"mb_s\":{:.1}}}{sections}}}",
            path.rsplit('/').next().unwrap_or(&path),
            bytes.len(),
            ms(&streaming),
            rate(&streaming),
            ms(&objects),
            rate(&objects),
        );
    }
    Ok(())
}
