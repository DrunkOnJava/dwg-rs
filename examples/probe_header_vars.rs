//! Every header variable of every drawing, against AutoCAD's own DXF export
//! (ODA spec v5.4.1 §9, #126).
//!
//! # What this proves
//!
//! For each DWG given, the probe decodes `AcDb:Header` through
//! [`dwg::header_vars::HeaderVars::decode`] and requires, with no
//! exceptions:
//!
//! 1. **the frame**: both §9 sentinels, the `RL` size against the section's
//!    length, and the CRC (seed `0xC0C1`, from the byte after the sentinel);
//! 2. **the boundary**: every slot of the release's rows in
//!    [`dwg::header_table::TABLE`] reads, the data stream ends where the
//!    string stream starts, the strings end where the string stream does
//!    (R2007 and later), and the last stream ends inside the last byte of
//!    the data. The padding is counted, not required: §9 calls it random and
//!    it is all ones on every AutoCAD 2016 to 2025 file of the corpus.
//!
//! Where the drawing has a DXF twin (AutoCAD's export of the same drawing,
//! `tools/corpus/pins.tsv`), it then compares every variable it decoded
//! with the DXF `HEADER` section's `$NAME`:
//!
//! - numbers, text and flags by value, reals to the precision the file was
//!   printed at;
//! - a handle whose DXF form is a name (`$CLAYER`, `$DIMBLK`, ...) by finding
//!   the DXF record with that handle and comparing its name (group 2);
//! - a handle whose DXF form is a handle (`$HANDSEED`, `$CMATERIAL`, ...)
//!   directly;
//! - a colour by its AutoCAD index, a date as one Julian date; a true colour
//!   (from R2004, a top byte of `0xC2`) has no index and DXF's `HEADER`
//!   writes no true-colour group (420) for it, only an approximate index, so
//!   its RGB cannot be checked there and only the kind of colour is;
//! - the control objects, named blocks, linetypes and dictionaries (which
//!   DXF does not write in `HEADER`) against the handles the DXF's `TABLE`
//!   records, named records and root dictionary give them.
//!
//! The twin is saved after the DWG, so three variables are compared by
//! relation instead of by value: `$TDUUPDATE`, `$TDINDWG` and `$TDUSRTIMER`
//! must each be later in the DXF by the same time (the seconds between the
//! two saves), and `$HANDSEED` must not be lower.
//!
//! Every variable falls in exactly one bucket, and the per-release table
//! prints all of them: **agree**, **disagree** (a finding: the probe exits
//! 1), **relation** (as above), **DWG-only** (a slot DXF does not write:
//! control objects, `$PELLIPSE`, unknown slots) and **DXF-only** (a `HEADER`
//! variable the section does not hold, with where it lives).
//!
//! A release with no DXF twin in the corpus (AC1014) is held to the frame and
//! the boundary only, and says so.
//!
//! # How to verify
//!
//! ```text
//! cargo run --release --features dxf-reader --example probe_header_vars -- <file-or-dir> ...
//! ```
//!
//! Directories are searched recursively for `.dwg` and `.dxf` files; a DWG's
//! twin is the DXF with the same stem, or the stem plus `_ascii`, anywhere
//! under the paths given or under `--dxf-dir`. With no path it reads
//! `../../samples` (the local corpus, see CLAUDE.md). With no DWG at all it
//! prints `NOT MEASURED` and exits 0. `--verbose` prints every unknown slot's
//! values and every finding in full.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dwg::dxf_reader::{DxfDocument, DxfGroup, DxfValue};
use dwg::header_vars::{Entry, HeaderVars, Value};
use dwg::{DwgFile, ParseLimits, Version};

#[derive(Default)]
struct Tally {
    files: usize,
    closed: usize,
    pad_ones: usize,
    twins: usize,
    vars: usize,
    unknown: usize,
    agree: usize,
    relation: usize,
    disagree: usize,
    dwg_only: usize,
    dxf_only: usize,
}

