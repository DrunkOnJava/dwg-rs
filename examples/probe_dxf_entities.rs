//! `dwg-to-dxf`'s ENTITIES and BLOCKS sections against AutoCAD's own DXF export
//! (#126).
//!
//! # What this proves
//!
//! For each DWG that has an AutoCAD DXF export of the same drawing, the probe
//! converts it with [`dwg::dxf_convert::convert_dwg_to_dxf`] to the DWG's own
//! release, reads the output back with [`dwg::dxf_reader`] and pairs every record
//! of one section (`ENTITIES`, or `BLOCKS` with `--section=BLOCKS`) with the
//! export's record of the same handle there. A paired record must agree group by
//! group, and in the order AutoCAD writes them:
//!
//! - every group written has the export's code and equal values (text and
//!   integers exactly, reals to the precision AutoCAD printed); a group the
//!   export does not have is **extra**;
//! - the `102 {ACAD_REACTORS` and `{ACAD_XDICTIONARY` brackets are compared as
//!   wholes;
//! - a group of the export that the record lacks is **omitted**, and needs a
//!   stated reason (extended data, 1000 to 1071; a colour book colour's RGB and
//!   name; proxy graphics, 92 and 310);
//! - a record of the export of a kind the converter writes, with no record
//!   written, is a failure; one of a kind it does not write yet is counted by
//!   kind and listed; one the export creates itself (a handle at or above the
//!   DWG's `$HANDSEED`) is not in the DWG.
//!
//! With `--section=BLOCKS` it also converts every drawing to every release the
//! writer writes it to, and checks that the block each `INSERT` names (group 2) is
//! a `BLOCK` the same output writes: an `INSERT` of a block left out is a failure.
//! `PROBE_DXF_ENTITIES_DROP_BLOCK=1` removes one written block from that check, a
//! negative run that must exit 1. No corpus drawing has a block the writer has to
//! leave out, so the same run also builds the entities of each drawing once per
//! layer with that layer taken out of the tables: the `BLOCK` and `ENDBLK` on it can
//! no longer be written, and no `INSERT` of such a block may be written either.
//!
//! Any disagreement, extra, unexplained omission, missing record, record out of
//! order, unresolved `INSERT` or unreadable output exits 1. AC1014 drawings are skipped (R12 and R14
//! targets keep the default layer).
//!
//! # How to verify
//!
//! ```text
//! cargo run --release --features dxf-reader --example probe_dxf_entities -- [--section=BLOCKS] <file-or-dir> ...
//! ```
//!
//! Directories are searched recursively for `.dwg` and `.dxf` files; a DWG's twin
//! is the DXF with the same stem, or the stem plus `_ascii`. With no DWG at all it
//! prints `NOT MEASURED` and exits 0.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dwg::dxf::DxfVersion;
use dwg::dxf_entities::kinds;
use dwg::dxf_reader::{DxfDocument, DxfGroup, DxfRecord, DxfValue};
use dwg::dxf_sections::HeaderGroup;
use dwg::{DwgFile, Version};

#[derive(Default, Clone)]
struct Tally {
    written: usize,
    paired: usize,
    groups: usize,
    agree: usize,
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

/// Whether the decoded `a` is what AutoCAD printed as `b`: it prints 16
/// significant digits and at most 16 decimal places, so the tolerance is half a
/// unit of the coarser of the two, plus four ulps for the parse and, for angles,
/// the radian-to-degree conversion (the oracle's rule, `dxf_oracle/scorers.rs`).
fn close(a: f64, b: f64, digits: u8) -> bool {
    if a == b {
        return true;
    }
    let significant = i32::from(digits.max(16));
    let exponent = if b == 0.0 {
        0
    } else {
        b.abs().log10().floor() as i32
    };
    let unit = 10f64.powi(exponent - significant + 1).max(10f64.powi(-16));
    let ulp = f64::EPSILON * a.abs().max(b.abs());
    (a - b).abs() <= 0.5 * unit + 4.0 * ulp
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

/// Why the converter may omit group `code` of an entity the export writes.
fn group_reason(kind: &str, code: u16) -> Option<&'static str> {
    let _ = kind;
    match code {
        1000..=1071 => Some("extended data is not decoded for entities"),
        92 | 310 => Some("the entity's proxy graphics are not decoded"),
        420 | 430 => Some("a colour book colour's RGB and name are in its AcDbColor object"),
        347 => Some("the material object is not written to OBJECTS yet"),
        _ => None,
    }
}

struct Outcome {
    tallies: BTreeMap<String, Tally>,
    reasons: BTreeMap<String, usize>,
    failures: Vec<String>,
}

fn entities<'d>(doc: &'d DxfDocument, section: &str) -> Vec<&'d DxfRecord> {
    doc.records
        .iter()
        .filter(|r| doc.section_name(r) == section)
        .collect()
}

