//! Walking a corpus directory for the object probes: every drawing opened,
//! its object records decoded as `DwgFile::decoded_entities` does and kept
//! next to their handles, and AutoCAD's DXF export of the drawing beside it
//! when the corpus holds one.
//!
//! The DXF side is read as plain group pairs. A record is a `0` group and
//! the groups up to the next `0`; its handle is its first group 5 (or 105);
//! the groups inside a `102 {...}` / `102 }` bracket (reactors, extension
//! dictionary) are dropped, as `dwg::dxf_reader` treats them.
//!
//! Each probe includes this file and uses part of it, so the unused part is
//! not a warning.

#![allow(dead_code)]

use dwg::classes::ClassMap;
use dwg::entities::{DecodedEntity, decode_from_raw, decode_from_raw_with_class_map};
use dwg::object::RawObject;
use dwg::{DwgFile, ObjectType, Version};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub struct Group {
    pub code: u16,
    pub value: String,
}

impl Group {
    pub fn int(&self) -> Option<i64> {
        self.value.trim().parse().ok()
    }

    pub fn real(&self) -> Option<f64> {
        self.value.trim().parse().ok()
    }
}

pub struct Record {
    pub kind: String,
    pub handle: Option<u64>,
    /// How many groups 330 the `{ACAD_REACTORS` bracket holds.
    pub reactors: usize,
    pub groups: Vec<Group>,
}

impl Record {
    pub fn first(&self, code: u16) -> Option<&Group> {
        self.groups.iter().find(|g| g.code == code)
    }
}

pub struct Twin {
    records: Vec<Record>,
    by_handle: HashMap<u64, usize>,
}

impl Twin {
    pub fn parse(bytes: &[u8]) -> Twin {
        let text = String::from_utf8_lossy(bytes);
        let mut lines = text.lines();
        let mut records: Vec<Record> = Vec::new();
        let mut depth = 0usize;
        let mut in_reactors = false;
        while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
            let code: u16 = code.trim().parse().unwrap_or(u16::MAX);
            if code == 0 {
                depth = 0;
                in_reactors = false;
                records.push(Record {
                    kind: value.trim().to_string(),
                    handle: None,
                    reactors: 0,
                    groups: Vec::new(),
                });
                continue;
            }
            let Some(rec) = records.last_mut() else {
                continue;
            };
            if code == 102 {
                if value.trim_start().starts_with('{') {
                    depth += 1;
                    in_reactors = value.trim_start().starts_with("{ACAD_REACTORS");
                } else {
                    depth = depth.saturating_sub(1);
                    in_reactors = false;
                }
                continue;
            }
            if depth > 0 {
                if in_reactors && code == 330 {
                    rec.reactors += 1;
                }
                continue;
            }
            if (code == 5 || code == 105) && rec.handle.is_none() {
                rec.handle = u64::from_str_radix(value.trim(), 16).ok();
            }
            rec.groups.push(Group {
                code,
                value: value.to_string(),
            });
        }
        let mut by_handle = HashMap::new();
        for (i, r) in records.iter().enumerate() {
            if let Some(h) = r.handle {
                by_handle.entry(h).or_insert(i);
            }
        }
        Twin { records, by_handle }
    }

    /// The record whose handle is `handle`.
    pub fn record(&self, handle: u64) -> Option<&Record> {
        self.by_handle.get(&handle).map(|&i| &self.records[i])
    }

    /// Every record of DXF type `kind`, in file order.
    pub fn records_of(&self, kind: &str) -> Vec<&Record> {
        self.records.iter().filter(|r| r.kind == kind).collect()
    }
}

