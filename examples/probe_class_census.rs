//! Declared-vs-walked object census per custom class.
//!
//! `AcDb:Classes` records a `num_objects` count for every custom class
//! in the drawing (DXF group 91), which makes the class table a cheap
//! oracle for handle-map / walker completeness: if the walk reaches
//! fewer objects of a class than the file declares, the walker is
//! missing records.
//!
//! ```bash
//! cargo run --release --example probe_class_census -- file.dwg
//! ```
//!
//! Exit code is non-zero when any class is under-walked, so the probe
//! doubles as a regression gate.

use dwg::error::Result;
use dwg::{DwgFile, ObjectType};
use std::collections::BTreeMap;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_class_census <file.dwg>");
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
    println!("file    : {path}");
    println!("version : {}", file.version());

    let Some(classes) = file.class_map().transpose()? else {
        println!("no AcDb:Classes section — nothing to census");
        return Ok(true);
    };
    let Some((objects, walk)) = file.all_objects_lossy().transpose()? else {
        println!("no handle-map-driven walk available for this file");
        return Ok(true);
    };

    let mut walked: BTreeMap<u16, usize> = BTreeMap::new();
    // Some classes listed in AcDb:Classes were promoted to fixed object
    // types in later releases (LAYOUT, ACDBPLACEHOLDER, ...). The file
    // still registers the class and still counts its instances, but it
    // writes those instances with the fixed type code, so crediting the
    // custom class number alone under-counts them. Index the fixed-code
    // records by normalized type name and credit both.
    let mut walked_fixed: BTreeMap<String, usize> = BTreeMap::new();
    for obj in &objects {
        match obj.kind {
            ObjectType::Custom(code) => *walked.entry(code).or_default() += 1,
            kind => {
                *walked_fixed
                    .entry(normalize(&kind.to_string()))
                    .or_default() += 1
            }
        }
    }

    println!("handle-map records walked : {}", objects.len());
    println!("walker skips              : {}", walk.skipped.len());
    println!(
        "handle self-check misses  : {}",
        walk.handle_mismatches.len()
    );

    // Byte-coverage of the object stream. If the class table declares
    // instances the walk never reaches, the first question is whether
    // unreferenced records are sitting in stream bytes no handle-map
    // entry addresses; a near-total coverage figure with only
    // inter-record padding left over answers "no".
    if let Some(Ok(stream)) = file.read_section("AcDb:AcDbObjects") {
        let mut spans: Vec<(usize, usize)> = objects
            .iter()
            .map(|o| {
                // MS is 2 bytes per 15-bit module.
                let ms_len = if o.size_bytes < 0x8000 { 2 } else { 4 };
                (
                    o.stream_offset,
                    o.stream_offset + ms_len + o.size_bytes as usize + 2,
                )
            })
            .collect();
        spans.sort_unstable();
        let covered: usize = spans.iter().map(|(s, e)| e - s).sum();
        let mut unclaimed = 0usize;
        let mut largest = 0usize;
        let mut end = 0usize;
        for (s, e) in &spans {
            if *s > end {
                unclaimed += s - end;
                largest = largest.max(s - end);
            }
            end = end.max(*e);
        }
        unclaimed += stream.len().saturating_sub(end);
        largest = largest.max(stream.len().saturating_sub(end));
        println!(
            "object stream coverage    : {covered} of {} bytes; {unclaimed} unclaimed \
             (largest run {largest})",
            stream.len()
        );
    }
    println!();
    println!(
        "{:<38} {:>6} {:>9} {:>7} {:>7} {:>7}",
        "class", "code", "declared", "walked", "fixed", "delta"
    );
    println!("{}", "-".repeat(80));

    let mut complete = true;
    for def in &classes.classes {
        let declared = def.num_objects as usize;
        let seen = walked.get(&def.class_number).copied().unwrap_or(0);
        let fixed = walked_fixed
            .get(&normalize(&def.dxf_class_name))
            .copied()
            .unwrap_or(0);
        if declared == 0 && seen == 0 && fixed == 0 {
            continue;
        }
        let delta = (seen + fixed) as i64 - declared as i64;
        if delta < 0 {
            complete = false;
        }
        println!(
            "{:<38} {:>6} {:>9} {:>7} {:>7} {:>+7}",
            def.dxf_class_name, def.class_number, declared, seen, fixed, delta
        );
    }

    println!();
    if complete {
        println!("RESULT: every class reached its declared num_objects.");
    } else {
        println!("RESULT: at least one class is under-walked (negative delta above).");
    }
    Ok(complete)
}

/// Fold a class name and a built-in type name into a comparable key:
/// the class table writes `ACDBPLACEHOLDER` where the built-in type
/// table writes `ACDB_PLACEHOLDER`.
fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_uppercase())
        .collect()
}
