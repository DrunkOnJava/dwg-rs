//! Census, joint-boundary check and uniqueness search for VISUALSTYLE.
//!
//! # What this measures
//!
//! [`probe_field_list`](../probe_field_list/index.html) measures one
//! candidate field list against one record. A field list is only
//! *derived*, though, when a single token sequence lands **every** record
//! of every file of a release band exactly on its data-stream boundary —
//! 72 records of 72, not one. This probe is that joint check.
//!
//! Three modes:
//!
//! * with no `--spec` it is a **census** — how many VISUALSTYLE records
//!   each file holds and what bit budget each gives a field list to fill.
//!   That is where dwg-rs#73's record distribution comes from;
//! * with `--spec` it is the **joint-boundary check** — every record's
//!   delta from its own boundary, and for a short list the bits left
//!   over, so a near miss shows what it is missing;
//! * with `--spec` and `--search` it is a **uniqueness search** — every
//!   one-token mutation of the spec (insert, delete or substitute one of
//!   `B` / `BS` / `BL` / `BD` / `RC` / `CMC`) is measured against the same
//!   joint boundary, and the ones that also close are printed. A base
//!   spec whose only closing neighbour is itself is a derivation; one
//!   with several is a guess with company.
//!
//! ```sh
//! cargo run --release --example probe_visualstyle_layout -- samples/*.dwg
//! cargo run --release --example probe_visualstyle_layout -- \
//!     --spec TV,BS,BS,BD,CMC --verbose samples/arc_2000.dwg
//! cargo run --release --example probe_visualstyle_layout -- \
//!     --spec TV,BS,BS,BD,CMC --search samples/arc_2000.dwg
//! ```
//!
//! Field-spec tokens are the ODA spec's type codes, the same set
//! `probe_field_list` accepts: `B`, `BB`, `BS`, `BSU`, `BL`, `BLU`, `BD`,
//! `RC`, `RS`, `RL`, `RD`, `TV` and `CMC`, with an optional `n*` repeat
//! prefix. A `TV` consumes no data-stream bits on R2007+, where the
//! characters live in the string stream, and is read inline before it.
//!
//! The exit code is non-zero when a `--spec` run leaves any record off
//! its boundary, so the probe doubles as a regression gate.

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::string_stream::{self, StringReader};
use dwg::{DwgFile, ObjectType, Version};
use std::collections::BTreeMap;
use std::process::ExitCode;

/// Token alphabet the uniqueness search mutates over.
const SEARCH_ALPHABET: [&str; 6] = ["B", "BS", "BL", "BD", "RC", "CMC"];

/// One VISUALSTYLE record, lifted out of its file so candidate field
/// lists can be replayed against it without re-walking the drawing.
struct Record {
    handle: u64,
    version: Version,
    raw: Vec<u8>,
    /// Bit just past the common object data.
    body: usize,
    /// Bit at which the data fields must end.
    data_end: Option<usize>,
}

/// Advance `c` past the EED chain of the common object data.
fn skip_eed(c: &mut BitCursor<'_>) -> Result<()> {
    for _ in 0..256 {
        let size = c.read_bs_u()? as usize;
        if size == 0 {
            return Ok(());
        }
        let _appid = c.read_handle()?;
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
    }
    Ok(())
}

