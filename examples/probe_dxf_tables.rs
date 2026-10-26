//! `dwg-to-dxf`'s TABLES section against AutoCAD's own DXF export (#126).
//!
//! # What this proves
//!
//! For each DWG that has an AutoCAD DXF export of the same drawing, the probe
//! converts it with [`dwg::dxf_convert::convert_dwg_to_dxf`] to the DWG's own
//! release, reads the output back with [`dwg::dxf_reader`] (the converter's
//! output as far as the end of its TABLES section) and pairs every record it
//! wrote with the export's record of the same handle, per table
//! (LAYER, STYLE, LTYPE, APPID). A paired record must agree group by group:
//!
//! - every group written has the export's code and equal values (text and
//!   integers exactly, reals to the precision AutoCAD printed); a group the
//!   export does not have is **extra**;
//! - a group of the export that the record lacks is **omitted**, and needs a
//!   stated reason: a LAYER's linetype name (6), plot-style handle (390) and
//!   R2013 material and visual style handles (347, 348), whose handle stream the
//!   decoder does not read, and its 420 and the 62 beside it for a true colour;
//! - a record of the export with no record written is **omitted**: one created
//!   by the export itself (a handle at or above the DWG's `$HANDSEED`: it is not
//!   in the DWG), or a linetype with a shape or text dash. Any other is a failure,
//!   as is a record written that the export does not have, or a table header
//!   whose handle or owner differs.
//!
//! The export's `70` count on a table header is not compared: AutoCAD's count
//! does not always equal its record count.
//!
//! Any disagreement, extra, unexplained omission or unreadable output exits 1.
//! AC1014 drawings are skipped (R12 and R14 targets keep the default layer).
//!
//! # How to verify
//!
//! ```text
//! cargo run --release --features dxf-reader --example probe_dxf_tables -- <file-or-dir> ...
//! ```
//!
//! Directories are searched recursively for `.dwg` and `.dxf` files; a DWG's twin
//! is the DXF with the same stem, or the stem plus `_ascii`. With no DWG at all it
//! prints `NOT MEASURED` and exits 0.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dwg::dxf::DxfVersion;
use dwg::dxf_reader::{DxfDocument, DxfGroup, DxfRecord, DxfValue};
use dwg::{DwgFile, Version};

const TABLES: [&str; 4] = ["LTYPE", "LAYER", "STYLE", "APPID"];

#[derive(Default, Clone)]
struct Tally {
    ours: usize,
    paired: usize,
    groups: usize,
    agree: usize,
    disagree: usize,
    extra: usize,
    omitted_groups: usize,
    omitted_records: usize,
    export_made: usize,
}

fn walk(path: &Path, dwgs: &mut Vec<PathBuf>, dxfs: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let mut kids: Vec<PathBuf> = match std::fs::read_dir(path) {
            Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
            Err(_) => return,
        };
        kids.sort();
        for k in kids {
            walk(&k, dwgs, dxfs);
        }
    } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if ext.eq_ignore_ascii_case("dwg") {
            dwgs.push(path.to_path_buf());
        } else if ext.eq_ignore_ascii_case("dxf") {
            dxfs.push(path.to_path_buf());
        }
    }
}

fn stem_key(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    stem.strip_suffix("_ascii")
        .unwrap_or(stem)
        .to_ascii_lowercase()
}

fn dxf_version(v: Version) -> Option<DxfVersion> {
    Some(match v {
        Version::R14 => return None,
        Version::R2000 => DxfVersion::R2000,
        Version::R2004 => DxfVersion::R2004,
        Version::R2007 => DxfVersion::R2007,
        Version::R2010 => DxfVersion::R2010,
        Version::R2013 => DxfVersion::R2013,
        Version::R2018 => DxfVersion::R2018,
    })
}

fn close(a: f64, b: f64, digits: u8) -> bool {
    if a == b {
        return true;
    }
    let rel = 10f64.powi(-(i32::from(digits.max(2)) - 1)) * 2.0;
    (a - b).abs() <= a.abs().max(b.abs()) * rel
}

/// The output as far as the end of its TABLES section, closed so the reader accepts it.
fn tables_only(out: &str) -> Option<String> {
    let mut at = 0;
    for _ in 0..2 {
        at += out[at..].find("ENDSEC")? + "ENDSEC".len();
    }
    let end = out[at..].find('\n')? + at + 1;
    Some(format!("{}0\nEOF\n", &out[..end]))
}

fn same(a: &DxfValue, b: &DxfValue, digits: u8) -> bool {
    match (a, b) {
        (DxfValue::Real { value: x, .. }, DxfValue::Real { value: y, .. }) => close(*x, *y, digits),
        (x, y) => x == y,
    }
}

fn values(r: &DxfRecord, code: u16) -> Vec<&DxfValue> {
    r.groups
        .iter()
        .filter(|g: &&DxfGroup| g.code == code)
        .map(|g| &g.value)
        .collect()
}

