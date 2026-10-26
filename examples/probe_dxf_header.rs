//! `dwg-to-dxf`'s HEADER section against AutoCAD's own DXF export (#126).
//!
//! # What this proves
//!
//! For each DWG that has an AutoCAD DXF export of the same drawing
//! (`tools/corpus/pins.tsv`), the probe converts the DWG with
//! [`dwg::dxf_convert::convert_dwg_to_dxf`] to the DXF release of the DWG's own,
//! reads the output back with [`dwg::dxf_reader`] and compares every `$VARIABLE`
//! of its `HEADER` with the export's:
//!
//! - **agree**: the same group codes, and equal values (text and handles exactly,
//!   integers exactly, reals to the precision AutoCAD printed);
//! - **relation**: the twin is a later save, so `$TDUUPDATE`, `$TDINDWG` and
//!   `$TDUSRTIMER` may not be earlier in it (the last two by equal amounts),
//!   `$HANDSEED` not lower, and `$VERSIONGUID` is a different GUID;
//! - **extra**: a variable the converter wrote and the export does not: a
//!   failure, because a variable AutoCAD omits must be omitted;
//! - **omitted**: a variable the export writes and the converter does not. Each
//!   needs a stated reason (`$ACADMAINTVER` and the local `$TDCREATE` and
//!   `$TDUPDATE`, which the DWG does not hold; a name for a symbol record the
//!   converter's output does not define, since it writes layer 0 and no other
//!   symbol table; a true colour, which DXF writes as an approximate index; a
//!   null `$INTERFEREOBJVS` or `$INTERFEREVPVS`, where the export names the
//!   default visual style). One without a reason is a failure.
//!
//! Any disagreement, extra, unexplained omission, unreadable output or file the
//! converter wrote no header for exits 1. AC1014 drawings are skipped and said to
//! be: no AutoCAD export of R12 or R14 exists to measure the set against, so the
//! converter writes `$ACADVER` alone for those targets.
//!
//! The output's other sections are not compared (the converter's TABLES and
//! ENTITIES carry no handles, so the reader refuses the whole file); the probe
//! reads the converter's real output as far as the end of its HEADER.
//!
//! # How to verify
//!
//! ```text
//! cargo run --release --features dxf-reader --example probe_dxf_header -- <file-or-dir> ...
//! ```
//!
//! Directories are searched recursively for `.dwg` and `.dxf` files; a DWG's twin
//! is the DXF with the same stem, or the stem plus `_ascii`. With no DWG at all it
//! prints `NOT MEASURED` and exits 0.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dwg::dxf::DxfVersion;
use dwg::dxf_reader::{DxfDocument, DxfGroup, DxfValue};
use dwg::header_vars::HeaderVars;
use dwg::{DwgFile, Version};