/// Read a `TV` from the string stream when the file has one, inline otherwise.
fn read_tv(c: &mut BitCursor<'_>, strings: &mut Option<StringReader<'_>>) -> Result<String> {
    if let Some(reader) = strings.as_mut() {
        return reader.read_tv();
    }
    let len = c.read_bs_u()? as usize;
    let mut bytes = Vec::with_capacity(len);
    for _ in 0..len {
        bytes.push(c.read_rc()?);
    }
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn expand(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in spec.split(',').filter(|t| !t.trim().is_empty()) {
        let token = token.trim();
        match token.split_once('*') {
            Some((n, t)) => {
                let n: usize = n.parse().expect("repeat count must be a number");
                for _ in 0..n {
                    out.push(t.to_ascii_uppercase());
                }
            }
            None => out.push(token.to_ascii_uppercase()),
        }
    }
    out
}

/// Read one token, returning its printable value.
fn read_token(
    token: &str,
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<String> {
    Ok(match token {
        "B" => format!("{}", c.read_b()?),
        "BB" => format!("{}", c.read_bb()?),
        "3B" => format!("{}", c.read_3b()?),
        "BS" => format!("{}", c.read_bs()?),
        "BSU" => format!("{}", c.read_bs_u()?),
        "BL" => format!("{}", c.read_bl()?),
        "BLU" => format!("{}", c.read_bl_u()?),
        "BD" => format!("{}", c.read_bd()?),
        "RC" => format!("{}", c.read_rc()?),
        "RS" => format!("{}", c.read_rs()?),
        "RL" => format!("{}", c.read_rl()?),
        "RD" => format!("{}", c.read_rd()?),
        "CMC" => {
            let index = c.read_bs()?;
            if !version.is_r2004_plus() {
                // §2.11 pre-R2004: a colour is a bare `BS` index.
                format!("idx {index}")
            } else {
                let rgb = c.read_bl_u()?;
                let flag = c.read_rc()?;
                if flag & 1 != 0 {
                    let _ = read_tv(c, strings)?;
                }
                if flag & 2 != 0 {
                    let _ = read_tv(c, strings)?;
                }
                format!("idx {index} rgb {rgb:#010X} flag {flag}")
            }
        }
        "TV" => format!("{:?}", read_tv(c, strings)?),
        other => panic!("unknown field token {other}"),
    })
}

/// One token as replayed: its code, bit offset, width and decoded value.
struct Slot {
    token: String,
    offset: usize,
    width: usize,
    value: String,
}

/// What one candidate field list did to one record.
struct Replay {
    slots: Vec<Slot>,
    /// Bits between where the list ended and where the record says it
    /// must end; `None` when a token failed to decode at all.
    delta: Option<isize>,
    /// Why the replay stopped early, when it did.
    failure: Option<String>,
}

/// Replay one candidate field list against one record.
fn replay(record: &Record, tokens: &[String]) -> Replay {
    let mut strings = match string_stream::locate(&record.raw, record.version) {
        Some(stream) => StringReader::new(&record.raw, stream).ok(),
        None if record.version.is_r2007_plus() => Some(StringReader::empty(&record.raw)),
        None => None,
    };
    let mut c = BitCursor::new(&record.raw);
    if seek(&mut c, record.body).is_err() {
        return Replay {
            slots: Vec::new(),
            delta: None,
            failure: Some("cannot reach the record body".into()),
        };
    }

    let mut slots = Vec::with_capacity(tokens.len());
    for token in tokens {
        let at = c.position_bits();
        match read_token(token, &mut c, &mut strings, record.version) {
            Ok(value) => slots.push(Slot {
                token: token.clone(),
                offset: at,
                width: c.position_bits() - at,
                value,
            }),
            Err(e) => {
                return Replay {
                    slots,
                    delta: None,
                    failure: Some(format!("{token} @{at}: {e}")),
                };
            }
        }
    }
    let at = c.position_bits();
    Replay {
        slots,
        delta: record.data_end.map(|e| at as isize - e as isize),
        failure: None,
    }
}

/// Advance `c` to absolute bit `bit` (the cursor has no random access).
fn seek(c: &mut BitCursor<'_>, bit: usize) -> Result<()> {
    while c.position_bits() < bit {
        let _ = c.read_b()?;
    }
    Ok(())
}

/// Render the bits of `payload` from `from` up to `to` as `0`/`1`.
fn bits_between(payload: &[u8], from: usize, to: usize) -> String {
    (from..to)
        .map(|bit| {
            let byte = payload.get(bit / 8).copied().unwrap_or(0);
            if (byte >> (7 - (bit % 8))) & 1 != 0 {
                '1'
            } else {
                '0'
            }
        })
        .collect()
}

/// Collect every VISUALSTYLE record of one file.
fn collect(path: &str) -> Result<(Version, Vec<Record>)> {
    let file = DwgFile::open(path)?;
    let version = file.version();
    let Some(objects) = file.all_objects() else {
        return Ok((version, Vec::new()));
    };
    let objects = objects?;
    let class_map = file.class_map().and_then(std::result::Result::ok);

    let mut out = Vec::new();
    for object in &objects {
        let ObjectType::Custom(code) = object.kind else {
            continue;
        };
        let name = class_map
            .as_ref()
            .and_then(|m| m.by_type_code(code))
            .map(|d| d.dxf_class_name.clone())
            .unwrap_or_default();
        if !name.eq_ignore_ascii_case("VISUALSTYLE")
            && !name.eq_ignore_ascii_case("ACDBVISUALSTYLE")
        {
            continue;
        }

        let mut c = dwg::object::body_cursor(object, version)?;
        skip_eed(&mut c)?;
        // R13/R14 write the object-data size inside the common object
        // data rather than in the object prologue, so the boundary is
        // only knowable once the prefix has been read.
        let data_end = if matches!(version, Version::R14) {
            Some(c.read_rl()? as usize)
        } else {
            dwg::object::data_end_bit(object, version)
        };
        let _num_reactors = c.read_bl()?;
        if version.is_r2004_plus() {
            let _ = c.read_b()?;
        }
        if matches!(version, Version::R2013 | Version::R2018) && c.read_b()? {
            let _ = c.read_rc()?;
            let _ = c.read_rc()?;
        }
        out.push(Record {
            handle: object.handle.value,
            version,
            raw: object.raw.clone(),
            body: c.position_bits(),
            data_end,
        });
    }
    Ok((version, out))
}

/// How many of `records` a candidate lands exactly on its boundary.
fn closes(records: &[Record], tokens: &[String]) -> usize {
    records
        .iter()
        .filter(|r| replay(r, tokens).delta == Some(0))
        .count()
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let mut spec = String::new();
    let mut verbose = false;
    let mut search = false;
    let mut paths = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--spec" => spec = args.next().expect("--spec needs a field spec"),
            "--verbose" => verbose = true,
            "--search" => search = true,
            other => paths.push(other.to_string()),
        }
    }
    let tokens = expand(&spec);

    let mut totals: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut all: Vec<Record> = Vec::new();
    let mut any_missed = false;

    for path in &paths {
        let (version, records) = collect(path)?;
        println!("{path}  ({version})");
        let mut closed = 0usize;
        for record in &records {
            if tokens.is_empty() {
                println!(
                    "  handle {:<6} budget {:<7}",
                    record.handle,
                    record
                        .data_end
                        .map(|e| e as isize - record.body as isize)
                        .unwrap_or(isize::MIN)
                );
                continue;
            }
            let run = replay(record, &tokens);
            if run.delta == Some(0) {
                closed += 1;
            } else {
                any_missed = true;
            }
            let head = run
                .slots
                .first()
                .map(|slot| slot.value.clone())
                .unwrap_or_default();
            print!(
                "  handle {:<6} budget {:<7} delta {:<6} {head}",
                record.handle,
                record
                    .data_end
                    .map(|e| e as isize - record.body as isize)
                    .unwrap_or(isize::MIN),
                run.delta
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "?".into()),
            );
            if let Some(message) = &run.failure {
                print!("  FAILED {message}");
            } else if let (Some(end), Some(d)) = (record.data_end, run.delta) {
                if d < 0 {
                    let at = (end as isize + d) as usize;
                    print!("  left {:?}", bits_between(&record.raw, at, end));
                }
            }
            println!();
            if verbose {
                for (index, slot) in run.slots.iter().enumerate() {
                    println!(
                        "      [{index:>3}] {:<4} @{:<5} w{:<3} = {}",
                        slot.token, slot.offset, slot.width, slot.value
                    );
                }
            }
        }
        if !tokens.is_empty() {
            println!(
                "  -> {closed}/{} records close on their data-stream boundary",
                records.len()
            );
        }
        let entry = totals.entry(format!("{version}")).or_default();
        entry.0 += 1;
        entry.1 += records.len();
        entry.2 += closed;
        all.extend(records);
    }

    println!(
        "\n{:<18} {:>5} {:>8} {:>8}",
        "release", "files", "records", "closed"
    );
    let (mut all_records, mut all_closed) = (0usize, 0usize);
    for (release, (files, records, closed)) in &totals {
        println!("{release:<18} {files:>5} {records:>8} {closed:>8}");
        all_records += records;
        all_closed += closed;
    }
    println!("{:<18} {:>5} {all_records:>8} {all_closed:>8}", "TOTAL", "");

    if search && !tokens.is_empty() {
        run_search(&all, &tokens);
    }

    Ok(if tokens.is_empty() || !any_missed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Measure every one-token neighbour of `tokens` against every record.
fn run_search(records: &[Record], tokens: &[String]) {
    let target = records.len();
    println!("\nuniqueness search over {target} records — neighbours that also close:");
    let base = closes(records, tokens);
    println!("  base                       {base}/{target}");

    let mut hits = 0usize;
    // Insertions.
    for position in 0..=tokens.len() {
        for token in SEARCH_ALPHABET {
            let mut candidate = tokens.to_vec();
            candidate.insert(position, token.to_string());
            if closes(records, &candidate) == target {
                println!("  insert {token:<4} at {position:<3}      {target}/{target}");
                hits += 1;
            }
        }
    }
    // Deletions.
    for position in 0..tokens.len() {
        let mut candidate = tokens.to_vec();
        let removed = candidate.remove(position);
        if closes(records, &candidate) == target {
            println!("  delete {removed:<4} at {position:<3}      {target}/{target}");
            hits += 1;
        }
    }
    // Substitutions.
    for position in 0..tokens.len() {
        for token in SEARCH_ALPHABET {
            if tokens[position] == token {
                continue;
            }
            let mut candidate = tokens.to_vec();
            candidate[position] = token.to_string();
            if closes(records, &candidate) == target {
                println!(
                    "  replace {:<4} at {position:<3} with {token:<4} {target}/{target}",
                    tokens[position]
                );
                hits += 1;
            }
        }
    }
    println!("  {hits} neighbouring token sequences also close on every record");
}