/// A form of a supported kind that the writer leaves out, told from the export's
/// own record: an embedded object (101), an MTEXT with a background fill (90, 441)
/// or columns (75), a POLYLINE that is a 3D polygon mesh (70 bit 16), an LWPOLYLINE
/// with PLINEGEN (70 bit 128).
fn left_out(r: &DxfRecord) -> bool {
    let flag = r.first(70).and_then(DxfValue::as_i64).unwrap_or(0);
    r.groups.iter().any(|g| g.code == 101)
        || match r.kind.as_str() {
            "MTEXT" => r.groups.iter().any(|g| matches!(g.code, 90 | 441 | 75)),
            "POLYLINE" => flag & 16 != 0,
            "LWPOLYLINE" => flag & 128 != 0,
            _ => false,
        }
}

/// The `INSERT`s of `doc` and those whose block (group 2, compared without regard
/// to case) is not a `BLOCK` of its `BLOCKS` section. `drop_one` first removes the
/// first named block, the negative run.
fn block_references(doc: &DxfDocument, drop_one: bool) -> (usize, Vec<String>) {
    let lower = |r: &DxfRecord| {
        r.first(2)
            .and_then(DxfValue::as_str)
            .map(str::to_ascii_lowercase)
    };
    let mut names: BTreeSet<String> = doc
        .records
        .iter()
        .filter(|r| r.kind == "BLOCK" && doc.section_name(r) == "BLOCKS")
        .filter_map(lower)
        .collect();
    if drop_one {
        if let Some(n) = names.iter().find(|n| !n.starts_with('*')).cloned() {
            names.remove(&n);
        }
    }
    let (mut checked, mut bad) = (0, Vec::new());
    for r in doc.records.iter().filter(|r| r.kind == "INSERT") {
        checked += 1;
        match lower(r) {
            Some(n) if names.contains(&n) => {}
            other => bad.push(format!(
                "INSERT {:X} names block {:?}, which is not written",
                r.handle.unwrap_or(0),
                other
            )),
        }
    }
    (checked, bad)
}

#[derive(Default)]
struct Gate {
    runs: usize,
    blocks_left_out: usize,
    inserts_left_out: usize,
    inserts: usize,
    failures: Vec<String>,
}

/// Build the entities of `path` once per layer, with that layer out of the tables,
/// and check every `INSERT` built names a block built.
fn gate_run(path: &Path) -> Result<Gate, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let file = DwgFile::from_bytes(bytes).map_err(|e| format!("open: {e}"))?;
    let mut g = Gate::default();
    let Some(target) = dxf_version(file.version()) else {
        return Ok(g);
    };
    if !dwg::dxf_entities::writes(file.version(), target) {
        return Ok(g);
    }
    let header = file
        .header_vars_strict()
        .ok_or("no AcDb:Header")?
        .map_err(|e| format!("header: {e}"))?;
    let none = HashSet::new();
    let tables = dwg::dxf_tables::build(&file, target, &header, &none, None);
    let whole = dwg::dxf_entities::build(&file, target, &header, &tables, &none);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    let layers: Vec<String> = tables
        .iter()
        .filter(|t| t.name == "LAYER")
        .flat_map(|t| t.records.iter().map(|r| r.name.clone()))
        .collect();
    let refused = "INSERT (its block is not written)";
    for layer in layers {
        let mut cut = tables.clone();
        for t in cut.iter_mut().filter(|t| t.name == "LAYER") {
            t.records.retain(|r| r.name != layer);
        }
        let e = dwg::dxf_entities::build(&file, target, &header, &cut, &none);
        let text = |groups: &[HeaderGroup]| {
            groups.iter().find_map(|g| match g {
                HeaderGroup::Text(2, t) => Some(t.to_ascii_lowercase()),
                _ => None,
            })
        };
        let blocks: BTreeSet<String> = e
            .blocks
            .iter()
            .filter_map(|b| text(&b.begin.groups))
            .collect();
        g.runs += 1;
        g.blocks_left_out += whole.blocks.len() - e.blocks.len().min(whole.blocks.len());
        g.inserts_left_out += [&e.skipped, &e.block_skipped]
            .iter()
            .map(|m| m.get(refused).copied().unwrap_or(0))
            .sum::<usize>();
        for r in e.records.iter().filter(|r| r.kind == "INSERT") {
            g.inserts += 1;
            let n = text(&r.groups);
            if !n.as_ref().is_some_and(|n| blocks.contains(n)) {
                g.failures.push(format!(
                    "{name} without layer {layer:?}: INSERT {:X} names block {n:?}, which is not written",
                    r.handle
                ));
            }
        }
    }
    Ok(g)
}

