//! Times the R2007 walk against the legacy reader on one drawing, in memory:
//! the whole-file streaming walk (`R2007Walker` over a `SliceSource`, 256-byte
//! scratch, every page verified and every section byte handed out, holes
//! counted as the zeros they stand for) and
//! `r2007::Container::parse` plus `read_section` for every section. Prints one
//! JSON line with the best of `rounds` runs of each (default 5) and the
//! throughput in MB/s (10^6 bytes) of the file and of the section bytes
//! produced. A measurement, not a gate: run it with `--release` on a quiet
//! machine. `stream_r2007_timing file.dwg [rounds]`.

use dwg::byte_source::{R2007Poll, SliceSource, poll_r2007_walk};
use dwg::r2007::Container;
use dwg::streaming::{CHUNK_BYTES, R2007Step, R2007Walker, SourceIdentity};
use std::time::Instant;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn streaming(bytes: &[u8]) -> Result<u64> {
    let mut walker = R2007Walker::new(SourceIdentity {
        digest: [0; 32],
        length: bytes.len() as u64,
    });
    let mut source = SliceSource::new(bytes);
    let mut scratch = [0; CHUNK_BYTES];
    let mut produced = 0u64;
    loop {
        match poll_r2007_walk(&mut walker, &mut source, &mut scratch)? {
            R2007Poll::Complete => return Ok(produced),
            R2007Poll::Pending => unreachable!("a slice never pends"),
            R2007Poll::Fed { step, .. } | R2007Poll::Pumped(step) => match step {
                R2007Step::Data { len, .. } => produced += walker.data().len().min(len) as u64,
                R2007Step::Hole { len, .. } => produced += len,
                _ => (),
            },
        }
    }
}

fn legacy(bytes: &[u8]) -> Result<u64> {
    let c = Container::parse(bytes)?;
    let mut produced = 0u64;
    for s in &c.section_map.sections {
        produced += c.read_section(bytes, &s.name)?.len() as u64;
    }
    Ok(produced)
}

fn best<T>(rounds: u32, mut f: impl FnMut() -> Result<T>) -> Result<(f64, T)> {
    let mut fastest = f64::MAX;
    let mut last = None;
    for _ in 0..rounds {
        let start = Instant::now();
        let value = f()?;
        fastest = fastest.min(start.elapsed().as_secs_f64());
        last = Some(value);
    }
    Ok((fastest, last.ok_or("no rounds")?))
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: stream_r2007_timing file.dwg [rounds]")?;
    let rounds: u32 = args.next().map(|v| v.parse()).transpose()?.unwrap_or(5);
    let bytes = std::fs::read(&path)?;
    let (stream_s, stream_bytes) = best(rounds, || streaming(&bytes))?;
    let (legacy_s, legacy_bytes) = best(rounds, || legacy(&bytes))?;
    if stream_bytes != legacy_bytes {
        return Err(
            format!("the walks produced {stream_bytes} and {legacy_bytes} section bytes").into(),
        );
    }
    let mb = |n: u64, s: f64| n as f64 / 1e6 / s;
    println!(
        "{{\"file_bytes\":{},\"section_bytes\":{stream_bytes},\"rounds\":{rounds},\"streaming_ms\":{:.3},\"legacy_ms\":{:.3},\"streaming_file_mb_s\":{:.1},\"legacy_file_mb_s\":{:.1},\"streaming_section_mb_s\":{:.1},\"legacy_section_mb_s\":{:.1}}}",
        bytes.len(),
        stream_s * 1e3,
        legacy_s * 1e3,
        mb(bytes.len() as u64, stream_s),
        mb(bytes.len() as u64, legacy_s),
        mb(stream_bytes, stream_s),
        mb(legacy_bytes, legacy_s),
    );
    Ok(())
}