#[derive(Default)]
struct Tally {
    files: usize,
    written: usize,
    agree: usize,
    relation: usize,
    disagree: usize,
    extra: usize,
    omitted: usize,
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

fn same_groups(ours: &[DxfGroup], theirs: &[DxfGroup], digits: u8) -> bool {
    ours.len() == theirs.len()
        && ours.iter().zip(theirs).all(|(a, b)| {
            a.code == b.code
                && match (&a.value, &b.value) {
                    (DxfValue::Real { value: x, .. }, DxfValue::Real { value: y, .. }) => {
                        close(*x, *y, digits)
                    }
                    (x, y) => x == y,
                }
        })
}

fn show(g: &[DxfGroup]) -> String {
    g.iter()
        .map(|g| format!("{}:{}", g.code, g.value))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The converter's output as far as the end of its HEADER, closed so the
/// reader accepts it.
fn header_only(out: &str) -> Option<String> {
    let at = out.find("ENDSEC")?;
    let end = out[at..].find('\n')? + at + 1;
    Some(format!("{}0\nEOF\n", &out[..end]))
}

fn real_of(doc: &DxfDocument, name: &str) -> Option<f64> {
    doc.header_var(name)?.first()?.value.as_f64()
}

fn relation(name: &str, ours: &DxfDocument, theirs: &DxfDocument) -> Result<(), String> {
    match name {
        "$TDUUPDATE" | "$TDINDWG" | "$TDUSRTIMER" => {
            let (a, b) = (real_of(ours, name), real_of(theirs, name));
            match (a, b) {
                (Some(a), Some(b)) if b - a >= -1e-9 => Ok(()),
                other => Err(format!("{name} {other:?}: the export is earlier")),
            }
        }
        "$HANDSEED" => {
            let h = |d: &DxfDocument| {
                d.header_var(name)
                    .and_then(|g| g.first())
                    .and_then(|g| g.value.as_handle())
            };
            match (h(ours), h(theirs)) {
                (Some(a), Some(b)) if b >= a => Ok(()),
                other => Err(format!("$HANDSEED {other:?}")),
            }
        }
        "$VERSIONGUID" => {
            let t = |d: &DxfDocument| {
                d.header_var(name)
                    .and_then(|g| g.first())
                    .and_then(|g| g.value.as_str().map(str::to_owned))
            };
            match (t(ours), t(theirs)) {
                (Some(a), Some(b)) if looks_like_guid(&a) && looks_like_guid(&b) => Ok(()),
                other => Err(format!("$VERSIONGUID {other:?}")),
            }
        }
        _ => Err("not a relation variable".into()),
    }
}

fn timers_advance_equally(ours: &DxfDocument, theirs: &DxfDocument) -> Result<(), String> {
    let d = |n: &str| match (real_of(ours, n), real_of(theirs, n)) {
        (Some(a), Some(b)) => Some(b - a),
        _ => None,
    };
    match (d("$TDINDWG"), d("$TDUSRTIMER")) {
        (Some(a), Some(b)) if (a - b).abs() <= 1e-8 => Ok(()),
        other => Err(format!("TDINDWG and TDUSRTIMER advanced {other:?}")),
    }
}

/// Why the converter may leave out a variable the export writes, or `None`.
fn omission_reason(name: &str, header: &HeaderVars, theirs: &DxfDocument) -> Option<&'static str> {
    let named = theirs
        .header_var(name)
        .and_then(|g| g.first())
        .and_then(|g| g.value.as_str())
        .unwrap_or("");
    match name {
        // The converter defines layer 0 and no other symbol record, so a name
        // for anything else would point at nothing.
        "$CLAYER" if named != "0" => Some("names a layer the output does not define"),
        "$TEXTSTYLE" | "$CELTYPE" | "$DIMSTYLE" | "$DIMTXSTY" | "$DIMLDRBLK" | "$DIMBLK"
        | "$DIMBLK1" | "$DIMBLK2" | "$DIMLTYPE" | "$DIMLTEX1" | "$DIMLTEX2" | "$CMLSTYLE"
        | "$UCSNAME" | "$PUCSNAME" | "$UCSBASE" | "$PUCSBASE" | "$UCSORTHOREF"
        | "$PUCSORTHOREF"
            if !named.is_empty() =>
        {
            Some("names a symbol record the output does not define")
        }
        "$ACADMAINTVER" => Some(
            "not derivable: the file header says 15 where AC1015 exports 20, and 0 where AC1032 exports 228 or 329",
        ),
        "$TDCREATE" | "$TDUPDATE" => Some("local time, not stored"),
        "$CECOLOR" | "$DIMCLRD" | "$DIMCLRE" | "$DIMCLRT" | "$DIMTFILLCLR" | "$INTERFERECOLOR" => {
            match header.get(name) {
                Some(dwg::header_vars::Value::Color(c)) if c.aci().is_none() => {
                    Some("a true colour has no index")
                }
                _ => None,
            }
        }
        "$INTERFEREOBJVS" | "$INTERFEREVPVS" => match header.get_handle(name) {
            Some(h) if h.value == 0 => {
                Some("null in the DWG, a default visual style in the export")
            }
            _ => None,
        },
        _ => None,
    }
}

struct Outcome {
    tally: Tally,
    omitted_reasons: BTreeMap<String, usize>,
    notes: Vec<String>,
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
    let theirs_bytes = std::fs::read(twin).map_err(|e| format!("read twin: {e}"))?;
    let theirs = DxfDocument::parse(&theirs_bytes).map_err(|e| format!("twin: {e}"))?;
    let header = file
        .header_vars_strict()
        .ok_or("no AcDb:Header")?
        .map_err(|e| format!("header: {e}"))?;
    let out =
        dwg::dxf_convert::convert_dwg_to_dxf(&file, target).map_err(|e| format!("convert: {e}"))?;
    let slice = header_only(&out).ok_or("the output has no HEADER")?;
    let ours = DxfDocument::parse(slice.as_bytes()).map_err(|e| format!("read back: {e}"))?;