/// Convert `path` to every release the writer writes it to and check each
/// output's `INSERT`s: the conversions made, the `INSERT`s checked and the
/// failures.
fn reference_run(path: &Path) -> Result<(usize, usize, Vec<String>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let file = DwgFile::from_bytes(bytes).map_err(|e| format!("open: {e}"))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    let drop_one = std::env::var_os("PROBE_DXF_ENTITIES_DROP_BLOCK").is_some();
    let (mut conversions, mut inserts, mut failures) = (0, 0, Vec::new());
    for target in [
        DxfVersion::R2000,
        DxfVersion::R2004,
        DxfVersion::R2007,
        DxfVersion::R2010,
        DxfVersion::R2013,
        DxfVersion::R2018,
    ] {
        if !dwg::dxf_entities::writes(file.version(), target) {
            continue;
        }
        let out = dwg::dxf_convert::convert_dwg_to_dxf(&file, target)
            .map_err(|e| format!("convert to {target:?}: {e}"))?;
        let doc = DxfDocument::parse(out.as_bytes()).map_err(|e| format!("read back: {e}"))?;
        let (checked, bad) = block_references(&doc, drop_one);
        conversions += 1;
        inserts += checked;
        failures.extend(
            bad.into_iter()
                .map(|b| format!("{name} to {target:?}: {b}")),
        );
    }
    Ok((conversions, inserts, failures))
}

