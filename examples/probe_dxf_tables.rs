//! `dwg-to-dxf`'s TABLES section against AutoCAD's own DXF export (#126).
//!
//! # What this proves
//!
//! For each DWG that has an AutoCAD DXF export of the same drawing, the probe
//! converts it with [`dwg::dxf_convert::convert_dwg_to_dxf`] to the DWG's own
//! release, reads the output back with [`dwg::dxf_reader`] (the converter's
//! output as far as the end of its TABLES section) and pairs every record it
//! wrote with the export's record of the same handle, per table (VPORT, LTYPE,
//! LAYER, STYLE, VIEW, UCS, APPID, DIMSTYLE, BLOCK_RECORD). A paired record must agree group by
//! group:
//!
//! - every group written has the export's code and equal values (text and
//!   integers exactly, reals to the precision AutoCAD printed); a group the
//!   export does not have is **extra**;
//! - the `102 {ACAD_REACTORS`, `{ACAD_XDICTIONARY` and `{BLKREFS` brackets are
//!   compared as wholes, apart from a record the export registered again under
//!   another handle (its handles are new);
//! - a group of the export that the record lacks is **omitted**, and needs a
//!   stated reason: extended data (1000 to 1071); a true colour's 420, its
//!   layer's 62 and a viewport's 63, which the DWG stores no index for; a
//!   handle group naming an object the OBJECTS section does not write (the layout, the
//!   plot style, the material, the visual style) and the bracket of entities
//!   that react to a record or insert a block, when one of them is not written
//!   (a DIMENSION, say);
//! - a value that differs is a **relation** only when the way the export was
//!   made explains it: the active viewport's view when the export is a later
//!   save (its `$TDUUPDATE` is later than the DWG's), and a dynamic block
//!   definition of an R2000 to R2007 file that AutoCAD loads as an anonymous
//!   `*U` block;
//! - a record of the export with no record written is **omitted**: one created
//!   by the export itself (a handle at or above the DWG's `$HANDSEED`: it is not
//!   in the DWG). Any other is a failure,
//!   as is a record written that the export does not have, or a table header
//!   whose handle or owner differs.
//!
//! Records are compared in the order AutoCAD writes them, and the groups both write
//! in the order AutoCAD writes them.
//!
//! The export's `70` count on a table header is not compared: AutoCAD's count
//! does not always equal its record count. No corpus drawing holds a UCS record,
//! so the UCS table is checked only as AutoCAD's empty one.
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

const TABLES: [&str; 9] = [
    "VPORT",
    "LTYPE",
    "LAYER",
    "STYLE",
    "VIEW",
    "UCS",
    "APPID",
    "DIMSTYLE",
    "BLOCK_RECORD",
];

#[derive(Default, Clone)]
struct Tally {
    ours: usize,
    paired: usize,
    groups: usize,
    agree: usize,
    disagree: usize,
    relation: usize,
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
    let start = out.find("\nTABLES\n")?;
    let at = start + out[start..].find("ENDSEC")? + "ENDSEC".len();
    let end = out[at..].find('\n')? + at + 1;
    Some(format!("{}0\nEOF\n", &out[..end]))
}

fn same(a: &DxfValue, b: &DxfValue, digits: u8) -> bool {
    match (a, b) {
        (DxfValue::Real { value: x, .. }, DxfValue::Real { value: y, .. }) => close(*x, *y, digits),
        (x, y) => x == y,
    }
}

/// The values of `code` outside the `102` brackets.
fn values(r: &DxfRecord, code: u16) -> Vec<&DxfValue> {
    r.top_level()
        .filter(|g: &&DxfGroup| g.code == code)
        .map(|g| &g.value)
        .collect()
}

/// The groups inside each `102 {NAME` bracket of `r`, by bracket name.
fn brackets(r: &DxfRecord) -> BTreeMap<String, Vec<(u16, DxfValue)>> {
    let mut out: BTreeMap<String, Vec<(u16, DxfValue)>> = BTreeMap::new();
    let mut open: Option<String> = None;
    for g in &r.groups {
        match (&open, g.code, g.value.as_str()) {
            (None, 102, Some(t)) if t.starts_with('{') => {
                open = Some(t[1..].to_string());
                out.entry(t[1..].to_string()).or_default();
            }
            (Some(_), 102, Some("}")) => open = None,
            (Some(name), c, _) => out
                .entry(name.clone())
                .or_default()
                .push((c, g.value.clone())),
            _ => {}
        }
    }
    out
}

