//! The colour words of the common entity data against AutoCAD's own DXF export.
//!
//! # What this proves
//!
//! §2.11 stores an R2004+ entity colour as a `BS` whose high byte flags what
//! follows: a `BL` with the true colour (flag 0x80) and then a `BL` with the
//! transparency (flag 0x20). [`dwg::common_entity::read_common_entity_data`]
//! keeps them as `color_raw`, `color_rgb` and `color_alpha`. For each DWG that has
//! an AutoCAD DXF export of the same drawing, the probe pairs every entity with
//! the export's record of the same handle and compares:
//!
//! - the colour index (62, absent for BYLAYER = 256) with `color_raw & 0x1FF`,
//!   unless the export has a true colour (AutoCAD derives a nearest index for
//!   it that the DWG does not store);
//! - the true colour: the export's 420 with the low 24 bits of `color_rgb`, and
//!   a `color_rgb` that is a true colour (`0xC2......`) only where the export
//!   has a 420. A colour book colour (flag 0x40) is counted and left out: its
//!   RGB and name are in the AcDbColor object its handle names, not in the
//!   entity;
//! - the transparency: the export's 440 with `color_alpha`, and a `color_alpha`
//!   only where the export has a 440.
//!
//! The order of the two words only matters for an entity that carries both. The
//! probe counts those and checks them twice: in §2.11's order (RGB first), and
//! with the two words exchanged, which is what a reader that takes the
//! transparency first would have produced.
//!
//! Any disagreement exits 1. R13 and R14 drawings are skipped (no true colour or
//! transparency exists before R2004; their index is not measured here).
//!
//! # How to verify
//!
//! ```text
//! cargo run --release --features dxf-reader --example probe_entity_colour -- <file-or-dir> ...
//! ```
//!
//! Directories are searched recursively for `.dwg` and `.dxf` files; a DWG's twin
//! is the DXF with the same stem, or the stem plus `_ascii`. With no DWG that has
//! a twin it prints `NOT MEASURED` and exits 0.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dwg::dxf_reader::{DxfDocument, DxfRecord};
use dwg::{DwgFile, Version};

