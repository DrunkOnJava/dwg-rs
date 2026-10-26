//! Declared-vs-walked object census per custom class.
//!
//! `AcDb:Classes` records a `num_objects` count for every custom class
//! in the drawing (DXF group 91), which makes the class table a cheap
//! oracle for handle-map / walker completeness: if the walk reaches
//! fewer objects of a class than the file declares, the walker is
//! missing records.
//!
//! ```bash
//! cargo run --release --example probe_class_census -- file.dwg [--strict] [--twin file.dxf]
//! ```
//!
//! Exit code is non-zero when any class is under-walked, so the probe
//! doubles as a regression gate — CI runs it over the canonical corpus
//! (`.github/workflows/ci.yml`, `class-census` job).
//!
//! # The allowlist (#56, #76)
//!
//! Four classes of the sample corpus report a shortfall today, and all
//! four are files that declare instances they do not contain rather
//! than records the walk misses — `examples/probe_reference_closure`
//! measures that directly. [`ALLOWLIST`] names each with its reason and
//! the probe reports them as `ALLOWED` instead of failing; `--strict`
//! disables it so the numbers stay visible, and is what CI runs over
//! the canonical fixtures.
//!
//! # The DXF twin (#230)
//!
//! `--twin` takes AutoCAD's DXF export of the same drawing and adds three
//! columns: how many records of each class the export holds, how many of
//! those have a handle the drawing's handle map does not list (`export-only`),
//! and the instance count the export's own CLASSES section gives the
//! class (group 91).
//!
//! The export is an independent census of what the drawing contains, so
//! with a twin an allowlisted shortfall is excused only when the export
//! holds no more records of the class than the walk reached, not counting
//! export-only ones, and a missing, unreadable or malformed twin fails the run.
//! The handle map, not the walk, says what the drawing holds, so a record the
//! walker skipped cannot pass for export-only data and hide its own shortfall.
//! A malformed twin is one whose lines are not whole group pairs, whose group
//! codes are not numbers, whose sections do not open and close in order, that
//! lacks an ENTITIES or an OBJECTS section or its closing `0 EOF`, or that has
//! data after the EOF; an empty or truncated export would otherwise census
//! every class at zero. The crate's `dxf_reader` parses and validates the
//! grammar but sits behind a feature the CI builds of this probe do not enable.
//! Export-only records are data the export adds: on the nextgis R2010
//! saves it gives the TABLESTYLE (`0x65`) an xdictionary `0xD2` holding a
//! round-trip CELLSTYLEMAP `0xD3`. Neither handle is an object of the
//! DWG, and `probe_reference_closure` finds no record of the drawing
//! referring to a handle it does not hold. The export also gives some
//! records handles the DWG does not use (one of the two TABLECONTENT
//! records of each ACadSharp save), so the rule bounds the export from
//! above rather than asking for equality.
//!
//! Wherever the corpus has a twin, no shortfall has the export holding
//! more records of the class than the walk reached, besides export-only
//! ones, while the two class tables disagree with each other:
//! `sample_AC1018.dwg` declares 9 TABLECONTENT, its export's table says
//! 16, and the drawing and the export both hold 2 records.