    let digits = theirs.real_precision();
    let mut o = Outcome {
        tally: Tally::default(),
        omitted_reasons: BTreeMap::new(),
        notes: Vec::new(),
        failures: Vec::new(),
    };
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for v in &ours.header {
        seen.insert(v.name.as_str());
        o.tally.written += 1;
        let Some(want) = theirs.header_var(&v.name) else {
            o.tally.extra += 1;
            o.failures.push(format!(
                "{name} {}: written ({}) but AutoCAD's export omits it",
                v.name,
                show(&v.groups)
            ));
            continue;
        };
        if v.name == "$ACADVER" {
            if same_groups(&v.groups, want, digits) {
                o.tally.agree += 1;
            } else {
                o.tally.disagree += 1;
                o.failures.push(format!(
                    "{name} $ACADVER: {} vs {}",
                    show(&v.groups),
                    show(want)
                ));
            }
            continue;
        }
        if matches!(
            v.name.as_str(),
            "$TDUUPDATE" | "$TDINDWG" | "$TDUSRTIMER" | "$HANDSEED" | "$VERSIONGUID"
        ) {
            let verdict = relation(&v.name, &ours, &theirs).and_then(|()| {
                if v.name == "$TDINDWG" {
                    timers_advance_equally(&ours, &theirs)
                } else {
                    Ok(())
                }
            });
            match verdict {
                Ok(()) if same_groups(&v.groups, want, digits) => o.tally.agree += 1,
                Ok(()) => o.tally.relation += 1,
                Err(why) => {
                    o.tally.disagree += 1;
                    o.failures.push(format!("{name} {}: {why}", v.name));
                }
            }
            continue;
        }
        if same_groups(&v.groups, want, digits) {
            o.tally.agree += 1;
        } else {
            o.tally.disagree += 1;
            o.failures.push(format!(
                "{name} {}: written {} but AutoCAD wrote {}",
                v.name,
                show(&v.groups),
                show(want)
            ));
        }
    }
    for v in &theirs.header {
        if seen.contains(v.name.as_str()) {
            continue;
        }
        o.tally.omitted += 1;
        match omission_reason(&v.name, &header, &theirs) {
            Some(why) => {
                *o.omitted_reasons
                    .entry(format!("{} ({why})", v.name))
                    .or_default() += 1
            }
            None => o.failures.push(format!(
                "{name} {}: AutoCAD writes {} and the converter omits it with no stated reason",
                v.name,
                show(&v.groups)
            )),
        }
    }
    o.tally.files = 1;
    o.notes.push(format!(
        "{name:28} {}  wrote {}, agree {}, relation {}, disagree {}, extra {}, omitted {} (twin {})",
        file.version(),
        o.tally.written,
        o.tally.agree,
        o.tally.relation,
        o.tally.disagree,
        o.tally.extra,
        o.tally.omitted,
        twin.file_name().and_then(|n| n.to_str()).unwrap_or("?")
    ));
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
    let mut by_release: BTreeMap<String, Tally> = BTreeMap::new();
    let mut omitted: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    let mut skipped = 0usize;
    let mut compared = 0usize;
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
                for n in &o.notes {
                    println!("{n}");
                }
                failures.extend(o.failures);
                let t = by_release.entry(release.clone()).or_default();
                t.files += o.tally.files;
                t.written += o.tally.written;
                t.agree += o.tally.agree;
                t.relation += o.tally.relation;
                t.disagree += o.tally.disagree;
                t.extra += o.tally.extra;
                t.omitted += o.tally.omitted;
                let m = omitted.entry(release).or_default();
                for (k, n) in o.omitted_reasons {
                    *m.entry(k).or_default() += n;
                }
            }
            Ok(None) => {
                skipped += 1;
                println!(
                    "{name:28} {release}  skipped: no AutoCAD export of this release to compare with"
                );
            }
            Err(e) => {
                println!("{name:28} FAILED: {e}");
                failures.push(format!("{name}: {e}"));
            }
        }
    }
    println!();
    println!(
        "{:14} {:>5} {:>8} {:>6} {:>9} {:>9} {:>6} {:>8}",
        "release", "files", "written", "agree", "relation", "disagree", "extra", "omitted"
    );
    let mut total = Tally::default();
    for (release, t) in &by_release {
        println!(
            "{release:14} {:>5} {:>8} {:>6} {:>9} {:>9} {:>6} {:>8}",
            t.files, t.written, t.agree, t.relation, t.disagree, t.extra, t.omitted
        );
        total.files += t.files;
        total.written += t.written;
        total.agree += t.agree;
        total.relation += t.relation;
        total.disagree += t.disagree;
        total.extra += t.extra;
        total.omitted += t.omitted;
    }
    println!(
        "{:14} {:>5} {:>8} {:>6} {:>9} {:>9} {:>6} {:>8}",
        "TOTAL",
        total.files,
        total.written,
        total.agree,
        total.relation,
        total.disagree,
        total.extra,
        total.omitted
    );
    println!("{skipped} drawings skipped (no twin or no AutoCAD export of the release)");
    for (release, reasons) in &omitted {
        let list: Vec<String> = reasons.iter().map(|(k, n)| format!("{k} x{n}")).collect();
        println!("\nomitted on {release}: {}", list.join(", "));
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