#[derive(Default, Clone)]
struct Tally {
    entities: usize,
    unpaired: usize,
    index: usize,
    index_agree: usize,
    true_colour: usize,
    true_colour_agree: usize,
    colour_book: usize,
    transparency: usize,
    transparency_agree: usize,
    both: usize,
    both_agree: usize,
    both_agree_exchanged: usize,
    failures: Vec<String>,
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

fn int(r: &DxfRecord, code: u16) -> Option<i64> {
    r.top_level()
        .find(|g| g.code == code)
        .and_then(|g| g.value.as_i64())
}

fn probe_file(
    path: &Path,
    twins: &BTreeMap<String, PathBuf>,
) -> Result<Option<(Version, Tally)>, String> {
    let Some(twin) = twins.get(&stem_key(path)) else {
        return Ok(None);
    };
    let file = DwgFile::open(path).map_err(|e| format!("open: {e}"))?;
    let version = file.version();
    if matches!(version, Version::R14) {
        return Ok(None);
    }
    let theirs = DxfDocument::parse(&std::fs::read(twin).map_err(|e| format!("read twin: {e}"))?)
        .map_err(|e| format!("twin: {e}"))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    let raws = match file.all_objects() {
        Some(Ok(r)) => r,
        Some(Err(e)) => return Err(format!("object walk: {e}")),
        None => return Ok(None),
    };
    let mut t = Tally::default();
    for raw in raws.iter().filter(|r| r.is_entity()) {
        let h = raw.handle.value;
        let Ok(mut cursor) = dwg::object::body_cursor(raw, version) else {
            continue;
        };
        let Ok(common) = dwg::common_entity::read_common_entity_data(&mut cursor, version) else {
            continue;
        };
        let Some(want) = theirs.record_by_handle(h) else {
            t.unpaired += 1;
            continue;
        };
        t.entities += 1;
        let want_index = int(want, 62).unwrap_or(256);
        let want_rgb = int(want, 420);
        let want_alpha = int(want, 440);

        if want_rgb.is_none() {
            t.index += 1;
            let have = i64::from(common.color_raw & 0x01FF);
            if have == want_index {
                t.index_agree += 1;
            } else {
                t.failures.push(format!(
                    "{name} {} {h:X}: colour index {have}, AutoCAD wrote {want_index}",
                    want.kind
                ));
            }
        }

        let book = version.is_r2004_plus() && common.color_raw >> 8 & 0x40 != 0;
        if book {
            t.colour_book += 1;
        }
        let have_rgb = common
            .color_rgb
            .filter(|w| w >> 24 == 0xC2)
            .map(|w| i64::from(w & 0xFF_FFFF));
        if !book && (want_rgb.is_some() || have_rgb.is_some()) {
            t.true_colour += 1;
            if have_rgb == want_rgb {
                t.true_colour_agree += 1;
            } else {
                t.failures.push(format!(
                    "{name} {} {h:X}: true colour {have_rgb:?}, AutoCAD wrote {want_rgb:?}",
                    want.kind
                ));
            }
        }

        let have_alpha = common.color_alpha.map(i64::from);
        if want_alpha.is_some() || have_alpha.is_some() {
            t.transparency += 1;
            if have_alpha == want_alpha {
                t.transparency_agree += 1;
            } else {
                t.failures.push(format!(
                    "{name} {} {h:X}: transparency {have_alpha:?}, AutoCAD wrote {want_alpha:?}",
                    want.kind
                ));
            }
        }

        if let (Some(rgb), Some(alpha)) = (common.color_rgb, common.color_alpha) {
            t.both += 1;
            if have_rgb == want_rgb && have_alpha == want_alpha {
                t.both_agree += 1;
            }
            let exchanged_rgb = Some(alpha)
                .filter(|w| w >> 24 == 0xC2)
                .map(|w| i64::from(w & 0xFF_FFFF));
            if exchanged_rgb == want_rgb && Some(i64::from(rgb)) == want_alpha {
                t.both_agree_exchanged += 1;
            }
        }
    }
    println!(
        "{name:28} {version}  {} entities paired, {} unpaired",
        t.entities, t.unpaired
    );
    Ok(Some((version, t)))
}

fn main() -> ExitCode {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        eprintln!("usage: probe_entity_colour <file-or-dir> ...");
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
    let mut by_release: BTreeMap<String, Tally> = BTreeMap::new();
    let mut errors = 0usize;
    let mut skipped = 0usize;
    for path in &dwgs {
        match probe_file(path, &twins) {
            Ok(Some((version, t))) => {
                let a = by_release.entry(version.to_string()).or_default();
                a.entities += t.entities;
                a.unpaired += t.unpaired;
                a.index += t.index;
                a.index_agree += t.index_agree;
                a.true_colour += t.true_colour;
                a.true_colour_agree += t.true_colour_agree;
                a.colour_book += t.colour_book;
                a.transparency += t.transparency;
                a.transparency_agree += t.transparency_agree;
                a.both += t.both;
                a.both_agree += t.both_agree;
                a.both_agree_exchanged += t.both_agree_exchanged;
                a.failures.extend(t.failures);
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
        "{:14} {:>8} {:>9} {:>6} {:>8} {:>6} {:>5} {:>8} {:>6} {:>5} {:>8} {:>10}",
        "release",
        "entities",
        "index",
        "agree",
        "true col",
        "agree",
        "book",
        "transp",
        "agree",
        "both",
        "agree",
        "exchanged"
    );
    let mut total = Tally::default();
    for (release, t) in &by_release {
        println!(
            "{release:14} {:>8} {:>9} {:>6} {:>8} {:>6} {:>5} {:>8} {:>6} {:>5} {:>8} {:>10}",
            t.entities,
            t.index,
            t.index_agree,
            t.true_colour,
            t.true_colour_agree,
            t.colour_book,
            t.transparency,
            t.transparency_agree,
            t.both,
            t.both_agree,
            t.both_agree_exchanged
        );
        total.entities += t.entities;
        total.index += t.index;
        total.true_colour += t.true_colour;
        total.transparency += t.transparency;
        total.both += t.both;
        total.both_agree += t.both_agree;
        total.both_agree_exchanged += t.both_agree_exchanged;
        total.failures.extend(t.failures.iter().cloned());
    }
    println!(
        "{skipped} drawings skipped (no twin, or R14), {} entities with a twin",
        total.entities
    );
    if by_release.is_empty() {
        println!("NOT MEASURED: no drawing had an AutoCAD export to compare");
        return ExitCode::SUCCESS;
    }
    for f in &total.failures {
        println!("FAILED {f}");
    }
    if errors > 0 || !total.failures.is_empty() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
