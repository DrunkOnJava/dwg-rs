//! Is the object walk complete? Three independent measurements (#76).
//!
//! # What this proves
//!
//! `examples/probe_class_census` compares each custom class's declared
//! `num_objects` against the number of instances the walk reaches. When
//! it reports a shortfall there are exactly two explanations, and they
//! call for opposite responses:
//!
//! * the walk is missing records — a walker bug to fix; or
//! * the drawing declares instances it does not contain — a file fact
//!   to document.
//!
//! This probe decides between them from the bytes, without consulting
//! the class table at all. It reports three things, each of which would
//! have to be violated for a record to be hiding from the walk:
//!
//! 1. **Stream tiling.** Every walked record's span
//!    (`MS` + payload + CRC) is laid end to end over the
//!    `AcDb:AcDbObjects` bytes. Bytes between two records are room an
//!    unreferenced record could occupy; the section prologue and the
//!    tail after the last record are not.
//! 2. **Reference closure.** Every record's trailing handle stream is
//!    decoded and each reference resolved against `AcDb:Handles`. An
//!    unanswered *hard* reference (§2.13 codes 3 and 5) is a pointer to
//!    a record the walk cannot reach; a soft one (codes 2 and 4) is
//!    allowed to dangle by definition, so it is reported and tolerated.
//! 3. **Owner census.** Every DICTIONARY record is decoded and its
//!    entry count summed. A dictionary key is the only way a
//!    dictionary-owned object is reachable from the drawing, so the key
//!    total bounds how many such objects the drawing can hold.
//!
//! # How to verify
//!
//! ```bash
//! cargo run --release --example probe_reference_closure -- file.dwg
//! ```
//!
//! Exit code is non-zero if any measurement is open — inter-record
//! bytes exist, or a hard handle reference goes unanswered.
//!
//! # Measured (2026-08-30)
//!
//! On all nine R2004 / R2007 / R2010 corpus files that
//! `probe_class_census` reports as under-walking DICTIONARYVAR and
//! CELLSTYLEMAP, all three measurements are closed: zero inter-record
//! bytes, zero unanswered references of any kind, and the
//! `AcDbVariableDictionary` holds exactly as many keys as the walk
//! finds DICTIONARYVAR records (10 / 6 / 5 by release band). The same
//! equality holds on the two files whose class table *does* agree with
//! the walk (`arc_2013.dwg` 5, `sample_AC1032.dwg` 11), so the class
//! table is the outlier, not the walk.
//!
//! `sample_AC1032.dwg` is the only corpus file with any unanswered
//! reference at all: BLOCK_HEADER `0xA0B` lists ten owned entities and
//! six of them — `0xD17`, `0xD18`, `0xD40`, `0xD41`, `0xD95`, `0xD96`
//! — have no record. All six are code 4 soft pointers, the reference
//! class §2.13 permits to dangle, and the four that do resolve are
//! INSERT entities; the drawing's handle space is sparse (2,841 of the
//! 3,683 values in `0x1..=0xE63` are unused), so these are erased
//! entities the block's list still names. Every hard reference in the
//! corpus resolves. This refines #56's "no reference the walk cannot
//! answer", which was measured before the soft/hard split was drawn.

use dwg::bitcursor::BitCursor;
use dwg::entities::{DecodedEntity, decode_from_raw};
use dwg::error::Result;
use dwg::object::RawObject;
use dwg::{DwgFile, ObjectType, Version};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::process::ExitCode;

