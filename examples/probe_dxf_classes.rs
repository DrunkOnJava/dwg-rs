//! `dwg-to-dxf`'s CLASSES section against AutoCAD's own DXF export (#126).
//!
//! # What this proves
//!
//! For each DWG that has an AutoCAD DXF export of the same drawing, the probe
//! converts it with [`dwg::dxf_convert::convert_dwg_to_dxf`] to the DWG's own
//! release, reads the output back with [`dwg::dxf_reader`] and pairs every
//! `CLASS` record of its CLASSES section with the export's record of the same
//! DXF name (group 1). A paired record must agree group by group (2, 3, 90, 280,
//! 281) and the paired records must come in the export's order. The instance count
//! (91) is a relation, not an agreement: the export counts the objects it holds, and
//! AutoCAD creates some when it loads the file and leaves others out. A class of
//! the export that the output lacks is counted, and one the output has that the
//! export lacks is a failure; the export may hold classes of its own (AutoCAD
//! registers them when it loads the file), which are listed.
//!
//! Any disagreement, extra, wrong order, section the output lacks or unreadable
//! output exits 1. AC1014 drawings are skipped. A drawing with no class table
//! (R12 and R14 exports) has no section to compare.
//!
//! # How to verify
//!
//! ```text
//! cargo run --release --features dxf-reader --example probe_dxf_classes -- <file-or-dir> ...
//! ```
//!
//! Directories are searched recursively for `.dwg` and `.dxf` files; a DWG's twin
//! is the DXF with the same stem, or the stem plus `_ascii`. With no DWG that has
//! a twin it prints `NOT MEASURED` and exits 0.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dwg::dxf::DxfVersion;
use dwg::dxf_reader::{DxfDocument, DxfRecord, DxfValue};
use dwg::{DwgFile, Version};

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

/// The CLASSES section of `out` alone, as a document of its own: the probe
/// compares that section, whatever state the others are in.
fn classes_only(out: &str) -> String {
    let lines: Vec<&str> = out.lines().collect();
    let mut kept: Vec<&str> = Vec::new();
    let mut inside = false;
    let mut i = 0;
    while i + 1 < lines.len() {
        let (code, value) = (lines[i].trim(), lines[i + 1].trim());
        if !inside
            && code == "0"
            && value == "SECTION"
            && lines.get(i + 3).map(|l| l.trim()) == Some("CLASSES")
        {
            inside = true;
        }
        if inside {
            kept.push(lines[i]);
            kept.push(lines[i + 1]);
            if code == "0" && value == "ENDSEC" {
                break;
            }
        }
        i += 2;
    }
    kept.extend(["  0", "EOF"]);
    kept.join("\n") + "\n"
}

fn classes(doc: &DxfDocument) -> Vec<&DxfRecord> {
    doc.records
        .iter()
        .filter(|r| doc.section_name(r) == "CLASSES")
        .collect()
}

fn name(r: &DxfRecord) -> String {
    r.first(1)
        .and_then(DxfValue::as_str)
        .unwrap_or("")
        .to_string()
}

#[derive(Default)]
struct Tally {
    written: usize,
    paired: usize,
    groups: usize,
    agree: usize,
    disagree: usize,
    relation: usize,
    export_only: usize,
}