/// How a DWG record finds its DXF partner.
pub enum Pairing {
    /// The DXF record with the same handle.
    Handle,
    /// The DXF record with the same handle; failing that, the DXF records of
    /// the type that no DWG record shares a handle with, taken in file order
    /// against the DWG records that find none. For the objects of a table
    /// AutoCAD created while saving, whose handles differ between the DWG and
    /// the DXF of the same drawing.
    HandleThenOrder,
    /// The DXF record with the same handle. An object the export omits while
    /// holding others of its kind (AutoCAD's R2000 and R2004 exports drop some
    /// objects of a class and keep one) is checked by its boundary alone, and
    /// the line says so, instead of failing for want of a partner.
    HandleOrAbsent,
    /// The one record of the DXF type in the twin, for an object AutoCAD
    /// writes afresh on every save, so that the two files never share a
    /// handle. A twin with none or several has no partner for it.
    OnlyOfKind,
}

pub struct Drawing {
    pub name: String,
    pub version: Version,
    pub twin: Option<Twin>,
    objects: Vec<RawObject>,
    classes: Option<ClassMap>,
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() && depth < 4 {
            collect(&path, depth + 1, out)?;
        } else if path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("dwg"))
        {
            out.push(path);
        }
    }
    Ok(())
}

/// Every `.dwg` under `dir`, sorted, with its DXF twin: `<stem>.dxf` or the
/// ACadSharp samples' `<stem>_ascii.dxf`. A drawing with no twin (R14: the
/// nextgis saves were exported only to R12 DXF) is returned without one.
pub fn drawings(dir: &Path) -> Result<Vec<Drawing>, String> {
    let mut paths = Vec::new();
    collect(dir, 0, &mut paths).map_err(|e| format!("{}: {e}", dir.display()))?;
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let file = DwgFile::open(&path).map_err(|e| format!("{name}: open: {e}"))?;
        let objects = match file.all_objects() {
            Some(Ok(o)) => o,
            Some(Err(e)) => return Err(format!("{name}: object walk: {e}")),
            None => return Err(format!("{name}: no object stream")),
        };
        let stem = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let parent = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let twin = [format!("{stem}.dxf"), format!("{stem}_ascii.dxf")]
            .iter()
            .map(|n| parent.join(n))
            .find(|p| p.exists())
            .map(|p| {
                std::fs::read(&p)
                    .map(|bytes| Twin::parse(&bytes))
                    .map_err(|e| format!("{name}: read DXF: {e}"))
            })
            .transpose()?;
        out.push(Drawing {
            name,
            version: file.version(),
            twin,
            objects,
            classes: file.class_map().and_then(Result::ok),
        });
    }
    Ok(out)
}

impl Drawing {
    /// The decode of every record of the custom class `class`, by handle.
    pub fn class_records(&self, class: &str) -> Vec<(u64, DecodedEntity)> {
        let Some(map) = self.classes.as_ref() else {
            return Vec::new();
        };
        self.objects
            .iter()
            .filter_map(|raw| {
                let ObjectType::Custom(code) = raw.kind else {
                    return None;
                };
                let def = map.by_type_code(code)?;
                (def.dxf_class_name == class).then(|| {
                    (
                        raw.handle.value,
                        decode_from_raw_with_class_map(raw, self.version, map, code),
                    )
                })
            })
            .collect()
    }

    /// The decode of every record of the fixed type `kind`, by handle.
    pub fn kind_records(&self, kind: ObjectType) -> Vec<(u64, DecodedEntity)> {
        self.objects
            .iter()
            .filter(|raw| raw.kind == kind)
            .map(|raw| (raw.handle.value, decode_from_raw(raw, self.version)))
            .collect()
    }
}

/// Counts of how a decoder's values stood against the DXF export.
#[derive(Default)]
pub struct Tally {
    pub compared: usize,
    pub differ: usize,
    pub omitted: usize,
}

impl Tally {
    /// Compare an integer; `None` means the DXF omits the group.
    pub fn int(&mut self, what: &str, ours: i64, theirs: Option<i64>) {
        match theirs {
            Some(t) if t == ours => self.compared += 1,
            Some(t) => {
                self.compared += 1;
                self.differ += 1;
                println!("    {what}: dwg {ours} dxf {t}");
            }
            None => self.omitted += 1,
        }
    }