struct Finding {
    file: String,
    var: String,
    dwg: String,
    dxf: String,
}

struct Report {
    by_release: BTreeMap<String, Tally>,
    findings: Vec<Finding>,
    failures: Vec<String>,
    dwg_only: BTreeMap<String, BTreeSet<String>>,
    dxf_only: BTreeMap<String, BTreeSet<String>>,
    unknowns: BTreeMap<(String, String), BTreeSet<String>>,
    relations: BTreeMap<String, BTreeSet<String>>,
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

fn show(v: &Value) -> String {
    match v {
        Value::Handle(h) => format!("handle {:#X}", h.value),
        Value::Color(c) => format!("color {:?}", c.aci()),
        Value::Date { day, ms } => format!("date {day} + {ms} ms"),
        other => format!("{other:?}"),
    }
}

fn show_groups(g: &[DxfGroup]) -> String {
    g.iter()
        .map(|g| format!("{}:{}", g.code, g.value))
        .collect::<Vec<_>>()
        .join(" ")
}

fn close(a: f64, b: f64, digits: u8) -> bool {
    if a == b {
        return true;
    }
    let rel = 10f64.powi(-(i32::from(digits.max(2)) - 1)) * 2.0;
    (a - b).abs() <= a.abs().max(b.abs()) * rel
}

fn name_of(doc: &DxfDocument, handle: u64) -> Option<String> {
    doc.record_by_handle(handle)?
        .first(2)?
        .as_str()
        .map(str::to_owned)
}

fn table_handle(doc: &DxfDocument, table: &str) -> Option<u64> {
    doc.records
        .iter()
        .find(|r| r.kind == "TABLE" && r.first(2).and_then(DxfValue::as_str) == Some(table))
        .and_then(|r| r.handle)
}

fn named_record(doc: &DxfDocument, kind: &str, name: &str) -> Option<u64> {
    doc.records
        .iter()
        .find(|r| {
            r.kind == kind
                && r.first(2)
                    .and_then(DxfValue::as_str)
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .and_then(|r| r.handle)
}

fn root_dictionary(doc: &DxfDocument) -> Option<(u64, Vec<(String, u64)>)> {
    let root = doc
        .records
        .iter()
        .find(|r| r.kind == "DICTIONARY" && r.owner == Some(0))
        .or_else(|| doc.records.iter().find(|r| r.kind == "DICTIONARY"))?;
    let mut entries = Vec::new();
    let mut pending: Option<String> = None;
    for g in root.top_level() {
        match (g.code, &g.value) {
            (3, DxfValue::Text(t)) => pending = Some(t.clone()),
            (350 | 360, DxfValue::Handle(h)) => {
                if let Some(name) = pending.take() {
                    entries.push((name, *h));
                }
            }
            _ => {}
        }
    }
    Some((root.handle?, entries))
}

/// The handle the DXF gives a header slot that DXF's `HEADER` does not
/// write, with a label for the record that gave it.
fn dxf_handle_of_slot(doc: &DxfDocument, name: &str) -> Option<u64> {
    let table = |t| table_handle(doc, t);
    let dict = |key: &str| {
        root_dictionary(doc)
            .and_then(|(_, e)| e.into_iter().find(|(n, _)| n == key).map(|(_, h)| h))
    };
    match name {
        "BLOCK_CONTROL_OBJECT" => table("BLOCK_RECORD"),
        "LAYER_CONTROL_OBJECT" => table("LAYER"),
        "STYLE_CONTROL_OBJECT" => table("STYLE"),
        "LINETYPE_CONTROL_OBJECT" => table("LTYPE"),
        "VIEW_CONTROL_OBJECT" => table("VIEW"),
        "UCS_CONTROL_OBJECT" => table("UCS"),
        "VPORT_CONTROL_OBJECT" => table("VPORT"),
        "APPID_CONTROL_OBJECT" => table("APPID"),
        "DIMSTYLE_CONTROL_OBJECT" => table("DIMSTYLE"),
        "BLOCK_RECORD_PAPER_SPACE" => named_record(doc, "BLOCK_RECORD", "*Paper_Space"),
        "BLOCK_RECORD_MODEL_SPACE" => named_record(doc, "BLOCK_RECORD", "*Model_Space"),
        "LTYPE_BYLAYER" => named_record(doc, "LTYPE", "ByLayer"),
        "LTYPE_BYBLOCK" => named_record(doc, "LTYPE", "ByBlock"),
        "LTYPE_CONTINUOUS" => named_record(doc, "LTYPE", "Continuous"),
        "DICTIONARY_NAMED_OBJECTS" => root_dictionary(doc).map(|(h, _)| h),
        "DICTIONARY_ACAD_GROUP" => dict("ACAD_GROUP"),
        "DICTIONARY_ACAD_MLINESTYLE" => dict("ACAD_MLINESTYLE"),
        "DICTIONARY_LAYOUTS" => dict("ACAD_LAYOUT"),
        "DICTIONARY_PLOTSETTINGS" => dict("ACAD_PLOTSETTINGS"),
        "DICTIONARY_PLOTSTYLES" => dict("ACAD_PLOTSTYLENAME"),
        "DICTIONARY_MATERIALS" => dict("ACAD_MATERIAL"),
        "DICTIONARY_COLORS" => dict("ACAD_COLOR"),
        "DICTIONARY_VISUALSTYLE" => dict("ACAD_VISUALSTYLE"),
        _ => None,
    }
}

enum Verdict {
    Agree,
    Relation(&'static str),
    Disagree(String),
}

const ARROWHEADS: [&str; 4] = ["$DIMBLK", "$DIMBLK1", "$DIMBLK2", "$DIMLDRBLK"];

fn looks_like_guid(t: &str) -> bool {
    t.len() == 38
        && t.starts_with('{')
        && t.ends_with('}')
        && t.bytes().enumerate().all(|(i, b)| match i {
            0 | 37 => true,
            9 | 14 | 19 | 24 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// What an AC1015 export of an AC1014 drawing cannot say about the AC1014 file:
/// a different release and a different save.
const NEIGHBOUR_SKIP: [&str; 5] = [
    "$TDUPDATE",
    "$TDINDWG",
    "$TDUSRTIMER",
    "$HANDSEED",
    "$MAXACTVP",
];

fn compare(
    entry: &Entry,
    groups: &[DxfGroup],
    doc: &DxfDocument,
    version: Version,
    neighbour: bool,
) -> Verdict {
    let digits = doc.real_precision();
    let name = entry.var.name;
    if neighbour && NEIGHBOUR_SKIP.contains(&name) {
        return Verdict::Relation(
            "an AC1015 export of the same drawing: another release, another save",
        );
    }
    let first = groups.first().map(|g| &g.value);
    let real = |i: usize| groups.get(i).and_then(|g| g.value.as_f64());
    let agree = match &entry.value {
        Value::Bool(_) | Value::Byte(_) | Value::Short(_) | Value::Long(_) | Value::LongLong(_) => {
            let want = first.and_then(DxfValue::as_i64);
            if name == "$XCLIPFRAME" && !version.is_r2010_plus() {
                // Before R2010 the variable is 0 or 1 only (§9); the export maps 2 to false.
                if want == Some(i64::from(entry.value.as_i64() == Some(1))) {
                    return Verdict::Relation("XCLIPFRAME 2 is written false before R2010");
                }
                false
            } else {
                want == entry.value.as_i64()
            }
        }
        Value::Double(v) => real(0).is_some_and(|d| close(*v, d, digits)),
        Value::Point3(p) => (0..3).all(|i| real(i).is_some_and(|d| close(p[i], d, digits))),
        Value::Point2(p) => (0..2).all(|i| real(i).is_some_and(|d| close(p[i], d, digits))),
        Value::Text(t) => {
            let want = first.and_then(DxfValue::as_str);
            if name == "$VERSIONGUID" && want != Some(t.as_str()) {
                if looks_like_guid(t) && want.is_some_and(looks_like_guid) {
                    return Verdict::Relation("VERSIONGUID changes on every save");
                }
                false
            } else if version == Version::R14
                && want != Some(t.as_str())
                && want.is_some_and(|w| w.eq_ignore_ascii_case(t))
            {
                return Verdict::Relation("R14 writes symbol names in capitals");
            } else {
                want == Some(t.as_str())
            }
        }
        Value::Color(c) => match c.aci() {
            Some(i) => Some(i64::from(i)) == first.and_then(DxfValue::as_i64),
            None => match first.and_then(DxfValue::as_i64) {
                // A true colour has no index; the export writes an approximate one,
                // so only the kind of colour is checked, not its RGB.
                Some(i) if (1..=255).contains(&i) => {
                    return Verdict::Relation("a true colour is written as an approximate index");
                }
                _ => false,
            },
        },
        Value::Date { .. } => match (entry.value.as_julian(), real(0)) {
            (Some(a), Some(b)) => (a - b).abs() <= 1e-9,
            _ => false,
        },
        Value::Handle(h) => match first {
            Some(DxfValue::Handle(d)) if *d == h.value => true,
            Some(DxfValue::Handle(d)) if h.value == 0 => {
                // The export names a visual style where the file holds the null handle.
                if doc
                    .record_by_handle(*d)
                    .is_some_and(|r| r.kind == "VISUALSTYLE")
                {
                    return Verdict::Relation("the export names the default visual style");
                }
                false
            }
            Some(DxfValue::Text(want)) => {
                if h.value == 0 {
                    want.is_empty()
                } else {
                    match name_of(doc, h.value) {
                        Some(got) if got == *want => true,
                        Some(got) if got.eq_ignore_ascii_case(want) => {
                            return Verdict::Relation(
                                "symbol names are case-insensitive (the MLINESTYLE record holds STANDARD)",
                            );
                        }
                        Some(got)
                            if ARROWHEADS.contains(&name)
                                && got.eq_ignore_ascii_case(&format!("_{want}")) =>
                        {
                            return Verdict::Relation(
                                "an arrowhead block is named with a leading underscore",
                            );
                        }
                        _ => false,
                    }
                }
            }
            _ => false,
        },
    };
    if agree {
        Verdict::Agree
    } else {
        Verdict::Disagree(show_groups(groups))
    }
}

/// The twin is saved after the DWG: these never go back, and the two timers
/// that count time in the drawing advance by the same amount.
const VOLATILE: [&str; 4] = ["$TDUUPDATE", "$TDINDWG", "$TDUSRTIMER", "$HANDSEED"];

/// Where a `HEADER` variable the section does not hold lives, as far as the
/// spec and the measurements say.
fn dxf_only_note(name: &str, neighbour: bool) -> &'static str {
    match name {
        "$ACADVER" => "the file's version magic",
        "$DWGCODEPAGE" => "the file header's code page (§3.2, §4.1)",
        "$MEASUREMENT" => "AcDb:Template (§22)",
        "$TDCREATE" | "$TDUPDATE" => {
            "not stored: the DWG holds the universal time, DXF adds the exporting machine's offset"
        }
        _ if neighbour => "a variable of a later release: the twin is an AC1015 export",
        _ => "not in AcDb:Header",
    }
}

fn volatile_relation(h: &HeaderVars, doc: &DxfDocument) -> Result<(), String> {
    let dxf_real = |name: &str| {
        doc.header_var(name)
            .and_then(|g| g.first())
            .and_then(|g| g.value.as_f64())
    };
    let dwg = |name: &str| h.get(name).and_then(Value::as_julian);
    let mut delta = BTreeMap::new();
    for name in ["$TDUUPDATE", "$TDINDWG", "$TDUSRTIMER"] {
        match (dwg(name), dxf_real(name)) {
            (Some(a), Some(b)) => {
                delta.insert(name, b - a);
            }
            _ => return Err(format!("{name} is missing on one side")),
        }
    }
    if let Some((name, d)) = delta.iter().find(|(_, d)| **d < -1e-9) {
        return Err(format!("{name} is earlier in the DXF by {d} days"));
    }
    let (a, b) = (delta["$TDINDWG"], delta["$TDUSRTIMER"]);
    if (a - b).abs() > 1e-8 {
        return Err(format!("TDINDWG advanced {a} days, TDUSRTIMER {b}"));
    }
    let seed = match (h.get("$HANDSEED"), doc.header_var("$HANDSEED")) {
        (Some(Value::Handle(a)), Some(g)) => g
            .first()
            .and_then(|g| g.value.as_handle())
            .map(|b| (a.value, b)),
        _ => None,
    };
    match seed {
        Some((a, b)) if b >= a => Ok(()),
        other => Err(format!("$HANDSEED DWG/DXF {other:?}")),
    }
}

fn probe_file(
    path: &Path,
    twins: &BTreeMap<String, PathBuf>,
    verbose: bool,
    report: &mut Report,
) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let file = DwgFile::from_bytes(bytes).map_err(|e| format!("open: {e}"))?;
    let version = file.version();
    let release = version.to_string();
    let tally = report.by_release.entry(release.clone()).or_default();
    tally.files += 1;
    let section = file
        .read_section("AcDb:Header")
        .ok_or("no AcDb:Header section")?
        .map_err(|e| format!("read AcDb:Header: {e}"))?;
    let h = HeaderVars::decode(&section, version, &ParseLimits::safe())
        .map_err(|e| format!("decode: {e}"))?;
    if !h.closed {
        return Err(format!("does not close: {:?}", h.diagnostics));
    }
    tally.closed += 1;
    tally.pad_ones += usize::from(h.pad_ones);
    tally.vars += h.entries.len();
    for e in h.unknowns() {
        tally.unknown += 1;
        report
            .unknowns
            .entry((release.clone(), e.var.name.to_string()))
            .or_default()
            .insert(show(&e.value));
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();
    let key = stem_key(path);
    let neighbour = if version == Version::R14 {
        // No AC1014 export exists; the same drawing's AC1015 export is the nearest.
        key.strip_suffix("_r14")
            .map(|base| format!("{base}_2000"))
            .or_else(|| {
                key.strip_suffix("ac1014")
                    .map(|base| format!("{base}ac1015"))
            })
    } else {
        None
    };
    let Some(twin) = twins
        .get(&key)
        .or_else(|| neighbour.as_ref().and_then(|k| twins.get(k)))
    else {
        println!(
            "{name:28} {release}  closed, {} slots, CRC ok, no DXF twin: frame and boundary only",
            h.entries.len()
        );
        return Ok(());
    };
    let dxf_bytes = std::fs::read(twin).map_err(|e| format!("read twin: {e}"))?;
    let doc =
        DxfDocument::parse(&dxf_bytes).map_err(|e| format!("twin {}: {e}", twin.display()))?;
    let tally = report.by_release.get_mut(&release).expect("inserted above");
    tally.twins += 1;
    let (mut agree, mut dis, mut rel, mut dwg_only) = (0usize, 0usize, 0usize, 0usize);
    let mut matched: BTreeSet<&str> = BTreeSet::new();
    let mut relations: BTreeSet<&'static str> = BTreeSet::new();
    for e in &h.entries {
        let var = e.var;
        if var.is_unknown() {
            dwg_only += 1;
            continue;
        }
        if h.get("$TDUUPDATE").is_some() && VOLATILE.contains(&var.name) {
            matched.insert(var.name);
            continue;
        }
        if !var.is_autocad_variable() {
            match (dxf_handle_of_slot(&doc, var.name), &e.value) {
                (Some(want), Value::Handle(got)) if want == got.value => agree += 1,
                (Some(want), Value::Handle(_)) => {
                    dis += 1;
                    report.findings.push(Finding {
                        file: name.clone(),
                        var: var.name.into(),
                        dwg: show(&e.value),
                        dxf: format!("handle {want:#X}"),
                    });
                }
                _ => {
                    dwg_only += 1;
                    report
                        .dwg_only
                        .entry(release.clone())
                        .or_default()
                        .insert(var.name.into());
                }
            }
            continue;
        }
        match doc.header_var(var.name) {
            None => {
                dwg_only += 1;
                report
                    .dwg_only
                    .entry(release.clone())
                    .or_default()
                    .insert(var.name.into());
            }
            Some(groups) => {
                matched.insert(var.name);
                match compare(e, groups, &doc, version, neighbour.is_some()) {
                    Verdict::Agree => agree += 1,
                    Verdict::Relation(why) => {
                        rel += 1;
                        relations.insert(why);
                    }
                    Verdict::Disagree(dxf) => {
                        dis += 1;
                        report.findings.push(Finding {
                            file: name.clone(),
                            var: var.name.into(),
                            dwg: show(&e.value),
                            dxf,
                        });
                    }
                }
            }
        }
    }
    for why in &relations {
        report
            .relations
            .entry(release.clone())
            .or_default()
            .insert((*why).to_string());
    }
    match if h.get("$TDUUPDATE").is_some() {
        volatile_relation(&h, &doc)
    } else {
        Ok(())
    } {
        Ok(()) if h.get("$TDUUPDATE").is_none() => {}
        Ok(()) => rel += VOLATILE.len(),
        Err(why) => {
            dis += 1;
            report.findings.push(Finding {
                file: name.clone(),
                var: "$TDUUPDATE/$TDINDWG/$TDUSRTIMER/$HANDSEED".into(),
                dwg: String::new(),
                dxf: why,
            });
        }
    }
    let mut dxf_only = 0usize;
    for v in &doc.header {
        if v.name == "$ACADVER" {
            let ok = v.groups.first().and_then(|g| g.value.as_str())
                == std::str::from_utf8(&version.magic()).ok();
            if neighbour.is_some() {
                rel += 1;
            } else if ok {
                agree += 1;
            } else {
                dis += 1;
                report.findings.push(Finding {
                    file: name.clone(),
                    var: v.name.clone(),
                    dwg: version.to_string(),
                    dxf: show_groups(&v.groups),
                });
            }
            continue;
        }
        if !matched.contains(v.name.as_str()) {
            dxf_only += 1;
            report
                .dxf_only
                .entry(release.clone())
                .or_default()
                .insert(format!(
                    "{} ({})",
                    v.name,
                    dxf_only_note(&v.name, neighbour.is_some())
                ));
        }
    }
    let tally = report.by_release.get_mut(&release).expect("inserted above");
    tally.agree += agree;
    tally.disagree += dis;
    tally.relation += rel;
    tally.dwg_only += dwg_only;
    tally.dxf_only += dxf_only;
    println!(
        "{name:28} {release}  closed, {} slots, CRC ok, twin {}: agree {agree}, relation {rel}, disagree {dis}, DWG-only {dwg_only}, DXF-only {dxf_only}",
        h.entries.len(),
        twin.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
    );
    if verbose && dis > 0 {
        for f in report.findings.iter().filter(|f| f.file == name) {
            println!("    {}: DWG {}  DXF {}", f.var, f.dwg, f.dxf);
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut dxf_dirs: Vec<PathBuf> = Vec::new();
    let mut verbose = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--verbose" => verbose = true,
            "--dxf-dir" => match args.next() {
                Some(d) => dxf_dirs.push(PathBuf::from(d)),
                None => {
                    eprintln!("--dxf-dir needs a directory");
                    return ExitCode::FAILURE;
                }
            },
            _ => paths.push(PathBuf::from(a)),
        }
    }
    if paths.is_empty() {
        paths.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples"));
    }
    let (mut dwgs, mut dxfs) = (Vec::new(), Vec::new());
    for p in paths.iter().chain(dxf_dirs.iter()) {
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
    let mut report = Report {
        by_release: BTreeMap::new(),
        findings: Vec::new(),
        failures: Vec::new(),
        dwg_only: BTreeMap::new(),
        dxf_only: BTreeMap::new(),
        unknowns: BTreeMap::new(),
        relations: BTreeMap::new(),
    };
    for path in &dwgs {
        if let Err(e) = probe_file(path, &twins, verbose, &mut report) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            println!("{name:28} FAILED: {e}");
            report.failures.push(format!("{name}: {e}"));
        }
    }
    println!();
    println!(
        "{:14} {:>5} {:>6} {:>5} {:>6} {:>7} {:>6} {:>9} {:>9} {:>8} {:>8}",
        "release",
        "files",
        "closed",
        "twins",
        "slots",
        "unknown",
        "agree",
        "relation",
        "disagree",
        "DWG-only",
        "DXF-only"
    );
    let mut total = Tally::default();
    for (release, t) in &report.by_release {
        println!(
            "{release:14} {:>5} {:>6} {:>5} {:>6} {:>7} {:>6} {:>9} {:>9} {:>8} {:>8}",
            t.files,
            t.closed,
            t.twins,
            t.vars,
            t.unknown,
            t.agree,
            t.relation,
            t.disagree,
            t.dwg_only,
            t.dxf_only
        );
        total.files += t.files;
        total.closed += t.closed;
        total.pad_ones += t.pad_ones;
        total.twins += t.twins;
        total.vars += t.vars;
        total.unknown += t.unknown;
        total.agree += t.agree;
        total.relation += t.relation;
        total.disagree += t.disagree;
        total.dwg_only += t.dwg_only;
        total.dxf_only += t.dxf_only;
    }
    println!(
        "{:14} {:>5} {:>6} {:>5} {:>6} {:>7} {:>6} {:>9} {:>9} {:>8} {:>8}",
        "TOTAL",
        total.files,
        total.closed,
        total.twins,
        total.vars,
        total.unknown,
        total.agree,
        total.relation,
        total.disagree,
        total.dwg_only,
        total.dxf_only
    );
    println!(
        "\npadding after the last slot is all ones on {} of {} files",
        total.pad_ones, total.closed
    );
    for (release, names) in &report.dwg_only {
        println!(
            "\nDWG-only on {release} (no DXF header variable or record): {}",
            names.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    for (release, why) in &report.relations {
        println!(
            "\nrelations on {release}: {}",
            why.iter().cloned().collect::<Vec<_>>().join("; ")
        );
    }
    for (release, names) in &report.dxf_only {
        println!(
            "\nDXF-only on {release}: {}",
            names.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    if verbose {
        println!();
        for ((release, name), values) in &report.unknowns {
            let shown: Vec<&str> = values.iter().take(4).map(String::as_str).collect();
            println!(
                "unknown {release} {name}: {} distinct, e.g. {}",
                values.len(),
                shown.join(" | ")
            );
        }
    }
    let mut bad = false;
    for f in &report.findings {
        bad = true;
        println!(
            "DISAGREE {} {}: DWG {}  DXF {}",
            f.file, f.var, f.dwg, f.dxf
        );
    }
    for f in &report.failures {
        bad = true;
        println!("FAILED {f}");
    }
    if report.by_release.values().all(|t| t.closed == 0) && !bad {
        println!("NOT MEASURED: no drawing reached the header decoder");
    }
    if bad {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