/// Why the converter may omit group `code` of a `table` record the export writes.
fn group_reason(table: &str, code: u16, theirs: &DxfRecord) -> Option<&'static str> {
    match (table, code) {
        (_, 1000..=1071) => Some("extended data is not decoded for table records"),
        (_, 102 | 360) => Some("the extension dictionary handle is in the handle stream"),
        ("LAYER", 430) => Some("the layer's R2007 colour name is not decoded"),
        ("LAYER", 6) => {
            Some("the linetype handle is in the handle stream, which the decoder does not read")
        }
        ("LAYER", 390) => Some("the plot-style handle is in the handle stream"),
        ("LAYER", 347 | 348) => {
            Some("the R2013 material and visual style handles are in the handle stream")
        }
        ("LAYER", 420) => Some("a true colour: the decoder surfaces no RGB"),
        ("LAYER", 62) if !values(theirs, 420).is_empty() => {
            Some("a true colour has no index in the DWG")
        }
        _ => None,
    }
}

struct Outcome {
    tallies: BTreeMap<&'static str, Tally>,
    reasons: BTreeMap<String, usize>,
    failures: Vec<String>,
}

fn probe_file(path: &Path, twins: &BTreeMap<String, PathBuf>) -> Result<Option<Outcome>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let file = DwgFile::from_bytes(bytes).map_err(|e| format!("open: {e}"))?;
    let Some(target) = dxf_version(file.version()) else {
        return Ok(None);
    };
    let Some(twin) = twins.get(&stem_key(path)) else {
        return Ok(None);
    };
    let theirs = DxfDocument::parse(&std::fs::read(twin).map_err(|e| format!("read twin: {e}"))?)
        .map_err(|e| format!("twin: {e}"))?;
    let header = file
        .header_vars_strict()
        .ok_or("no AcDb:Header")?
        .map_err(|e| format!("header: {e}"))?;
    let seed = header.get_handle("$HANDSEED").map_or(u64::MAX, |h| h.value);
    let out =
        dwg::dxf_convert::convert_dwg_to_dxf(&file, target).map_err(|e| format!("convert: {e}"))?;
    let slice = tables_only(&out).ok_or("the output has no TABLES section")?;
    let ours = DxfDocument::parse(slice.as_bytes()).map_err(|e| format!("read back: {e}"))?;
    let digits = theirs.real_precision();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    let mut o = Outcome {
        tallies: BTreeMap::new(),
        reasons: BTreeMap::new(),
        failures: Vec::new(),
    };
    for table in TABLES {
        let t = o.tallies.entry(table).or_default();
        let ours_recs: Vec<&DxfRecord> = ours.records.iter().filter(|r| r.kind == table).collect();
        let their_recs: Vec<&DxfRecord> =
            theirs.records.iter().filter(|r| r.kind == table).collect();
        let their_table = theirs
            .records
            .iter()
            .find(|r| r.kind == "TABLE" && r.first(2).and_then(DxfValue::as_str) == Some(table));
        let our_table = ours
            .records
            .iter()
            .find(|r| r.kind == "TABLE" && r.first(2).and_then(DxfValue::as_str) == Some(table));
        if let (Some(a), Some(b)) = (our_table, their_table) {
            if a.handle != b.handle {
                o.failures.push(format!(
                    "{name} TABLE {table}: handle {:?} but AutoCAD wrote {:?}",
                    a.handle, b.handle
                ));
            }
        } else if our_table.is_none() && !ours_recs.is_empty() {
            o.failures
                .push(format!("{name} {table}: records without a TABLE block"));
        }
        let by_handle: BTreeMap<u64, &DxfRecord> = their_recs
            .iter()
            .filter_map(|r| r.handle.map(|h| (h, *r)))
            .collect();
        let ours_handles: BTreeSet<u64> = ours_recs.iter().filter_map(|r| r.handle).collect();
        let mut paired: BTreeSet<u64> = BTreeSet::new();
        for r in &ours_recs {
            t.ours += 1;
            let Some(h) = r.handle else { continue };
            let record_name = r.first(2).and_then(DxfValue::as_str).unwrap_or("");
            let (want, same_handle) = match by_handle.get(&h) {
                Some(w) => (*w, true),
                None => {
                    // The export may register a record again under another handle.
                    let other = their_recs.iter().find(|w| {
                        w.handle.is_some_and(|wh| !ours_handles.contains(&wh))
                            && w.first(2)
                                .and_then(DxfValue::as_str)
                                .is_some_and(|x| x.eq_ignore_ascii_case(record_name))
                    });
                    match other {
                        Some(w) => {
                            *o.reasons
                                .entry(format!(
                                    "{table} record the export re-registered under another handle"
                                ))
                                .or_default() += 1;
                            (*w, false)
                        }
                        None => {
                            o.failures.push(format!(
                                "{name} {table} {h:X} {record_name:?}: written but AutoCAD's export has no such record"
                            ));
                            continue;
                        }
                    }
                }
            };
            paired.insert(want.handle.unwrap_or(h));
            t.paired += 1;
            if same_handle && r.owner != want.owner {
                o.failures.push(format!(
                    "{name} {table} {h:X}: owner {:?} but AutoCAD wrote {:?}",
                    r.owner, want.owner
                ));
            }
            let codes: BTreeSet<u16> = r.groups.iter().map(|g| g.code).collect();
            for code in &codes {
                if [100, 330, 5].contains(code) {
                    continue;
                }
                t.groups += 1;
                let (a, b) = (values(r, *code), values(want, *code));
                if b.is_empty() {
                    t.extra += 1;
                    o.failures.push(format!(
                        "{name} {table} {h:X}: group {code} written but absent from the export"
                    ));
                } else if a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| same(x, y, digits)) {
                    t.agree += 1;
                } else {
                    t.disagree += 1;
                    o.failures.push(format!(
                        "{name} {table} {h:X}: group {code} is {} but AutoCAD wrote {}",
                        a.iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                        b.iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    ));
                }
            }
            let their_codes: BTreeSet<u16> = want.groups.iter().map(|g| g.code).collect();
            for code in their_codes.difference(&codes) {
                if [100, 330, 5].contains(code) {
                    continue;
                }
                t.omitted_groups += 1;
                match group_reason(table, *code, want) {
                    Some(why) => *o.reasons.entry(format!("{table} group {code} ({why})")).or_default() += 1,
                    None => o.failures.push(format!(
                        "{name} {table} {h:X}: AutoCAD writes group {code} and the record omits it with no stated reason"
                    )),
                }
            }
        }
        for r in &their_recs {
            let Some(h) = r.handle else { continue };
            if paired.contains(&h) {
                continue;
            }
            if h >= seed {
                t.export_made += 1;
                continue;
            }
            t.omitted_records += 1;
            let complex = table == "LTYPE" && r.all(74).any(|v| v.as_i64().is_some_and(|f| f != 0));
            if complex {
                *o.reasons
                    .entry("LTYPE record (a shape or text dash is not written)".into())
                    .or_default() += 1;
            } else {
                o.failures.push(format!(
                    "{name} {table} {h:X}: in the export, not written, no stated reason"
                ));
            }
        }
    }
    println!(
        "{name:28} {}  {}",
        file.version(),
        TABLES
            .iter()
            .map(|t| {
                let x = &o.tallies[t];
                format!("{t} {}/{} ", x.paired, x.ours)
            })
            .collect::<String>()
    );
    Ok(Some(o))
}