fn probe_file(
    path: &Path,
    twins: &BTreeMap<String, PathBuf>,
    failures: &mut Vec<String>,
    tally: &mut BTreeMap<String, Tally>,
    export_only: &mut BTreeMap<String, usize>,
) -> Result<bool, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let file = DwgFile::from_bytes(bytes).map_err(|e| format!("open: {e}"))?;
    let Some(target) = dxf_version(file.version()) else {
        return Ok(false);
    };
    let Some(twin) = twins.get(&stem_key(path)) else {
        return Ok(false);
    };
    let theirs = DxfDocument::parse(&std::fs::read(twin).map_err(|e| format!("read twin: {e}"))?)
        .map_err(|e| format!("twin: {e}"))?;
    let out =
        dwg::dxf_convert::convert_dwg_to_dxf(&file, target).map_err(|e| format!("convert: {e}"))?;
    let ours =
        DxfDocument::parse(classes_only(&out).as_bytes()).map_err(|e| format!("read back: {e}"))?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    let (our_recs, their_recs) = (classes(&ours), classes(&theirs));
    let by_name: BTreeMap<String, usize> = their_recs
        .iter()
        .enumerate()
        .map(|(i, r)| (name(r), i))
        .collect();
    let t = tally.entry(file.version().to_string()).or_default();
    let mut paired: BTreeSet<usize> = BTreeSet::new();
    let mut last = 0usize;
    for r in &our_recs {
        t.written += 1;
        let n = name(r);
        let Some(&at) = by_name.get(&n) else {
            failures.push(format!(
                "{file_name} {n}: written but AutoCAD's CLASSES has no such class"
            ));
            continue;
        };
        paired.insert(at);
        t.paired += 1;
        if at < last {
            failures.push(format!(
                "{file_name} {n}: written before a class AutoCAD's CLASSES has ahead of it"
            ));
        }
        last = at;
        let want = their_recs[at];
        for code in [2u16, 3, 90, 91, 280, 281] {
            let (a, b) = (r.first(code), want.first(code));
            if a.is_none() && b.is_none() {
                continue;
            }
            t.groups += 1;
            if a == b {
                t.agree += 1;
            } else if code == 91 {
                // The export counts the objects it holds, among them those AutoCAD
                // creates or leaves out when it loads the file.
                t.relation += 1;
            } else {
                t.disagree += 1;
                failures.push(format!(
                    "{file_name} {n}: group {code} is {a:?} but AutoCAD wrote {b:?}"
                ));
            }
        }
    }
    for (i, r) in their_recs.iter().enumerate() {
        if !paired.contains(&i) {
            t.export_only += 1;
            *export_only.entry(name(r)).or_default() += 1;
        }
    }
    if our_recs.is_empty() && !their_recs.is_empty() {
        failures.push(format!(
            "{file_name}: the output has no CLASSES section but AutoCAD's holds {} classes",
            their_recs.len()
        ));
    }
    println!(
        "{file_name:28} {}  {} classes written, {} paired",
        file.version(),
        our_recs.len(),
        paired.len()
    );
    Ok(true)
}

fn main() -> ExitCode {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        eprintln!("usage: probe_dxf_classes <file-or-dir> ...");
        return ExitCode::from(2);
    }
    let mut dwgs = Vec::new();
    let mut dxfs = Vec::new();
    for p in &paths {
        walk(p, &mut dwgs, &mut dxfs);
    }
    let mut twins = BTreeMap::new();
    for d in dxfs {
        twins.entry(stem_key(&d)).or_insert(d);
    }
    if dwgs.is_empty() {
        println!("NOT MEASURED: no .dwg under {paths:?}");
        return ExitCode::SUCCESS;
    }
    let mut failures = Vec::new();
    let mut tally: BTreeMap<String, Tally> = BTreeMap::new();
    let mut export_only: BTreeMap<String, usize> = BTreeMap::new();
    let (mut measured, mut skipped, mut errors) = (0usize, 0usize, 0usize);
    for path in &dwgs {
        match probe_file(path, &twins, &mut failures, &mut tally, &mut export_only) {
            Ok(true) => measured += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                errors += 1;
                println!("ERROR {}: {e}", path.display());
            }
        }
    }
    println!();
    println!(
        "{:14} {:>8} {:>7} {:>7} {:>6} {:>8} {:>9} {:>12}",
        "release", "written", "paired", "groups", "agree", "relation", "disagree", "export only"
    );
    for (release, t) in &tally {
        println!(
            "{release:14} {:>8} {:>7} {:>7} {:>6} {:>8} {:>9} {:>12}",
            t.written, t.paired, t.groups, t.agree, t.relation, t.disagree, t.export_only
        );
    }
    println!("{skipped} drawings skipped (no twin or no AutoCAD export of the release)");
    for (n, c) in &export_only {
        println!("export only: {n} x{c}");
    }
    if measured == 0 && errors == 0 {
        println!(
            "NOT MEASURED: no drawing reached the converter with an AutoCAD export to compare"
        );
        return ExitCode::SUCCESS;
    }
    for f in &failures {
        println!("FAILED {f}");
    }
    if errors > 0 || !failures.is_empty() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
