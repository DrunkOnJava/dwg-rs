//! `dwg-to-dxf`'s OBJECTS section against AutoCAD's own DXF export (#126).
//!
//! # What this proves
//!
//! For each DWG that has an AutoCAD DXF export of the same drawing, the probe
//! converts it with [`dwg::dxf_convert::convert_dwg_to_dxf`] to the DWG's own
//! release, reads the output back with [`dwg::dxf_reader`] and pairs every
//! record of its OBJECTS section with the export's OBJECTS record of the same
//! handle. A paired record must agree:
//!
//! - the owner (330) and the groups after it, as ordered lists of code and value
//!   (text and integers exactly, reals to the precision AutoCAD printed), with a
//!   group the export lacks counted **extra**;
//! - a DICTIONARY's entries (key, group 350 or 360, handle) as a list: every
//!   entry written is the export's, in its order, and an entry the record
//!   lacks is **omitted** only when the object it names is defined nowhere in
//!   the output;
//! - the `102 {ACAD_REACTORS` and `{ACAD_XDICTIONARY` brackets as wholes, or
//!   omitted only when a handle in them is defined nowhere in the output;
//! - the order of the paired records, which must be the export's.
//!
//! Every handle a record of the output names, as its owner, a reactor, its extension
//! dictionary, a dictionary entry or default or a GROUP member, must resolve to a
//! record of the same output (the handles inside an XRECORD's data are an
//! application's and are only counted); the table of those references is printed.
//! A synthetic check writes an XRECORD whose group 102 strings hold a newline and a
//! character outside ASCII and reads them back.
//!
//! A record of the export of a kind the converter writes with no record written
//! is a failure, unless its owner is a record the output does not write; one of a kind it does not write yet is counted by kind; one the
//! export creates itself (a handle at or above the DWG's `$HANDSEED`) is not in
//! the DWG. Any disagreement, extra, unexplained omission, wrong order, missing
//! record or unreadable output exits 1. AC1014 drawings are skipped.
//!
//! # How to verify
//!
//! ```text
//! cargo run --release --features dxf-reader --example probe_dxf_objects -- <file-or-dir> ...
//! ```
//!
//! Directories are searched recursively for `.dwg` and `.dxf` files; a DWG's twin
//! is the DXF with the same stem, or the stem plus `_ascii`. With no DWG that has
//! a twin it prints `NOT MEASURED` and exits 0.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dwg::dxf::DxfVersion;
use dwg::dxf_objects::kinds;
use dwg::dxf_reader::{DxfDocument, DxfRecord, DxfValue};
use dwg::{DwgFile, Version};

#[derive(Default, Clone)]
struct Tally {
    written: usize,
    paired: usize,
    groups: usize,
    agree: usize,
    relation: usize,
    disagree: usize,
    extra: usize,
    omitted_groups: usize,
    unwritten: usize,
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
    if a == b || (a - b).abs() <= 1.1e-16 {
        return true;
    }
    let rel = 10f64.powi(-(i32::from(digits.max(2)) - 1)) * 2.0;
    (a - b).abs() <= a.abs().max(b.abs()) * rel
}

fn same(a: &DxfValue, b: &DxfValue, digits: u8) -> bool {
    match (a, b) {
        (DxfValue::Real { value: x, .. }, DxfValue::Real { value: y, .. }) => close(*x, *y, digits),
        (x, y) => x == y,
    }
}

/// The groups of `r` outside the `102` brackets, without the handle (5), the
/// owner (330) and, for a dictionary, the entries.
fn body(r: &DxfRecord) -> Vec<(u16, DxfValue)> {
    let dictionary = r.kind == "DICTIONARY" || r.kind == "ACDBDICTIONARYWDFLT";
    // The record's own handle and owner are the first 5 and the first 330; a 330
    // after them is a group of the object (an XRECORD's data, a LAYOUT's block
    // record) and is compared.
    let (mut handle, mut owner) = (false, false);
    r.top_level()
        .filter(|g| match g.code {
            5 if !handle => {
                handle = true;
                false
            }
            330 if !owner => {
                owner = true;
                false
            }
            _ => true,
        })
        .filter(|g| !(dictionary && matches!(g.code, 3 | 350 | 360)))
        .map(|g| (g.code, g.value.clone()))
        .collect()
}