fn main() -> ExitCode {
    let mut paths: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        paths.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples"));
    }
    let (mut dwgs, mut dxfs) = (Vec::new(), Vec::new());
    for p in &paths {
        walk(p, &mut dwgs, &mut dxfs);
    }
    dwgs.sort();
    dwgs.dedup();
    let mut twins = BTreeMap::new();
    for d in dxfs {
        twins.entry(stem_key(&d)).or_insert(d);
    }
    if dwgs.is_empty() {
        println!("NOT MEASURED: no .dwg under {paths:?}");
        return ExitCode::SUCCESS;
    }
    let mut by: BTreeMap<(String, &'static str), Tally> = BTreeMap::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    let (mut compared, mut skipped) = (0usize, 0usize);
    for path in &dwgs {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let release = std::fs::read(path)
            .ok()
            .and_then(|b| DwgFile::from_bytes(b).ok())
            .map(|f| f.version().to_string())
            .unwrap_or_else(|| "?".into());
        match probe_file(path, &twins) {
            Ok(Some(o)) => {
                compared += 1;
                failures.extend(o.failures);
                for (k, n) in o.reasons {
                    *reasons.entry(k).or_default() += n;
                }
                for (table, t) in o.tallies {
                    let a = by.entry((release.clone(), table)).or_default();
                    a.ours += t.ours;
                    a.paired += t.paired;
                    a.groups += t.groups;
                    a.agree += t.agree;
                    a.disagree += t.disagree;
                    a.extra += t.extra;
                    a.omitted_groups += t.omitted_groups;
                    a.omitted_records += t.omitted_records;
                    a.export_made += t.export_made;
                }
            }
            Ok(None) => skipped += 1,
            Err(e) => {
                println!("{name:28} FAILED: {e}");
                failures.push(format!("{name}: {e}"));
            }
        }
    }
    println!();
    println!(
        "{:14} {:6} {:>8} {:>7} {:>7} {:>6} {:>9} {:>6} {:>7} {:>8} {:>9}",
        "release",
        "table",
        "written",
        "paired",
        "groups",
        "agree",
        "disagree",
        "extra",
        "om.grp",
        "om.rec",
        "export-made"
    );
    for ((release, table), t) in &by {
        println!(
            "{release:14} {table:6} {:>8} {:>7} {:>7} {:>6} {:>9} {:>6} {:>7} {:>8} {:>9}",
            t.ours,
            t.paired,
            t.groups,
            t.agree,
            t.disagree,
            t.extra,
            t.omitted_groups,
            t.omitted_records,
            t.export_made
        );
    }
    println!("{skipped} drawings skipped (no twin or no AutoCAD export of the release)");
    for (k, n) in &reasons {
        println!("omitted: {k} x{n}");
    }
    for f in &failures {
        println!("FAILED {f}");
    }
    if compared == 0 && failures.is_empty() {
        println!(
            "NOT MEASURED: no drawing reached the converter with an AutoCAD export to compare"
        );
    }
    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