use dwg::error::Result;
use dwg::{DwgFile, ObjectType};
use std::collections::{BTreeMap, BTreeSet};
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
///
/// The R2004, R2007 and R2010 files of the corpus each declare more
/// DICTIONARYVAR and CELLSTYLEMAP instances than they contain, and #76
/// settled that it is the count and not the walk that is wrong. The
/// `AcDbVariableDictionary` is the sole owner of a drawing's
/// DICTIONARYVAR objects, and it holds exactly as many keys as the walk
/// finds DICTIONARYVAR records on **every** corpus file — 10 / 6 / 5 on
/// the three bands that under-declare, and 5 / 11 on `arc_2013.dwg` and
/// `sample_AC1032.dwg`, whose class tables do agree with the walk.
/// Meanwhile the object stream tiles completely with zero inter-record
/// bytes and every hard handle reference resolves, so there is neither
/// room for the surplus records nor anything pointing at them.
/// `arc_2010.dwg` settles CELLSTYLEMAP the same way: it declares one,
/// contains none, and has no
/// `ACAD_ROUNDTRIP_2008_TABLESTYLE_CELLSTYLEMAP` dictionary key for one
/// to hang from. Run `examples/probe_reference_closure` to reproduce.
/// The counts stay pinned per file by `tests/class_census.rs`.
///
/// The public corpus (#225) added three more, each settled by AutoCAD's
/// DXF export of the same drawing (`--twin`, #230). ACadSharp's R2004 and
/// R2007 saves declare 2 ACDB_TEXTOBJECTCONTEXTDATA_CLASS and contain
/// none, and their exports hold none either, while the R2010-R2018 saves
/// and their exports hold the 2. `geoloc.dwg` declares 2 GEODATA and 1
/// AcGeoBackupObject and holds 1 and 0; its export holds 1 and 0, and its
/// own class table counts them 1 and 0. The same twins hold exactly 2
/// TABLECONTENT and 2 TABLEGEOMETRY records on every release from R2004.
pub const ALLOWLIST: &[(&str, &str)] = &[
    (
        "TABLECONTENT",
        "declares 5 to 9 against 2 ACAD_TABLE entities; the object stream is \
         99.92 % covered, no handle reference is unresolved (#56), and \
         AutoCAD's DXF export of each save holds 2 (#230)",
    ),
    (
        "TABLEGEOMETRY",
        "declares 5 to 9 against 2 ACAD_TABLE entities; the object stream is \
         99.92 % covered, no handle reference is unresolved (#56), and \
         AutoCAD's DXF export of each save holds 2 (#230)",
    ),
    (
        "ACDB_TEXTOBJECTCONTEXTDATA_CLASS",
        "the R2004 and R2007 saves declare 2 and hold none, and so do \
         AutoCAD's DXF exports of them; the R2010+ saves and exports hold 2 (#230)",
    ),
    (
        "GEODATA",
        "geoloc.dwg declares 2 and holds 1; AutoCAD's DXF export holds 1 \
         and its class table counts 1 (#230)",
    ),
    (
        "AcGeoBackupObject",
        "geoloc.dwg declares 1 and holds none; AutoCAD's DXF export holds \
         none and its class table counts 0 (#230)",
    ),
    (
        "DICTIONARYVAR",
        "the R2004/R2007/R2010 files declare 16/11/10 against an \
         AcDbVariableDictionary holding 10/6/5 keys, which is the whole \
         population; the stream tiles with zero inter-record bytes (#76)",
    ),
    (
        "CELLSTYLEMAP",
        "the R2004/R2007/R2010 files declare 5/3/1 against 1/1/0 \
         ACAD_ROUNDTRIP_2008_TABLESTYLE_CELLSTYLEMAP dictionary keys, \
         the only slot such an object can hang from (#76)",
    ),
];

/// The handle of every record per DXF name in a DXF export, and the
/// instance count its CLASSES section gives each class (group 91), keyed
/// by normalized name.
#[derive(Default)]
struct Twin {
    records: BTreeMap<String, Vec<Option<u64>>>,
    class_counts: BTreeMap<String, usize>,
}