/// A dictionary's entries as (key, code, handle).
fn entries(r: &DxfRecord) -> Vec<(String, u16, u64)> {
    let mut out = Vec::new();
    let mut key: Option<String> = None;
    for g in r.top_level() {
        match g.code {
            3 => key = g.value.as_str().map(str::to_string),
            350 | 360 => {
                if let (Some(k), Some(h)) = (key.take(), g.value.as_handle()) {
                    out.push((k, g.code, h));
                }
            }
            _ => {}
        }
    }
    out
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

/// Every handle an OBJECTS record names, by what names it: the references written
/// and those that resolve to no record of the output. An owner, a reactor, an
/// extension dictionary, a dictionary entry or default and a GROUP member are
/// structural and must all resolve; the handles inside an XRECORD's data are an
/// application's, counted and not held to it.
fn dangling(doc: &DxfDocument) -> BTreeMap<&'static str, (usize, usize)> {
    let known: BTreeSet<u64> = doc.handles().collect();
    let mut out: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
    let mut note = |what: &'static str, v: &DxfValue| {
        if let Some(h) = v.as_handle() {
            let e = out.entry(what).or_default();
            e.0 += 1;
            if h != 0 && !known.contains(&h) {
                e.1 += 1;
            }
        }
    };
    for r in objects(doc) {
        let mut owner_seen = false;
        for g in r.top_level() {
            let what = match (r.kind.as_str(), g.code) {
                (_, 330) if !owner_seen => {
                    owner_seen = true;
                    "owner (330)"
                }
                ("DICTIONARY" | "ACDBDICTIONARYWDFLT", 350 | 360) => "dictionary entry (350, 360)",
                ("ACDBDICTIONARYWDFLT", 340) => "dictionary default (340)",
                ("GROUP", 340) => "GROUP member (340)",
                ("XRECORD", 320..=369 | 390..=399) => "XRECORD data handle (informational)",
                (_, 320..=369 | 390..=399) => "other reference",
                _ => continue,
            };
            note(what, &g.value);
        }
        let b = brackets(r);
        for (name, what) in [
            ("ACAD_REACTORS", "reactor (330)"),
            ("ACAD_XDICTIONARY", "extension dictionary (360)"),
        ] {
            for (_, v) in b.get(name).into_iter().flatten() {
                note(what, v);
            }
        }
    }
    out
}

/// An XRECORD's application data written through the record writer: a group 102
/// string with a newline must stay one group, and one outside ASCII must be
/// escaped for the release like every other string. Synthetic (no corpus drawing
/// has either), so it measures the writer and not a drawing.
fn escape_check() -> Result<String, String> {
    use dwg::dxf::DxfWriter;
    use dwg::dxf_entities::{Entities, EntityRecord};
    use dwg::dxf_sections::HeaderGroup;
    let record = EntityRecord {
        kind: "XRECORD",
        handle: 0x10,
        owner: 0x1,
        block: 0x7,
        reactors: Vec::new(),
        xdictionary: None,
        groups: vec![
            HeaderGroup::Text(100, "AcDbXrecord".into()),
            HeaderGroup::Text(102, "line one\nline two".into()),
            HeaderGroup::Text(102, "caf\u{e9}".into()),
            HeaderGroup::Text(100, "marker\nname \u{e9}".into()),
        ],
    };
    let entities = Entities {
        records: vec![record],
        model_space: 0x7,
        ..Default::default()
    };
    let mut w = DxfWriter::with_version(DxfVersion::R2004);
    dwg::dxf_entities::write(&mut w, &entities);
    w.finish();
    let doc =
        DxfDocument::parse(w.take_output().as_bytes()).map_err(|e| format!("read back: {e}"))?;
    let rec = doc
        .records
        .iter()
        .find(|r| r.kind == "XRECORD")
        .ok_or("the XRECORD was not written")?;
    let strings: Vec<String> = rec
        .groups
        .iter()
        .filter(|g| g.code == 102)
        .filter_map(|g| g.value.as_str().map(str::to_string))
        .collect();
    let subclass: Vec<String> = rec
        .groups
        .iter()
        .filter(|g| g.code == 100)
        .filter_map(|g| g.value.as_str().map(str::to_string))
        .collect();
    let ok = strings.len() == 2
        && strings.iter().all(|t| !t.contains('\n'))
        && strings[1] != "caf\u{e9}"
        && subclass.len() == 2
        && subclass[0] == "AcDbXrecord"
        && !subclass[1].contains('\n')
        && subclass[1] != "marker\nname \u{e9}";
    let shown = format!("{strings:?}, group 100 {subclass:?}");
    if ok {
        Ok(shown)
    } else {
        Err(format!("group 102 strings read back as {shown}"))
    }
}