fn probe_file(
    path: &Path,
    twins: &BTreeMap<String, PathBuf>,
    section: &str,
) -> Result<Option<Outcome>, String> {
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
    };
    let our_recs = entities(&ours, section);
    let their_recs = entities(&theirs, section);
    let by_handle: BTreeMap<u64, &DxfRecord> = their_recs
        .iter()
        .filter_map(|r| r.handle.map(|h| (h, *r)))
        .collect();
    let supported = kinds(target);
    let their_index: BTreeMap<u64, usize> = their_recs
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.handle.map(|h| (h, i)))
        .collect();
    // A block the export registers again under another handle (the tables probe
    // pairs its record by name): its BLOCK, its entities and its ENDBLK have new
    // handles too, so they cannot be paired by handle.
    let (mut our_skip, mut their_skip): (BTreeSet<u64>, BTreeSet<u64>) = Default::default();
    if section == "BLOCKS" {
        let lower = |r: &DxfRecord| {
            r.first(2)
                .and_then(DxfValue::as_str)
                .map(str::to_ascii_lowercase)
        };
        let our_handles: BTreeSet<u64> = our_recs.iter().filter_map(|r| r.handle).collect();
        let mut our_blocks: BTreeSet<u64> = BTreeSet::new();
        let mut their_blocks: BTreeSet<u64> = BTreeSet::new();
        for a in our_recs.iter().filter(|r| {
            r.kind == "BLOCK"
                && r.handle
                    .is_some_and(|h| !by_handle.contains_key(&h) && h < seed)
        }) {
            let twin = their_recs.iter().find(|b| {
                b.kind == "BLOCK"
                    && b.handle.is_some_and(|h| !our_handles.contains(&h))
                    && lower(b) == lower(a)
            });
            if let (Some(b), Some(ao)) = (twin, a.owner) {
                our_blocks.insert(ao);
                their_blocks.extend(b.owner);
            }
        }
        for (recs, blocks, skip) in [
            (&our_recs, &our_blocks, &mut our_skip),
            (&their_recs, &their_blocks, &mut their_skip),
        ] {
            for r in recs.iter() {
                if let (Some(h), Some(ow)) = (r.handle, r.owner) {
                    if blocks.contains(&ow) || skip.contains(&ow) {
                        skip.insert(h);
                    }
                }
            }
        }
    }
    let mut last_index = 0usize;
    let mut paired: BTreeSet<u64> = BTreeSet::new();
    for r in &our_recs {
        let kind = r.kind.clone();
        let t = o.tallies.entry(kind.clone()).or_default();
        t.written += 1;
        let Some(h) = r.handle else {
            o.failures
                .push(format!("{name} {kind}: written without a handle"));
            continue;
        };
        if our_skip.contains(&h) {
            *o.reasons
                .entry(format!(
                    "{kind} of a block the export registered again under another handle"
                ))
                .or_default() += 1;
            continue;
        }
        let Some(want) = by_handle.get(&h).copied() else {
            o.failures.push(format!(
                "{name} {kind} {h:X}: written but AutoCAD's ENTITIES has no such record"
            ));
            continue;
        };
        if want.kind != r.kind {
            o.failures.push(format!(
                "{name} {h:X}: written as {kind} but AutoCAD wrote {}",
                want.kind
            ));
            continue;
        }
        paired.insert(h);
        t.paired += 1;
        let at = their_index[&h];
        if at < last_index {
            o.failures.push(format!(
                "{name} {kind} {h:X}: written before a record AutoCAD's {section} has ahead of it"
            ));
        }
        last_index = at;
        if r.owner != want.owner {
            o.failures.push(format!(
                "{name} {kind} {h:X}: owner {:?} but AutoCAD wrote {:?}",
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
                Some(_)
                    if bname == "ACAD_XDICTIONARY"
                        && theirs_in.iter().any(|(c, v)| {
                            *c == 360 && v.as_handle().is_some_and(|x| x >= seed)
                        }) =>
                {
                    *o.reasons
                        .entry(format!(
                            "{kind} extension dictionary the export made itself (a handle at or above the DWG's $HANDSEED)"
                        ))
                        .or_default() += 1;
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
                    let why = match bname.as_str() {
                        "ACAD_REACTORS" => {
                            "the entities that react are not all written with their handles"
                        }
                        "ACAD_XDICTIONARY" => {
                            "the extension dictionary object is not written to OBJECTS"
                        }
                        _ => "",
                    };
                    if why.is_empty() {
                        o.failures.push(format!(
                            "{name} {kind} {h:X}: AutoCAD writes {{{bname}}} and the record omits it with no stated reason"
                        ));
                    } else {
                        *o.reasons
                            .entry(format!("{kind} bracket {bname} ({why})"))
                            .or_default() += 1;
                    }
                }
            }
        }
        for bname in our_brackets.keys() {
            if !their_brackets.contains_key(bname) {
                t.extra += 1;
                o.failures.push(format!(
                    "{name} {kind} {h:X}: bracket {bname} written but absent from the export"
                ));
            }
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
                    "{name} {kind} {h:X}: group {code} written but absent from the export"
                ));
            } else if a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| same(x, y, digits)) {
                t.agree += 1;
            } else if kind == "BLOCK"
                && matches!(code, 2 | 3)
                && matches!(
                    file.version(),
                    Version::R2000 | Version::R2004 | Version::R2007
                )
                && b.first().and_then(|v| v.as_str()).is_some_and(|n| {
                    n.strip_prefix("*U")
                        .is_some_and(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()))
                })
                && !a
                    .first()
                    .and_then(|v| v.as_str())
                    .is_some_and(|n| n.starts_with('*'))
            {
                *o.reasons
                    .entry(
                        "relation: BLOCK name (AutoCAD loads this release's dynamic block as an anonymous *U block)"
                            .into(),
                    )
                    .or_default() += 1;
            } else {
                t.disagree += 1;
                o.failures.push(format!(
                    "{name} {kind} {h:X}: group {code} is {} but AutoCAD wrote {}",
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
            match group_reason(&kind, *code) {
                Some(why) => {
                    *o.reasons
                        .entry(format!("{kind} group {code} ({why})"))
                        .or_default() += 1
                }
                None => o.failures.push(format!(
                    "{name} {kind} {h:X}: AutoCAD writes group {code} and the record omits it with no stated reason"
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
                .filter(|c| shared.contains(c) && ![100, 330, 5].contains(c))
                .collect()
        };
        if in_order(r) != in_order(want) {
            o.failures.push(format!(
                "{name} {kind} {h:X}: groups {:?} but AutoCAD wrote them {:?}",
                in_order(r),
                in_order(want)
            ));
        }
    }
    for r in &their_recs {
        let Some(h) = r.handle else { continue };
        if paired.contains(&h) || h >= seed || their_skip.contains(&h) {
            continue;
        }
        let t = o.tallies.entry(r.kind.clone()).or_default();
        t.unwritten += 1;
        // A child is written with its parent only.
        let parent_written = !matches!(r.kind.as_str(), "ATTRIB" | "VERTEX" | "SEQEND")
            || r.owner
                .and_then(|o| by_handle.get(&o))
                .is_none_or(|p| supported.contains(&p.kind.as_str()) && !left_out(p));
        if supported.contains(&r.kind.as_str()) && !left_out(r) && parent_written {
            o.failures.push(format!(
                "{name} {} {h:X}: in the export, not written, though the converter writes {}",
                r.kind, r.kind
            ));
        }
    }
    println!(
        "{name:28} {}  {} entities written, {} paired",
        file.version(),
        o.tallies.values().map(|t| t.written).sum::<usize>(),
        o.tallies.values().map(|t| t.paired).sum::<usize>()
    );
    Ok(Some(o))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let section = args
        .iter()
        .find_map(|a| a.strip_prefix("--section="))
        .unwrap_or("ENTITIES")
        .to_string();
    let mut paths: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .collect();
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
    let mut by: BTreeMap<String, Tally> = BTreeMap::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    let (mut compared, mut skipped) = (0usize, 0usize);
    for path in &dwgs {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        match probe_file(path, &twins, &section) {
            Ok(Some(o)) => {
                compared += 1;
                failures.extend(o.failures);
                for (k, n) in o.reasons {
                    *reasons.entry(k).or_default() += n;
                }
                for (kind, t) in o.tallies {
                    let a = by.entry(kind).or_default();
                    a.written += t.written;
                    a.paired += t.paired;
                    a.groups += t.groups;
                    a.agree += t.agree;
                    a.disagree += t.disagree;
                    a.extra += t.extra;
                    a.omitted_groups += t.omitted_groups;
                    a.unwritten += t.unwritten;
                }
            }
            Ok(None) => skipped += 1,
            Err(e) => {
                println!("{name:28} FAILED: {e}");
                failures.push(format!("{name}: {e}"));
            }
        }
    }
    let (mut conversions, mut inserts, mut unresolved) = (0usize, 0usize, 0usize);
    let mut gate = Gate::default();
    if section == "BLOCKS" {
        for path in &dwgs {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            match gate_run(path) {
                Ok(g) => {
                    gate.runs += g.runs;
                    gate.blocks_left_out += g.blocks_left_out;
                    gate.inserts_left_out += g.inserts_left_out;
                    gate.inserts += g.inserts;
                    gate.failures.extend(g.failures);
                }
                Err(e) => {
                    println!("{name:28} FAILED: {e}");
                    failures.push(format!("{name}: {e}"));
                }
            }
            match reference_run(path) {
                Ok((c, i, f)) => {
                    conversions += c;
                    inserts += i;
                    unresolved += f.len();
                    failures.extend(f);
                }
                Err(e) => {
                    println!("{name:28} FAILED: {e}");
                    failures.push(format!("{name}: {e}"));
                }
            }
        }
    }
    println!();
    println!(
        "{:14} {:>8} {:>7} {:>7} {:>6} {:>9} {:>6} {:>7} {:>10}",
        "kind",
        "written",
        "paired",
        "groups",
        "agree",
        "disagree",
        "extra",
        "om.grp",
        "not written"
    );
    for (kind, t) in &by {
        println!(
            "{kind:14} {:>8} {:>7} {:>7} {:>6} {:>9} {:>6} {:>7} {:>10}",
            t.written,
            t.paired,
            t.groups,
            t.agree,
            t.disagree,
            t.extra,
            t.omitted_groups,
            t.unwritten
        );
    }
    println!("{skipped} drawings skipped (no twin or no AutoCAD export of the release)");
    if section == "BLOCKS" {
        if conversions == 0 {
            println!("NOT MEASURED: INSERT block references (no drawing converts to a release)");
        } else {
            println!(
                "INSERT block references: {inserts} INSERTs in {conversions} conversions, {unresolved} naming a block that is not written"
            );
        }
    }
    for (k, n) in &reasons {
        if k.starts_with("relation: ") {
            println!("{k} x{n}");
        } else {
            println!("omitted: {k} x{n}");
        }
    }
    if section == "BLOCKS" {
        if gate.blocks_left_out == 0 {
            println!("NOT MEASURED: INSERT gate (no layer removed took a block out)");
        } else {
            println!(
                "INSERT gate: {} builds, each without one layer: {} blocks left out, {} INSERTs of them left out, {} INSERTs written, {} naming a block that is not written",
                gate.runs,
                gate.blocks_left_out,
                gate.inserts_left_out,
                gate.inserts,
                gate.failures.len()
            );
        }
        failures.extend(std::mem::take(&mut gate.failures));
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