/// Why the converter may omit group `code` of a `table` record the export writes.
fn group_reason(table: &str, code: u16, theirs: &DxfRecord) -> Option<&'static str> {
    match (table, code) {
        (_, 1000..=1071) => Some("extended data is not decoded for table records"),
        ("BLOCK_RECORD", 340) => Some("the layout object is not written to OBJECTS"),
        ("DIMSTYLE", 176..=178) => Some("a true colour has no index in the DWG"),
        ("VPORT" | "VIEW", 348) => Some("the visual style object is written as an untyped record"),
        ("VPORT", 332 | 361 | 345 | 346) => Some("the object it names is not written"),
        ("VPORT", 63) if !values(theirs, 421).is_empty() => {
            Some("a true colour has no index in the DWG")
        }
        ("LAYER", 430) => Some("the layer's R2007 colour name is not decoded"),
        ("LAYER", 390) => Some("the plot-style object is not written to OBJECTS"),
        ("LAYER", 347 | 348) => {
            Some("the material and visual style objects are written as untyped records")
        }
        ("LAYER", 420) => Some("a true colour: the decoder surfaces no RGB"),
        ("LAYER", 62) if !values(theirs, 420).is_empty() => {
            Some("a true colour has no index in the DWG")
        }
        _ => None,
    }
}

