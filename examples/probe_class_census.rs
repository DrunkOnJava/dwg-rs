//! Declared-vs-walked object census per custom class.
//!
//! `AcDb:Classes` records a `num_objects` count for every custom class
//! in the drawing (DXF group 91), which makes the class table a cheap
//! oracle for handle-map / walker completeness: if the walk reaches
//! fewer objects of a class than the file declares, the walker is
//! missing records.
//!
//! ```bash
//! cargo run --release --example probe_class_census -- file.dwg [--strict]
//! ```
//!
//! Exit code is non-zero when any class is under-walked, so the probe
//! doubles as a regression gate — CI runs it over the canonical corpus
//! (`.github/workflows/ci.yml`, `class-census` job).
//!
//! # The allowlist (#56)
//!
//! Two classes of `sample_AC1032.dwg` declare more instances than the
//! drawing contains. [`ALLOWLIST`] names them with the reason and the
//! probe reports them as `ALLOWED` instead of failing; `--strict`
//! disables it so the open question stays measurable.

use dwg::error::Result;
use dwg::{DwgFile, ObjectType};
use std::collections::BTreeMap;
use std::env;
use std::process::ExitCode;

/// Classes whose declared `num_objects` is known not to be a live
/// census on the sample corpus, with the reason each is tolerated.
///
/// `sample_AC1032.dwg` (AutoCAD 2025 output) declares 5 TABLECONTENT
/// and 5 TABLEGEOMETRY against 2 ACAD_TABLE entities, and only 2 of
/// each exist. Three independent measurements say the missing six
/// records are not in the drawing:
///
/// * the 842 walked records cover 1,191,935 of the object stream's
///   1,192,851 bytes and the longest unclaimed run is 4 bytes, so no
///   standalone record is hiding in the stream;
/// * decoding every record's trailing handle stream yields no
///   reference to a handle the walk cannot answer, so nothing in the
///   drawing points at a missing content or geometry object;
/// * ACAD_TABLE itself declares 2 and walks 2, so the drawing holds
///   exactly two tables — and TABLECONTENT / TABLEGEOMETRY are 1:1
///   with a table, which cannot yield 2.5 instances each.
///
/// Whether the surplus three are a stale registration count or copies
/// embedded inside the two ACAD_TABLE records is not decidable from a
/// one-file, two-table corpus, so the gate tolerates a *shortfall* on
/// exactly these two classes and never a surplus.
pub const ALLOWLIST: &[(&str, &str)] = &[
    (
        "TABLECONTENT",
        "declares 5 against 2 ACAD_TABLE entities; the object stream is \
         99.92 % covered and no handle reference is unresolved (#56)",
    ),
    (
        "TABLEGEOMETRY",
        "declares 5 against 2 ACAD_TABLE entities; the object stream is \
         99.92 % covered and no handle reference is unresolved (#56)",
    ),
];

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: probe_class_census <file.dwg> [--strict]");
        return ExitCode::FAILURE;
    };
    let strict = env::args().any(|a| a == "--strict");
    match run(&path, strict) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("probe failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str, strict: bool) -> Result<bool> {
    let file = DwgFile::open(path)?;
    println!("file    : {path}");
    println!("version : {}", file.version());

    // A drawing with no custom classes has nothing to census, and the
    // canonical fixtures are exactly that. `class_map()` reports a
    // missing section as a parse error rather than `None`, so ask the
    // section map directly before treating an error as a failure.
    if file.section_by_name("AcDb:Classes").is_none() {
        println!("no AcDb:Classes section — nothing to census");
        return Ok(true);
    }
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

        // What are the unclaimed bytes? Not alignment padding — they
        // are not zero. Each run equals the *width of the leading `MC`
        // handle-stream-size field* of the record it follows, which is
        // what a record's `MS` object size does not count. Reported as
        // an agreement count so a regression shows up as a
        // disagreement rather than as a changed byte total.
        let mut by_offset: Vec<(usize, usize)> = objects
            .iter()
            .map(|o| (o.stream_offset, mc_field_width(&o.raw)))
            .collect();
        by_offset.sort_unstable();
        let mc_widths: Vec<usize> = by_offset.iter().map(|(_, w)| *w).collect();

        let mut agree = 0usize;
        let mut disagree = 0usize;
        let mut end = 0usize;
        let mut leading = 0usize;
        for (i, (s, e)) in spans.iter().enumerate() {
            if i == 0 {
                leading = *s;
            } else if s.saturating_sub(end) == mc_widths[i - 1] {
                agree += 1;
            } else {
                disagree += 1;
            }
            end = end.max(*e);
        }
        let trailing = stream.len().saturating_sub(end);
        let attributed: usize = mc_widths.iter().sum::<usize>() + leading;
        println!(
            "unclaimed bytes explained : {attributed} of {unclaimed} \
             ({agree} inter-record runs equal the preceding record\'s MC width, \
             {disagree} do not; {leading} leading, {trailing} trailing)"
        );
    }
    println!();
    println!(
        "{:<38} {:>6} {:>9} {:>7} {:>7} {:>7}",
        "class", "code", "declared", "walked", "fixed", "delta"
    );
    println!("{}", "-".repeat(80));

    let mut complete = true;
    let mut allowed = Vec::new();
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
        let excused = !strict
            && delta < 0
            && ALLOWLIST
                .iter()
                .any(|(name, _)| normalize(name) == normalize(&def.dxf_class_name));
        if delta < 0 && !excused {
            complete = false;
        }
        if excused {
            allowed.push(def.dxf_class_name.clone());
        }
        println!(
            "{:<38} {:>6} {:>9} {:>7} {:>7} {:>+7} {}",
            def.dxf_class_name,
            def.class_number,
            declared,
            seen,
            fixed,
            delta,
            if excused { "ALLOWED" } else { "" }
        );
    }

    println!();
    for name in &allowed {
        if let Some((_, reason)) = ALLOWLIST
            .iter()
            .find(|(n, _)| normalize(n) == normalize(name))
        {
            println!("ALLOWED {name}: {reason}");
        }
    }
    if complete {
        println!("RESULT: every class reached its declared num_objects.");
    } else {
        println!("RESULT: at least one class is under-walked (negative delta above).");
    }
    Ok(complete)
}

/// Width in bytes of a record\'s leading `MC` handle-stream-size field.
fn mc_field_width(raw: &[u8]) -> usize {
    for (i, b) in raw.iter().enumerate().take(10) {
        if b & 0x80 == 0 {
            return i + 1;
        }
    }
    0
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