/// DICTIONARY object type code (spec §19.4 table).
const TYPE_DICTIONARY: u16 = 0x2A;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_reference_closure <file.dwg>");
        return ExitCode::FAILURE;
    };
    match run(&path) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("probe failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str) -> Result<bool> {
    let file = DwgFile::open(path)?;
    let version = file.version();
    println!("file    : {path}");
    println!("version : {version}");

    let Some((objects, walk)) = file.all_objects_lossy().transpose()? else {
        println!("no handle-map-driven walk available for this file");
        return Ok(true);
    };
    let map = match file.handle_map() {
        Some(Ok(m)) => m,
        _ => {
            println!("no AcDb:Handles section");
            return Ok(true);
        }
    };
    let known: BTreeSet<u64> = map.entries.iter().map(|e| e.handle).collect();
    println!("records walked            : {}", objects.len());
    println!("walker skips              : {}", walk.skipped.len());
    println!(
        "handle self-check misses  : {}",
        walk.handle_mismatches.len()
    );

    let mut closed = true;

    // ---- 1. stream tiling -------------------------------------------
    if let Some(Ok(stream)) = file.read_section("AcDb:AcDbObjects") {
        let mut spans: Vec<(usize, usize)> = objects
            .iter()
            .map(|o| {
                let ms_len = if o.size_bytes < 0x8000 { 2 } else { 4 };
                (o.stream_offset, o.stream_offset + ms_len + o.raw.len() + 2)
            })
            .collect();
        spans.sort_unstable();
        let mut inter = 0usize;
        let mut runs = 0usize;
        let mut end = 0usize;
        let mut leading = 0usize;
        for (i, (s, e)) in spans.iter().enumerate() {
            if i == 0 {
                leading = *s;
            } else if *s > end {
                inter += s - end;
                runs += 1;
            }
            end = end.max(*e);
        }
        let trailing = stream.len().saturating_sub(end);
        println!(
            "stream tiling             : {} of {} bytes claimed; {leading} leading, \
             {inter} between records in {runs} run(s), {trailing} trailing",
            end.saturating_sub(leading),
            stream.len()
        );
        if inter > 0 {
            closed = false;
        }
    }

    // ---- 2. reference closure ---------------------------------------
    let mut refs = 0usize;
    let mut no_boundary = 0usize;
    let mut unanswered: BTreeMap<u64, Vec<(u64, u8)>> = BTreeMap::new();
    for obj in &objects {
        let Some(start) = handle_stream_start(obj, version) else {
            no_boundary += 1;
            continue;
        };
        for (target, code) in handle_stream_targets(obj, start) {
            refs += 1;
            if !known.contains(&target) {
                unanswered
                    .entry(target)
                    .or_default()
                    .push((obj.handle.value, code));
            }
        }
    }
    // §2.13 splits references into hard (codes 3 and 5), which the
    // owner cannot exist without, and soft (codes 2 and 4), which are
    // explicitly allowed to point at an object that is no longer in the
    // drawing. Only an unanswered *hard* reference proves a record is
    // missing from the walk.
    let hard: usize = unanswered
        .values()
        .flatten()
        .filter(|(_, code)| matches!(code, 3 | 5))
        .count();
    println!(
        "reference closure         : {refs} references decoded, {} unanswered target(s), \
         {hard} of them hard ({no_boundary} record(s) expose no handle-stream boundary)",
        unanswered.len()
    );
    for (target, sources) in unanswered.iter().take(20) {
        let from: Vec<String> = sources
            .iter()
            .map(|(h, code)| format!("0x{h:X} (code {code})"))
            .collect();
        println!(
            "  unanswered handle 0x{target:X} referenced from {}",
            from.join(", ")
        );
    }
    if hard > 0 {
        closed = false;
    }

    // ---- 3. owner census --------------------------------------------
    let mut keys = 0usize;
    let mut dicts = 0usize;
    let mut variable_dict: Option<(u64, usize)> = None;
    for obj in &objects {
        if obj.type_code != TYPE_DICTIONARY {
            continue;
        }
        dicts += 1;
        if let DecodedEntity::Dictionary(d) = decode_from_raw(obj, version) {
            keys += d.keys.len();
            // The AcDbVariableDictionary is the sole owner of a
            // drawing's DICTIONARYVAR objects; identify it by its
            // membership rather than by handle, which moves per file.
            if d.contains("CANNOSCALE") {
                variable_dict = Some((obj.handle.value, d.keys.len()));
            }
        }
    }
    println!("dictionaries decoded      : {dicts} holding {keys} entries");
    let dictionary_vars = objects
        .iter()
        .filter(|o| matches!(o.kind, ObjectType::Custom(_)) && is_dictionary_var(&file, o))
        .count();
    match variable_dict {
        Some((handle, n)) => println!(
            "AcDbVariableDictionary    : 0x{handle:X} with {n} entries; \
             {dictionary_vars} DICTIONARYVAR record(s) walked"
        ),
        None => println!("AcDbVariableDictionary    : absent"),
    }

    println!();
    if closed {
        println!(
            "RESULT: the walk is closed — no inter-record bytes and no unanswered \
             hard reference, so no record this drawing depends on is unreachable."
        );
    } else {
        println!("RESULT: the walk is open — see the measurements above.");
    }
    Ok(closed)
}

/// Does this record's class number name DICTIONARYVAR in the file's
/// `AcDb:Classes` table?
fn is_dictionary_var(file: &DwgFile, obj: &RawObject) -> bool {
    let Some(Ok(classes)) = file.class_map() else {
        return false;
    };
    classes
        .classes
        .iter()
        .any(|c| c.class_number == obj.type_code && c.dxf_class_name == "DICTIONARYVAR")
}

/// First bit of a record's trailing handle stream: the end of its data
/// (and, on R2007+, string) area.
fn handle_stream_start(obj: &RawObject, version: Version) -> Option<usize> {
    if version.is_r2007_plus() {
        dwg::string_stream::data_section_end(&obj.raw, version)
    } else {
        obj.obj_size_bits.map(|b| b as usize)
    }
}

/// Decode the handle references packed into `obj`'s handle stream and
/// resolve each to an absolute handle. Codes 2..=5 carry the value
/// outright; 6 / 8 are the owner's handle ±1 and 0xA / 0xC add or
/// subtract the encoded offset (spec §2.13).
fn handle_stream_targets(obj: &RawObject, start: usize) -> Vec<(u64, u8)> {
    let total = obj.raw.len() * 8;
    let mut out = Vec::new();
    if start >= total {
        return out;
    }
    let mut c = BitCursor::new(&obj.raw);
    for _ in 0..start {
        if c.read_b().is_err() {
            return out;
        }
    }
    while c.position_bits() + 8 <= total {
        let Ok(h) = c.read_handle() else { break };
        let target = match h.code {
            2..=5 => h.value,
            6 => obj.handle.value.wrapping_add(1),
            8 => obj.handle.value.wrapping_sub(1),
            0xA => obj.handle.value.wrapping_add(h.value),
            0xC => obj.handle.value.wrapping_sub(h.value),
            _ => continue,
        };
        if target != 0 {
            out.push((target, h.code));
        }
    }
    out
}
