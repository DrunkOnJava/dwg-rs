//! What fills the bytes of an R13 or R14 object stream that no handle-map
//! entry claims? (#119)
//!
//! # What this proves
//!
//! For each drawing given, this walks the object map, takes the runs of the
//! stream between the records it names (`probe_reference_closure` reports
//! their size as "between records"), and reads each run as a sequence of
//! object records: the `MS` size, that many bytes, a 2-byte CRC (§2.14, seed
//! 0xC0C1 over the `MS` and the data), the `BS` object type and the `H`
//! handle that open the data. It prints, per drawing and in total, how many
//! runs tile exactly into records, how many records have a valid CRC, which
//! of their handles the object map also names, and which object types they
//! are. A run that tiles into CRC-valid records whose handles the map does
//! not name holds records the map no longer lists; one that does not tile is
//! reported as it is, not explained.
//!
//! ```sh
//! cargo run --release --example probe_unclaimed_runs -- a.dwg b.dwg
//! ```
//!
//! Exit status is non-zero when a drawing does not open or has no walk.

use dwg::DwgFile;
use dwg::bitcursor::BitCursor;
use dwg::crc::crc8;
use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

const SEED: u16 = 0xC0C1;

fn ms_at(stream: &[u8], at: usize) -> Option<(usize, usize)> {
    let lo = u16::from_le_bytes([*stream.get(at)?, *stream.get(at + 1)?]);
    if lo & 0x8000 == 0 {
        return Some((usize::from(lo), 2));
    }
    let hi = u16::from_le_bytes([*stream.get(at + 2)?, *stream.get(at + 3)?]);
    Some((usize::from(lo & 0x7FFF) | usize::from(hi & 0x7FFF) << 15, 4))
}

#[derive(Default)]
struct Tally {
    drawings: usize,
    drawings_with_runs: usize,
    runs: usize,
    runs_tiling: usize,
    run_bytes: usize,
    records: usize,
    crc_valid: usize,
    handle_listed: usize,
    types: BTreeMap<u16, usize>,
}

fn main() -> ExitCode {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: probe_unclaimed_runs <file.dwg>...");
        return ExitCode::FAILURE;
    }
    let mut total = Tally::default();
    let mut failed = false;
    for path in &paths {
        let Ok(file) = DwgFile::open(path) else {
            eprintln!("{path}: does not open");
            failed = true;
            continue;
        };
        let (Some(Ok((objects, _))), Some(Ok(stream))) = (
            file.all_objects_lossy().map(|r| r.map_err(|_| ())),
            file.read_section("AcDb:AcDbObjects"),
        ) else {
            eprintln!("{path}: no object walk");
            failed = true;
            continue;
        };
        let listed: BTreeSet<u64> = objects.iter().map(|o| o.handle.value).collect();
        let mut spans: Vec<(usize, usize)> = objects
            .iter()
            .map(|o| {
                let ms_len = if o.size_bytes < 0x8000 { 2 } else { 4 };
                (o.stream_offset, o.stream_offset + ms_len + o.raw.len() + 2)
            })
            .collect();
        spans.sort_unstable();
        let mut runs = Vec::new();
        let mut end = spans.first().map_or(0, |s| s.0);
        for (s, e) in &spans {
            if *s > end {
                runs.push((end, *s));
            }
            end = end.max(*e);
        }
        let mut here = Tally::default();
        for (a, b) in &runs {
            here.runs += 1;
            here.run_bytes += b - a;
            let mut at = *a;
            let mut tiles = true;
            while at < *b {
                let Some((size, ms_len)) = ms_at(&stream, at) else {
                    tiles = false;
                    break;
                };
                let stop = at + ms_len + size + 2;
                if size == 0 || stop > *b {
                    tiles = false;
                    break;
                }
                here.records += 1;
                let crc = u16::from_le_bytes([stream[stop - 2], stream[stop - 1]]);
                if crc8(SEED, &stream[at..stop - 2]) == crc {
                    here.crc_valid += 1;
                }
                let mut c = BitCursor::new(&stream[at + ms_len..stop - 2]);
                if let (Ok(kind), Ok(handle)) = (c.read_bs_u(), c.read_handle()) {
                    *here.types.entry(kind).or_default() += 1;
                    if listed.contains(&handle.value) {
                        here.handle_listed += 1;
                    }
                }
                at = stop;
            }
            here.runs_tiling += usize::from(tiles);
        }
        println!(
            "{path}: {} run(s), {} bytes, {} tile exactly; {} record(s), {} with a valid CRC, {} whose handle the map names",
            here.runs,
            here.run_bytes,
            here.runs_tiling,
            here.records,
            here.crc_valid,
            here.handle_listed
        );
        total.drawings += 1;
        total.drawings_with_runs += usize::from(here.runs > 0);
        total.runs += here.runs;
        total.runs_tiling += here.runs_tiling;
        total.run_bytes += here.run_bytes;
        total.records += here.records;
        total.crc_valid += here.crc_valid;
        total.handle_listed += here.handle_listed;
        for (k, v) in here.types {
            *total.types.entry(k).or_default() += v;
        }
    }
    println!(
        "{} drawing(s), {} with unclaimed runs: {} run(s), {} bytes, {} tile exactly; {} record(s), {} with a valid CRC, {} whose handle the map names",
        total.drawings,
        total.drawings_with_runs,
        total.runs,
        total.run_bytes,
        total.runs_tiling,
        total.records,
        total.crc_valid,
        total.handle_listed
    );
    for (kind, n) in &total.types {
        println!("  type 0x{kind:02X}: {n}");
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