fn objects(doc: &DxfDocument) -> Vec<&DxfRecord> {
    doc.records
        .iter()
        .filter(|r| doc.section_name(r) == "OBJECTS")
        .collect()
}

struct Outcome {
    tallies: BTreeMap<String, Tally>,
    reasons: BTreeMap<String, usize>,
    failures: Vec<String>,
    entries_written: usize,
    entries_omitted: usize,
    dangling: BTreeMap<&'static str, (usize, usize)>,
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
    let ours = DxfDocument::parse(out.as_bytes()).map_err(|e| format!("read back: {e}"))?;
    let digits = theirs.real_precision();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    let mut o = Outcome {
        tallies: BTreeMap::new(),
        reasons: BTreeMap::new(),
        failures: Vec::new(),
        entries_written: 0,
        entries_omitted: 0,
        dangling: dangling(&ours),
    };
    for (what, (refs, bad)) in &o.dangling {
        if *bad > 0 && !what.contains("informational") {
            o.failures.push(format!(
                "{name}: {bad} of {refs} {what} references resolve to no record"
            ));
        }
    }
    let our_recs = objects(&ours);
    let their_recs = objects(&theirs);
    let by_handle: BTreeMap<u64, &DxfRecord> = their_recs
        .iter()
        .filter_map(|r| r.handle.map(|h| (h, *r)))
        .collect();
    let their_index: BTreeMap<u64, usize> = their_recs
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.handle.map(|h| (h, i)))
        .collect();
    let defined = |h: u64| ours.record_by_handle(h).is_some();
    let is_dictionary = |k: &str| k == "DICTIONARY" || k == "ACDBDICTIONARYWDFLT";
    // The key under which each object of the export sits in its dictionary.
    let mut key_of: BTreeMap<u64, String> = BTreeMap::new();
    for t in their_recs.iter().filter(|t| is_dictionary(&t.kind)) {
        for (k, _, x) in entries(t) {
            key_of.entry(x).or_insert(k);
        }
    }
    let has_xdictionary = |h: u64| {
        theirs
            .record_by_handle(h)
            .is_some_and(|r| brackets(r).contains_key("ACAD_XDICTIONARY"))
    };

    // Pairing. A record the export has under the same handle pairs by it. An
    // object the export registers again under a handle of its own (one at or
    // above the DWG's `$HANDSEED`) pairs by what names it: an extension
    // dictionary by its owner, anything else by its key in the dictionary that
    // holds it, which is already paired.
    let mut rel: BTreeMap<u64, u64> = BTreeMap::new();
    let mut taken: BTreeSet<u64> = BTreeSet::new();
    for r in &our_recs {
        if let Some(h) = r.handle.filter(|h| by_handle.contains_key(h)) {
            rel.insert(h, h);
            taken.insert(h);
        }
    }
    for r in &our_recs {
        let Some(h) = r.handle else { continue };
        if !rel.contains_key(&h) && is_dictionary(&r.kind) {
            let owner = r.owner.map(|ow| rel.get(&ow).copied().unwrap_or(ow));
            let twin = their_recs.iter().find(|t| {
                t.kind == r.kind
                    && t.owner == owner
                    && t.handle.is_some_and(|x| x >= seed && !taken.contains(&x))
            });
            if let Some(x) = twin.and_then(|t| t.handle) {
                rel.insert(h, x);
                taken.insert(x);
            }
        }
        if let (Some(&x), true) = (rel.get(&h), is_dictionary(&r.kind)) {
            let (ea, eb) = (entries(r), entries(by_handle[&x]));
            for (key, code, ho) in ea {
                let Some((_, _, ht)) = eb.iter().find(|(k, c, _)| *k == key && *c == code) else {
                    continue;
                };
                let (Some(mine), Some(theirs_rec)) = (
                    our_recs.iter().find(|q| q.handle == Some(ho)),
                    by_handle.get(ht),
                ) else {
                    continue;
                };
                if !rel.contains_key(&ho)
                    && *ht >= seed
                    && !taken.contains(ht)
                    && mine.kind == theirs_rec.kind
                {
                    rel.insert(ho, *ht);
                    taken.insert(*ht);
                }
            }
        }
    }

    // Objects the export leaves out: an extension dictionary whose owner has no
    // `{ACAD_XDICTIONARY` in the export, an entry the export's dictionary does not
    // hold, and what an object left out owns.
    let holder: BTreeMap<u64, (u64, String)> = our_recs
        .iter()
        .filter(|r| is_dictionary(&r.kind))
        .flat_map(|d| {
            let dh = d.handle.unwrap_or(0);
            entries(d).into_iter().map(move |(k, _, x)| (x, (dh, k)))
        })
        .collect();
    let mut dropped: BTreeMap<u64, &'static str> = BTreeMap::new();
    for r in &our_recs {
        let Some(h) = r.handle.filter(|h| !rel.contains_key(h)) else {
            continue;
        };
        let owner = r.owner.unwrap_or(0);
        let why = if is_dictionary(&r.kind)
            && theirs.record_by_handle(owner).is_some()
            && !has_xdictionary(owner)
        {
            Some(
                "an extension dictionary the export leaves out (its owner has no {ACAD_XDICTIONARY in the export)",
            )
        } else if holder.get(&h).is_some_and(|(d, key)| {
            rel.get(d)
                .and_then(|x| by_handle.get(x))
                .is_some_and(|dt| !entries(dt).iter().any(|(k, _, _)| k == key))
        }) {
            Some("an entry the export's dictionary does not hold")
        } else if dropped.contains_key(&owner) {
            Some("an object inside one the export leaves out")
        } else if theirs.record_by_handle(owner).is_none() && !rel.contains_key(&owner) {
            Some("an object whose owner is not in the export")
        } else {
            None
        };
        if let Some(why) = why {
            dropped.insert(h, why);
        }
    }
    let supported = kinds(file.version());
    let mut last_index = 0usize;
    for r in &our_recs {
        let kind = r.kind.clone();
        let t = o.tallies.entry(kind.clone()).or_default();
        t.written += 1;
        let Some(h) = r.handle else {
            o.failures
                .push(format!("{name} {kind}: written without a handle"));
            continue;
        };
        let Some(x) = rel.get(&h).copied() else {
            if let Some(why) = dropped.get(&h) {
                *o.reasons
                    .entry(format!("{kind} left out by the export ({why})"))
                    .or_default() += 1;
                continue;
            }
            o.failures.push(format!(
                "{name} {kind} {h:X}: written but AutoCAD's OBJECTS has no such record (owner {:X?}, first key {:?}, held as {:X?}, owner paired with {:X?})",
                r.owner,
                r.first(3).and_then(DxfValue::as_str),
                holder.get(&h),
                r.owner.and_then(|ow| rel.get(&ow))
            ));
            continue;
        };
        let want = by_handle[&x];
        if want.kind != r.kind {
            o.failures.push(format!(
                "{name} {h:X}: written as {kind} but AutoCAD wrote {}",
                want.kind
            ));
            continue;
        }
        t.paired += 1;
        if x != h {
            *o.reasons
                .entry(format!(
                    "relation: {kind} (the export registers it again under a handle of its own)"
                ))
                .or_default() += 1;
        }
        let at = their_index[&x];
        if at < last_index {
            let ahead = their_recs[last_index];
            o.failures.push(format!(
                "{name} {kind} {h:X}: written before a record AutoCAD's OBJECTS has ahead of it (twin position {at}, after one at {last_index}: {} {:X?}, owner {:X?})",
                ahead.kind, ahead.handle, ahead.owner
            ));
        }
        last_index = at;
        t.groups += 1;
        let owner_ok = match (r.owner, want.owner) {
            (Some(a), Some(b)) => rel.get(&a).copied().unwrap_or(a) == b,
            (a, b) => a == b,
        };
        if owner_ok {
            t.agree += 1;
        } else {
            t.disagree += 1;
            o.failures.push(format!(
                "{name} {kind} {h:X}: owner {:?} but AutoCAD wrote {:?}",
                r.owner, want.owner
            ));
        }

        let (our_brackets, their_brackets) = (brackets(r), brackets(want));
        for (bname, theirs_in) in &their_brackets {
            let mapped = |v: &[(u16, DxfValue)]| -> Vec<(u16, DxfValue)> { v.to_vec() };
            match our_brackets.get(bname) {
                Some(ours_in) if mapped(ours_in) == *theirs_in => {
                    t.groups += 1;
                    t.agree += 1;
                }
                Some(ours_in)
                    if ours_in.len() == theirs_in.len()
                        && ours_in.iter().zip(theirs_in).all(|((ca, va), (cb, vb))| {
                            ca == cb
                                && (va == vb
                                    || va
                                        .as_handle()
                                        .zip(vb.as_handle())
                                        .is_some_and(|(a, b)| rel.get(&a) == Some(&b)))
                        }) =>
                {
                    t.groups += 1;
                    t.agree += 1;
                }
                Some(_) => {
                    t.groups += 1;
                    t.disagree += 1;
                    o.failures.push(format!(
                        "{name} {kind} {h:X}: bracket {bname} differs from AutoCAD's"
                    ));
                }
                None => {
                    t.omitted_groups += 1;
                    let missing = theirs_in
                        .iter()
                        .filter_map(|(_, v)| v.as_handle())
                        .any(|y| y >= seed || !defined(y));
                    if missing {
                        *o.reasons
                            .entry(format!(
                                "{kind} bracket {bname} (it names an object the output does not define)"
                            ))
                            .or_default() += 1;
                    } else {
                        o.failures.push(format!(
                            "{name} {kind} {h:X}: bracket {bname} omitted though every handle in it is defined"
                        ));
                    }
                }
            }
        }
        for bname in our_brackets.keys() {
            if !their_brackets.contains_key(bname) {
                if bname == "ACAD_XDICTIONARY"
                    && our_brackets[bname]
                        .iter()
                        .filter_map(|(_, v)| v.as_handle())
                        .all(|x| dropped.contains_key(&x))
                {
                    *o.reasons
                        .entry(format!(
                            "{kind} extension dictionary the export leaves out (it names a dictionary the export does not have)"
                        ))
                        .or_default() += 1;
                    continue;
                }
                t.extra += 1;
                o.failures.push(format!(
                    "{name} {kind} {h:X}: bracket {bname} written but absent from the export"
                ));
            }
        }

        let (a, mut b) = (body(r), body(want));
        // A GROUP lists only the members the output holds.
        if kind == "GROUP" {
            b.retain(|(code, v)| {
                let held = *code == 340 && v.as_handle().is_some_and(|x| !defined(x));
                if held {
                    t.omitted_groups += 1;
                    *o.reasons
                        .entry(
                            "GROUP member 340 (an entity the output does not write: an MLINE, a VIEWPORT)"
                                .to_string(),
                        )
                        .or_default() += 1;
                }
                !held
            });
        }
        let n = a.len().max(b.len());
        for i in 0..n {
            match (a.get(i), b.get(i)) {
                (Some((ca, va)), Some((cb, vb))) => {
                    t.groups += 1;
                    let handle_group = (320..=369).contains(ca) || (390..=399).contains(ca);
                    let same_handle = handle_group
                        && va
                            .as_handle()
                            .zip(vb.as_handle())
                            .is_some_and(|(p, q)| rel.get(&p) == Some(&q));
                    let key = key_of.get(&x).map(String::as_str).unwrap_or("");
                    let why = if ca != cb {
                        None
                    } else if handle_group && vb.as_handle().is_some_and(|q| q >= seed) {
                        Some(
                            "a handle of an object the export registers again under one of its own",
                        )
                    } else {
                        match (kind.as_str(), key, *ca) {
                            ("XRECORD", "DWGPROPS", 40 | 42) => Some(
                                "the editing time and the save date, which AutoCAD updates when it exports",
                            ),
                            ("XRECORD", "ACAD_LAST_SAVED_VERSION_INFO", 300 | 302 | 303) => Some(
                                "the saved file's identity, which AutoCAD writes for its export",
                            ),
                            ("XRECORD", "ACDB_RECOMPOSE_DATA", 330) => {
                                Some("the styles AutoCAD lists again when it loads the file")
                            }
                            _ => None,
                        }
                    };
                    if ca == cb && (same(va, vb, digits) || same_handle) {
                        t.agree += 1;
                    } else if let Some(why) = why {
                        t.relation += 1;
                        *o.reasons
                            .entry(format!("relation: {kind} group {ca} ({why})"))
                            .or_default() += 1;
                    } else {
                        t.disagree += 1;
                        o.failures.push(format!(
                            "{name} {kind} {h:X}: group {i} is {ca} {va} but AutoCAD wrote {cb} {vb}"
                        ));
                    }
                }
                (Some((ca, _)), None)
                    if kind == "XRECORD"
                        && key_of.get(&x).is_some_and(|k| k == "ACAD_XREC_ROUNDTRIP") =>
                {
                    t.relation += 1;
                    *o.reasons
                        .entry(format!(
                            "relation: XRECORD group {ca} (round-trip data the export no longer holds: AutoCAD consumes it when it loads the file)"
                        ))
                        .or_default() += 1;
                }
                (Some((ca, _)), None) => {
                    t.extra += 1;
                    o.failures.push(format!(
                        "{name} {kind} {h:X}: group {ca} (position {i}) written but absent from the export"
                    ));
                }
                (None, Some((cb, vb)))
                    if matches!(cb, 331 | 345 | 346)
                        && vb.as_handle().is_some_and(|x| !defined(x)) =>
                {
                    t.omitted_groups += 1;
                    *o.reasons
                        .entry(format!(
                            "{kind} group {cb} (it names an entity or UCS record the output does not define)"
                        ))
                        .or_default() += 1;
                }
                (None, Some((330, _)))
                    if kind == "XRECORD"
                        && key_of.get(&x).is_some_and(|k| k == "ACDB_RECOMPOSE_DATA") =>
                {
                    t.relation += 1;
                    *o.reasons
                        .entry("relation: XRECORD group 330 (the styles AutoCAD lists again in ACDB_RECOMPOSE_DATA when it loads the file)".to_string())
                        .or_default() += 1;
                }
                (None, Some((cb, _))) => {
                    t.omitted_groups += 1;
                    if (1000..=1071).contains(cb) {
                        *o.reasons
                            .entry(format!(
                                "{kind} group {cb} (extended data is not decoded for objects)"
                            ))
                            .or_default() += 1;
                    } else {
                        o.failures.push(format!(
                            "{name} {kind} {h:X}: AutoCAD writes group {cb} (position {i}) and the record omits it with no stated reason"
                        ));
                    }
                }
                (None, None) => {}
            }
        }

        if is_dictionary(&kind) {
            let (ea, eb) = (entries(r), entries(want));
            let mut at = 0usize;
            let skip_dropped = |at: &mut usize, reasons: &mut BTreeMap<String, usize>| {
                while ea.get(*at).is_some_and(|m| dropped.contains_key(&m.2)) {
                    *reasons
                        .entry(format!(
                            "{kind} entry (it names an object the export leaves out)"
                        ))
                        .or_default() += 1;
                    *at += 1;
                }
            };
            for e in &eb {
                t.groups += 1;
                skip_dropped(&mut at, &mut o.reasons);
                let matches_ours = ea.get(at).is_some_and(|m| {
                    m.0 == e.0 && m.1 == e.1 && (m.2 == e.2 || rel.get(&m.2) == Some(&e.2))
                });
                if matches_ours {
                    at += 1;
                    t.agree += 1;
                    o.entries_written += 1;
                } else if e.2 >= seed {
                    t.omitted_groups += 1;
                    o.entries_omitted += 1;
                    *o.reasons
                        .entry(format!(
                            "{kind} entry (the export made the object itself, a handle at or above the DWG's $HANDSEED)"
                        ))
                        .or_default() += 1;
                } else if !defined(e.2) {
                    t.omitted_groups += 1;
                    o.entries_omitted += 1;
                    *o.reasons
                        .entry(format!(
                            "{kind} entry (the object it names is not defined in the output)"
                        ))
                        .or_default() += 1;
                } else {
                    t.disagree += 1;
                    o.failures.push(format!(
                        "{name} {kind} {h:X}: entry {} {:X} omitted or out of order though its object is defined (ours {} entries, AutoCAD {}; ours at {at}: {:?})",
                        e.0,
                        e.2,
                        ea.len(),
                        eb.len(),
                        ea.get(at)
                    ));
                }
            }
            skip_dropped(&mut at, &mut o.reasons);
            if at != ea.len() {
                t.extra += ea.len() - at;
                o.failures.push(format!(
                    "{name} {kind} {h:X}: {} entries written that AutoCAD's record lacks or orders differently",
                    ea.len() - at
                ));
            }
        }
    }
    for r in &their_recs {
        let Some(h) = r.handle else { continue };
        if taken.contains(&h) || h >= seed {
            continue;
        }
        let t = o.tallies.entry(r.kind.clone()).or_default();
        t.unwritten += 1;
        let owner_missing = r.owner.is_some_and(|ow| ow != 0 && !defined(ow));
        if owner_missing {
            *o.reasons
                .entry(format!(
                    "{} left out because its owner is not written (the output keeps no record whose owner it lacks)",
                    r.kind
                ))
                .or_default() += 1;
        } else if supported.contains(&r.kind.as_str()) {
            o.failures.push(format!(
                "{name} {} {h:X}: in the export, not written, though the converter writes {}",
                r.kind, r.kind
            ));
        }
    }
    println!(
        "{name:28} {}  {} objects written, {} paired",
        file.version(),
        o.tallies.values().map(|t| t.written).sum::<usize>(),
        o.tallies.values().map(|t| t.paired).sum::<usize>()
    );
    Ok(Some(o))
}