    /// Compare a real to the 1e-12 relative tolerance of a 16-digit DXF.
    pub fn real(&mut self, what: &str, ours: f64, theirs: Option<f64>) {
        match theirs {
            Some(t) if (ours - t).abs() <= 1e-12 * ours.abs().max(t.abs()).max(1.0) => {
                self.compared += 1;
            }
            Some(t) => {
                self.compared += 1;
                self.differ += 1;
                println!("    {what}: dwg {ours} dxf {t}");
            }
            None => self.omitted += 1,
        }
    }

    /// Compare text.
    pub fn text(&mut self, what: &str, ours: &str, theirs: Option<&str>) {
        match theirs {
            Some(t) if t == ours => self.compared += 1,
            Some(t) => {
                self.compared += 1;
                self.differ += 1;
                println!("    {what}: dwg {ours:?} dxf {t:?}");
            }
            None => self.omitted += 1,
        }
    }
}

/// What one probe measures: the records of `class` (a custom class) and of
/// `kind` (a fixed type), the DXF record type they pair with (empty when the
/// DXF exports hold none, so that only the boundary is checked), and the
/// releases that must decode.
pub struct Probe<'a> {
    pub tag: &'a str,
    pub class: Option<&'a str>,
    pub kind: Option<ObjectType>,
    pub dxf_kind: &'a str,
    pub decodes_on: fn(Version) -> bool,
    pub note: &'a str,
}

/// Run `p` over the corpus directory given as the first argument and print
/// each record, a table by release and a total. `compare` gets the decode
/// and the DXF record (`None` on a drawing with no twin), tallies the
/// values and says whether the decode is the variant the probe expects.
/// The exit status is non-zero when a record that must decode errors or is
/// unhandled, a twin lacks the record, a value differs, or none was compared.
pub fn run(
    p: &Probe,
    compare: impl Fn(&DecodedEntity, Option<&Record>, &mut Tally) -> bool,
) -> ExitCode {
    run_with_twin(p, Pairing::Handle, |d, r, _, t| compare(d, r, t))
}