/// A difference from the export that is explained by how the export was made,
/// not by the converter.
fn relation(
    table: &str,
    record: &str,
    code: u16,
    ours: &[&DxfValue],
    theirs: &[&DxfValue],
    later_save: bool,
    release: Version,
) -> Option<&'static str> {
    const VIEW_STATE: [u16; 18] = [
        10, 20, 11, 21, 12, 22, 16, 26, 36, 17, 27, 37, 40, 41, 42, 43, 44, 51,
    ];
    if table == "VPORT" && record == "*Active" && later_save && VIEW_STATE.contains(&code) {
        return Some("the active view of a drawing whose export is a later save");
    }
    if table == "DIMSTYLE"
        && code == 70
        && theirs.len() == ours.len() + 1
        && theirs[..ours.len()] == *ours
    {
        return Some("the fill colour is a true colour, which has no index in the DWG");
    }
    let anon =
        |v: &[&DxfValue], f: fn(&str) -> bool| v.first().and_then(|x| x.as_str()).is_some_and(f);
    let is_u_block = |n: &str| {
        n.strip_prefix("*U")
            .is_some_and(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()))
    };
    if table == "BLOCK_RECORD"
        && code == 2
        && matches!(release, Version::R2000 | Version::R2004 | Version::R2007)
        && anon(theirs, is_u_block)
        && !anon(ours, |n| n.starts_with('*'))
    {
        return Some("AutoCAD loads this release's dynamic block as an anonymous *U block");
    }
    None
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
    let timer = |d: &DxfDocument| {
        d.header_var("$TDUUPDATE")
            .and_then(|g| g.first())
            .and_then(|g| g.value.as_f64())
            .unwrap_or(f64::NAN)
    };
    let later_save = timer(&theirs) - timer(&ours) > 1e-6;
    let release = file.version();
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
            if table == "DIMSTYLE" {
                for code in [71, 340] {
                    let (x, y) = (
                        a.all(code).collect::<Vec<_>>(),
                        b.all(code).collect::<Vec<_>>(),
                    );
                    if x != y {
                        o.failures.push(format!(
                            "{name} TABLE {table}: group {code} is {x:?} but AutoCAD wrote {y:?}"
                        ));
                    }
                }
            }
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
        let mut last_index = 0usize;
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
            let at = their_recs
                .iter()
                .position(|w| std::ptr::eq(*w, want))
                .unwrap_or(0);
            if at < last_index {
                o.failures.push(format!(
                    "{name} {table} {h:X}: written before a record AutoCAD's {table} table has ahead of it"
                ));
            }
            last_index = at;
            if same_handle && r.owner != want.owner {
                o.failures.push(format!(
                    "{name} {table} {h:X}: owner {:?} but AutoCAD wrote {:?}",
                    r.owner, want.owner
                ));
            }
            let codes: BTreeSet<u16> = r.top_level().map(|g| g.code).collect();
            let (our_brackets, their_brackets) = (brackets(r), brackets(want));
            for (bname, theirs_in) in &their_brackets {
                match our_brackets.get(bname) {
                    Some(ours_in) if ours_in == theirs_in => {
                        t.groups += 1;
                        t.agree += 1;
                    }
                    Some(_) if !same_handle => {
                        *o.reasons
                            .entry(format!(
                                "{table} bracket {bname} of a record the export re-registered (its handles are new)"
                            ))
                            .or_default() += 1;
                    }
                    Some(_) => {
                        t.groups += 1;
                        t.disagree += 1;
                        o.failures.push(format!(
                            "{name} {table} {h:X}: bracket {bname} differs from AutoCAD's"
                        ));
                    }
                    None => {
                        t.omitted_groups += 1;
                        let why = match bname.as_str() {
                            "ACAD_REACTORS" => "an entity that reacts to the record is not written",
                            "ACAD_XDICTIONARY" => {
                                "the extension dictionary object is not written to OBJECTS"
                            }
                            "BLKREFS" => "an INSERT of the block is not written",
                            _ => "",
                        };
                        if why.is_empty() {
                            o.failures.push(format!(
                                "{name} {table} {h:X}: AutoCAD writes {{{bname}}} and the record omits it with no stated reason"
                            ));
                        } else {
                            *o.reasons
                                .entry(format!("{table} bracket {bname} ({why})"))
                                .or_default() += 1;
                        }
                    }
                }
            }
            for bname in our_brackets.keys() {
                if their_brackets.contains_key(bname) {
                    continue;
                }
                if table == "LAYER" && bname == "ACAD_XDICTIONARY" && release == Version::R2000 {
                    *o.reasons
                        .entry("LAYER extension dictionary of an R2000 file (it holds a true or book colour that AutoCAD's export drops)".into())
                        .or_default() += 1;
                    continue;
                }
                t.extra += 1;
                o.failures.push(format!(
                    "{name} {table} {h:X}: bracket {bname} written but absent from the export"
                ));
            }
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
                } else if let Some(why) =
                    relation(table, record_name, *code, &a, &b, later_save, release)
                {
                    t.relation += 1;
                    *o.reasons
                        .entry(format!("relation: {table} group {code} ({why})"))
                        .or_default() += 1;
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
            let their_codes: BTreeSet<u16> = want.top_level().map(|g| g.code).collect();
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
            // The groups both write come in the same order.
            let count = |x: &DxfRecord, c: u16| x.top_level().filter(|g| g.code == c).count();
            let shared: BTreeSet<u16> = codes
                .intersection(&their_codes)
                .copied()
                .filter(|c| count(r, *c) == count(want, *c))
                .collect();
            let in_order = |x: &DxfRecord| -> Vec<u16> {
                x.top_level()
                    .map(|g| g.code)
                    .filter(|c| shared.contains(c) && ![100, 330, 5, 105].contains(c))
                    .collect()
            };
            if in_order(r) != in_order(want) {
                o.failures.push(format!(
                    "{name} {table} {h:X}: groups {:?} but AutoCAD wrote them {:?}",
                    in_order(r),
                    in_order(want)
                ));
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
            o.failures.push(format!(
                "{name} {table} {h:X}: in the export, not written, no stated reason"
            ));
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
                    a.relation += t.relation;
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
        "{:14} {:12} {:>8} {:>7} {:>7} {:>6} {:>8} {:>9} {:>6} {:>7} {:>8} {:>9}",
        "release",
        "table",
        "written",
        "paired",
        "groups",
        "agree",
        "relation",
        "disagree",
        "extra",
        "om.grp",
        "om.rec",
        "export-made"
    );
    for ((release, table), t) in &by {
        println!(
            "{release:14} {table:12} {:>8} {:>7} {:>7} {:>6} {:>8} {:>9} {:>6} {:>7} {:>8} {:>9}",
            t.ours,
            t.paired,
            t.groups,
            t.agree,
            t.relation,
            t.disagree,
            t.extra,
            t.omitted_groups,
            t.omitted_records,
            t.export_made
        );
    }
    println!("{skipped} drawings skipped (no twin or no AutoCAD export of the release)");
    for (k, n) in &reasons {
        if k.starts_with("relation: ") {
            println!("{k} x{n}");
        } else {
            println!("omitted: {k} x{n}");
        }
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