fn main() -> ExitCode {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        eprintln!("usage: probe_dxf_objects <file-or-dir> ...");
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
    let mut all: BTreeMap<String, Tally> = BTreeMap::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    let (mut entries_written, mut entries_omitted) = (0usize, 0usize);
    let mut measured = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;
    let mut refs: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
    for path in &dwgs {
        match probe_file(path, &twins) {
            Ok(Some(o)) => {
                measured += 1;
                for (k, (n, bad)) in &o.dangling {
                    let a = refs.entry(k).or_default();
                    a.0 += n;
                    a.1 += bad;
                }
                for (k, t) in o.tallies {
                    let a = all.entry(k).or_default();
                    a.written += t.written;
                    a.paired += t.paired;
                    a.groups += t.groups;
                    a.agree += t.agree;
                    a.relation += t.relation;
                    a.disagree += t.disagree;
                    a.extra += t.extra;
                    a.omitted_groups += t.omitted_groups;
                    a.unwritten += t.unwritten;
                }
                for (r, n) in o.reasons {
                    *reasons.entry(r).or_default() += n;
                }
                failures.extend(o.failures);
                entries_written += o.entries_written;
                entries_omitted += o.entries_omitted;
            }
            Ok(None) => skipped += 1,
            Err(e) => {
                errors += 1;
                println!("ERROR {}: {e}", path.display());
            }
        }
    }
    println!();
    println!(
        "{:22} {:>8} {:>7} {:>7} {:>6} {:>8} {:>9} {:>6} {:>7} {:>12}",
        "kind",
        "written",
        "paired",
        "groups",
        "agree",
        "relation",
        "disagree",
        "extra",
        "om.grp",
        "not written"
    );
    for (k, t) in &all {
        println!(
            "{k:22} {:>8} {:>7} {:>7} {:>6} {:>8} {:>9} {:>6} {:>7} {:>12}",
            t.written,
            t.paired,
            t.groups,
            t.agree,
            t.relation,
            t.disagree,
            t.extra,
            t.omitted_groups,
            t.unwritten
        );
    }
    println!(
        "{skipped} drawings skipped (no twin or no AutoCAD export of the release); dictionary entries: {entries_written} written, {entries_omitted} omitted"
    );
    for (r, n) in &reasons {
        println!("omitted: {r} x{n}");
    }
    println!(
        "{:38} {:>10} {:>14}",
        "handle references written", "written", "to no record"
    );
    for (what, (n, bad)) in &refs {
        println!("{what:38} {n:>10} {bad:>14}");
    }
    match escape_check() {
        Ok(shown) => println!("XRECORD group 102 escaping (synthetic): ok, read back {shown}"),
        Err(e) => {
            failures.push(format!("XRECORD group 102 escaping (synthetic): {e}"));
        }
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