/// [`run`], with the record's partner found by `pairing` and the whole twin
/// handed to `compare`, for a record whose values also live in a second DXF
/// object.
pub fn run_with_twin(
    p: &Probe,
    pairing: Pairing,
    compare: impl Fn(&DecodedEntity, Option<&Record>, Option<&Twin>, &mut Tally) -> bool,
) -> ExitCode {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!(
            "usage: {} <corpus-dir>",
            std::env::args().next().unwrap_or_default()
        );
        return ExitCode::FAILURE;
    };
    let drawings = match drawings(Path::new(&dir)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let tag = p.tag;
    let mut per_release: BTreeMap<String, [usize; 5]> = BTreeMap::new();
    let mut total = Tally::default();
    let mut failed = false;
    for d in &drawings {
        let release = format!("{:?}", d.version);
        let mut records = p.class.map_or_else(Vec::new, |c| d.class_records(c));
        records.extend(p.kind.map_or_else(Vec::new, |k| d.kind_records(k)));
        let mut by_order: HashMap<u64, &Record> = HashMap::new();
        if let (Pairing::HandleThenOrder, Some(t), false) =
            (&pairing, &d.twin, p.dxf_kind.is_empty())
        {
            let mut spare = t
                .records_of(p.dxf_kind)
                .into_iter()
                .filter(|r| !records.iter().any(|(h, _)| r.handle == Some(*h)));
            for (h, _) in &records {
                if t.record(*h).is_none_or(|r| r.kind != p.dxf_kind) {
                    if let Some(r) = spare.next() {
                        by_order.insert(*h, r);
                    }
                }
            }
        }
        for (handle, decoded) in records {
            let row = per_release.entry(release.clone()).or_default();
            row[0] += 1;
            let name = &d.name;
            match &decoded {
                DecodedEntity::Error { message, .. } => {
                    failed = true;
                    println!("{name} ({release}) {tag} 0x{handle:X} ERROR {message}");
                }
                DecodedEntity::Unhandled { .. } if !(p.decodes_on)(d.version) => {
                    row[2] += 1;
                    println!("{name} ({release}) {tag} 0x{handle:X} not decoded on this release");
                }
                DecodedEntity::Unhandled { .. } => {
                    failed = true;
                    println!("{name} ({release}) {tag} 0x{handle:X} UNHANDLED");
                }
                _ => {
                    let mut tally = Tally::default();
                    let rec = if p.dxf_kind.is_empty() {
                        None
                    } else {
                        d.twin.as_ref().map(|t| match pairing {
                            Pairing::Handle | Pairing::HandleOrAbsent => {
                                t.record(handle).filter(|r| r.kind == p.dxf_kind)
                            }
                            Pairing::HandleThenOrder => t
                                .record(handle)
                                .filter(|r| r.kind == p.dxf_kind)
                                .or_else(|| by_order.get(&handle).copied()),
                            Pairing::OnlyOfKind => {
                                let all = t.records_of(p.dxf_kind);
                                (all.len() == 1).then(|| all[0])
                            }
                        })
                    };
                    // A release whose whole DXF export holds no record of this kind (AutoCAD's
                    // R2000/R2004 exports omit some object classes entirely) has no twin for it:
                    // the record is compared by its boundary only, as on R14. A missing record
                    // where the export does hold others of the kind still fails.
                    let rec = match rec {
                        Some(None) if matches!(pairing, Pairing::HandleOrAbsent) => {
                            println!(
                                "{name} ({release}) {tag} 0x{handle:X} closes; this release's DXF \
                                 export omits this {}, so it is checked by its boundary only",
                                p.dxf_kind
                            );
                            None
                        }
                        Some(None)
                            if d.twin
                                .as_ref()
                                .is_some_and(|t| t.records_of(p.dxf_kind).is_empty()) =>
                        {
                            println!(
                                "{name} ({release}) {tag} 0x{handle:X} closes; this release's DXF \
                                 export holds no {} at all, so it is checked by its boundary only",
                                p.dxf_kind
                            );
                            None
                        }
                        other => other,
                    };
                    if matches!(rec, Some(None)) {
                        failed = true;
                        println!(
                            "{name} ({release}) {tag} 0x{handle:X} closes; the DXF has no {} \
                             to pair it with",
                            p.dxf_kind
                        );
                        continue;
                    }
                    if !compare(&decoded, rec.flatten(), d.twin.as_ref(), &mut tally) {
                        failed = true;
                        println!("{name} ({release}) {tag} 0x{handle:X} decoded as another type");
                        continue;
                    }
                    row[1] += 1;
                    if rec.is_some() {
                        println!(
                            "{name} ({release}) {tag} 0x{handle:X} closes; DXF: {} compared, \
                             {} differ, {} omitted",
                            tally.compared, tally.differ, tally.omitted
                        );
                    } else {
                        println!("{name} ({release}) {tag} 0x{handle:X} closes; no DXF twin");
                    }
                    row[3] += tally.compared;
                    row[4] += tally.differ;
                    total.compared += tally.compared;
                    total.differ += tally.differ;
                    total.omitted += tally.omitted;
                }
            }
        }
    }
    println!();
    println!("release   records  close  not decoded  compared  differ");
    for (release, r) in &per_release {
        println!(
            "{release:<8}  {:>7}  {:>5}  {:>11}  {:>8}  {:>6}",
            r[0], r[1], r[2], r[3], r[4]
        );
    }
    println!(
        "total: {} values compared, {} differ, {} omitted by the DXF{}",
        total.compared, total.differ, total.omitted, p.note
    );
    if failed || total.differ > 0 || (total.compared == 0 && !p.dxf_kind.is_empty()) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