fn read_twin(path: &str) -> Result<Twin, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() % 2 == 1 {
        return Err(format!(
            "{} lines are not whole group pairs; the file is truncated",
            lines.len()
        ));
    }
    let mut twin = Twin::default();
    let mut class_name: Option<String> = None;
    let mut in_class = false;
    let mut current: Option<String> = None;
    let mut open_section: Option<Option<String>> = None;
    let mut sections = BTreeSet::new();
    let mut saw_eof = false;
    for (n, pair) in lines.chunks_exact(2).enumerate() {
        let line = 2 * n + 1;
        let (code, value) = (pair[0].trim(), pair[1].trim());
        let numeric: i64 = code
            .parse()
            .map_err(|_| format!("line {line}: group code {code:?} is not a number"))?;
        if saw_eof {
            return Err(format!("line {line}: data after EOF"));
        }
        if numeric == 999 {
            continue;
        }
        if numeric != 0 {
            match &mut open_section {
                Some(name @ None) if numeric == 2 => {
                    sections.insert(normalize(value));
                    *name = Some(normalize(value));
                }
                Some(None) => return Err(format!("line {line}: SECTION is not followed by 2")),
                None => return Err(format!("line {line}: group {numeric} outside any section")),
                Some(Some(_)) => {}
            }
        } else {
            match (value, &open_section) {
                ("SECTION", None) => open_section = Some(None),
                ("SECTION", Some(_)) => {
                    return Err(format!("line {line}: SECTION inside a section"));
                }
                (_, Some(None)) => {
                    return Err(format!("line {line}: SECTION is not followed by 2"));
                }
                ("ENDSEC", Some(Some(_))) => open_section = None,
                ("ENDSEC", None) => {
                    return Err(format!("line {line}: ENDSEC with no open section"));
                }
                ("EOF", None) => saw_eof = true,
                ("EOF", Some(_)) => return Err(format!("line {line}: EOF inside a section")),
                (_, Some(Some(_))) => {}
                (_, None) => return Err(format!("line {line}: {value} outside any section")),
            }
            in_class = value == "CLASS";
            class_name = None;
            let name = normalize(value);
            twin.records.entry(name.clone()).or_default().push(None);
            current = Some(name);
            continue;
        }
        if code == "5" || code == "105" {
            // The record's own handle is its first `5` (`105` on DIMSTYLE).
            if let Some(slot) = current
                .as_ref()
                .and_then(|n| twin.records.get_mut(n))
                .and_then(|v| v.last_mut())
                .filter(|h| h.is_none())
            {
                *slot = u64::from_str_radix(value, 16).ok();
            }
        } else if in_class && code == "1" {
            class_name = Some(normalize(value));
        } else if in_class && code == "91" {
            if let (Some(name), Ok(n)) = (class_name.as_ref(), value.parse()) {
                twin.class_counts.insert(name.clone(), n);
            }
        }
    }
    if !saw_eof {
        return Err("no closing EOF; the file is empty or truncated".to_string());
    }
    for needed in ["ENTITIES", "OBJECTS"] {
        if !sections.contains(needed) {
            return Err(format!("no {needed} section"));
        }
    }
    Ok(twin)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(path) = args.first().cloned() else {
        eprintln!("usage: probe_class_census <file.dwg> [--strict] [--twin <file.dxf>]");
        return ExitCode::FAILURE;
    };
    let strict = args.iter().any(|a| a == "--strict");
    let twin = match args.iter().position(|a| a == "--twin") {
        Some(i) => match args.get(i + 1).map(|p| read_twin(p)) {
            Some(Ok(twin)) => Some(twin),
            Some(Err(e)) => {
                eprintln!("cannot read the DXF twin: {e}");
                return ExitCode::FAILURE;
            }
            None => {
                eprintln!("--twin needs a DXF path");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    match run(&path, strict, twin.as_ref()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("probe failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str, strict: bool, twin: Option<&Twin>) -> Result<bool> {
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
                // MS is 2 bytes per 15-bit module, and `raw` spans the
                // whole record payload — including the R2010+ leading
                // `MC` the `MS` does not count (#77).
                let ms_len = if o.size_bytes < 0x8000 { 2 } else { 4 };
                (o.stream_offset, o.stream_offset + ms_len + o.raw.len() + 2)
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

        // Where are the unclaimed bytes? A record run that is neither
        // the section prologue nor the tail after the last record is
        // room a record the walk never reached could be hiding in, so
        // it is the number that decides whether an under-walked class
        // is a walker gap or a file that over-declares.
        //
        // Before #77 every R2010+ inter-record run held the width of
        // the preceding record's leading `MC` handle-stream-size field
        // — the bytes a record's `MS` does not count — and the walker
        // sliced them off. The slice now spans the whole record, so
        // every band reports zero inter-record bytes.
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
            "unclaimed bytes located    : {leading} leading (section prologue), \
             {inter} between records in {runs} run(s), {trailing} trailing"
        );
    }
    println!();
    let twin_header = if twin.is_some() {
        format!(" {:>6} {:>11} {:>8}", "twin", "export-only", "twin 91")
    } else {
        String::new()
    };
    let Some(handle_map) = file.handle_map().transpose()? else {
        println!("no handle map to tell the export's records from the drawing's");
        return Ok(false);
    };
    let in_drawing: std::collections::HashSet<u64> = handle_map.iter().map(|e| e.handle).collect();
    println!(
        "{:<38} {:>6} {:>9} {:>7} {:>7} {:>7}{twin_header}",
        "class", "code", "declared", "walked", "fixed", "delta"
    );
    println!("{}", "-".repeat(if twin.is_some() { 108 } else { 80 }));

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
        let key = normalize(&def.dxf_class_name);
        // The export's records of this class, and how many of them have a
        // handle the drawing's handle map does not list: data the export writes
        // and the DWG does not hold, such as the round-trip objects AutoCAD
        // adds when it writes an older release.
        let twin_records = twin.map(|t| {
            let handles = t.records.get(&key).map(Vec::as_slice).unwrap_or(&[]);
            let export_only = handles
                .iter()
                .filter(|h| h.is_some_and(|h| !in_drawing.contains(&h)))
                .count();
            (handles.len(), export_only)
        });
        // With a twin, a shortfall is excused only when the export holds no
        // more records of the class than the walk reached, besides its
        // export-only ones: the allowlist names the classes, and the twin
        // has to agree for this file.
        let excused = !strict
            && delta < 0
            && twin_records.is_none_or(|(n, export_only)| n - export_only <= seen + fixed)
            && ALLOWLIST.iter().any(|(name, _)| normalize(name) == key);
        if delta < 0 && !excused {
            complete = false;
        }
        if excused {
            allowed.push(def.dxf_class_name.clone());
        }
        let twin_cells = match (twin, twin_records) {
            (Some(t), Some((records, export_only))) => {
                let counted = t
                    .class_counts
                    .get(&key)
                    .map_or_else(|| "-".to_string(), |n| n.to_string());
                format!(" {records:>6} {export_only:>11} {counted:>8}")
            }
            _ => String::new(),
        };
        println!(
            "{:<38} {:>6} {:>9} {:>7} {:>7} {:>+7}{twin_cells} {}",
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

/// Fold a class name and a built-in type name into a comparable key:
/// the class table writes `ACDBPLACEHOLDER` where the built-in type
/// table writes `ACDB_PLACEHOLDER`.
fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_uppercase())
        .collect()
}
