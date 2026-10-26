//! The DXF value oracle (#99): every DWG record of the corpus paired by handle
//! with the record AutoCAD's own DXF export of the same drawing holds (#101),
//! and every decoded field compared with the DXF group that carries it
//! (#102).
//!
//! What it proves: which records the two files share and which one side
//! lacks, by release and type; then, field by field, whether the value the
//! decoder returns equals the value AutoCAD wrote. A record that closes on
//! its data-stream boundary proves only that the widths add up; this is the
//! check that says the values are right. The scorers, their sources and the
//! real-number tolerance are documented in `scorers.rs`.
//!
//! How to verify:
//!
//! ```text
//! cargo run --release --features dxf-reader --example dxf_oracle -- <corpus-dir> \
//!     [--details] [--tsv <out.tsv>] [--baseline <in.tsv>] [--summary <out.md>]
//! ```
//!
//! `<corpus-dir>` is searched (subdirectories included, as the corpus fetch
//! of #225 lays sources out) for `.dwg` files with a DXF twin:
//! `<name>.dxf`, `<name>_ascii.dxf` (the ACadSharp samples), or for
//! nextgis's R14 saves, which were exported only to R12 DXF, `<name>_R12.dxf`.
//! A DWG with no twin is listed and skipped. The report prints, in order:
//! the pairing table and the one-sided records by release and type; one
//! matrix per decoded type (a row per field, a column per release); every
//! disagreement; the DXF groups no decoded field maps to; the paired records
//! no scorer covers; and a summary by spec section.
//!
//! - `--details` lists every one-sided record and every disagreement.
//! - `--tsv` writes one row per release, type, field and group with its
//!   equal / default / different / DWG-only counts.
//! - `--baseline` compares this run with such a TSV and exits non-zero if
//!   any row's agreeing count (equal plus default) fell, or a row the
//!   baseline holds is absent: the CI ratchet (#103). A change that renames,
//!   moves or removes a field row, or raises a count, commits the
//!   regenerated TSV in the same pull request, as the corpus baseline does.
//! - `--summary` writes the section summary as Markdown for a job summary.
//!
//! # What one side has and the other lacks
//!
//! Measured on the nextgis (`ff05d5a8`) and ACadSharp (`c30a97ce`) exports
//! (`--details` lists every record):
//!
//! - **Save-time objects.** Each save creates its own round-trip and
//!   variable objects and gives them fresh handles above everything else in
//!   the file: the `ACAD_XREC_ROUNDTRIP` dictionaries and their XRECORDs, the
//!   variable dictionary and its DICTIONARYVARs (`CANNOSCALE`, `CMLEADERSTYLE`,
//!   ...), the TABLESTYLE's CELLSTYLEMAP and, before R2010, two APPIDs. The
//!   DWG's set and the DXF's set therefore never share a handle. The report
//!   marks a one-sided record `save-time` when its handle is above every
//!   handle the two files share; on the nextgis drawings every one-sided
//!   record from R2000 on is save-time except VP_ENT_HDR_CTRL.
//! - **Solid history on R2000 and R2004.** The ACadSharp saves hold
//!   ACAD_EVALUATION_GRAPH and the ACSH_* history objects of their 3D solid in
//!   the DWG; the R2000 and R2004 exports omit them (from R2007 they pair).
//! - **VP_ENT_HDR_CTRL** (the viewport-entity control, §20.4.69) is in the
//!   R14 and R2000 DWGs and has no DXF record; the R2004 and later DWGs do
//!   not hold one.
//! - **R14 against its R12 export.** R12 DXF has no OBJECTS section, no
//!   BLOCK_RECORD table, no CLASSES, no handles on TABLE records, table
//!   entries or BLOCK, no LWPOLYLINE and no ELLIPSE. Only the two ENDBLKs and
//!   the drawing's own entity pair; an LWPOLYLINE or ELLIPSE pairs by handle
//!   with the POLYLINE AutoCAD converted it to (listed under "same handle,
//!   different type"), whose VERTEX and SEQEND records are DXF-only.

mod corpus;
mod pairing;
mod report;
mod scorers;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: dxf_oracle <corpus-dir> [--details] [--tsv <out.tsv>] \
     [--baseline <in.tsv>] [--summary <out.md>]";

fn main() -> ExitCode {
    let mut dir = None;
    let mut details = false;
    let mut tsv_out = None;
    let mut baseline = None;
    let mut summary = None;
    let mut args = std::env::args().skip(1);
    // A path option without its path, an unknown option or a second corpus
    // directory is a usage error: a ratchet asked for and silently skipped
    // would pass.
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--details" => details = true,
            "--tsv" | "--baseline" | "--summary" => {
                let Some(path) = args.next().filter(|v| !v.starts_with("--")) else {
                    eprintln!("{arg} needs a path\n{USAGE}");
                    return ExitCode::FAILURE;
                };
                let path = Some(PathBuf::from(path));
                match arg.as_str() {
                    "--tsv" => tsv_out = path,
                    "--baseline" => baseline = path,
                    _ => summary = path,
                }
            }
            a if a.starts_with("--") => {
                eprintln!("unknown option {a}\n{USAGE}");
                return ExitCode::FAILURE;
            }
            _ if dir.is_some() => {
                eprintln!("one corpus directory only\n{USAGE}");
                return ExitCode::FAILURE;
            }
            _ => dir = Some(PathBuf::from(arg)),
        }
    }
    let Some(dir) = dir else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let (pairs, alone) = match corpus::find_pairs(&dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cannot read {}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
    };
    for dwg in &alone {
        println!("no DXF twin, skipped: {}", dwg.display());
    }

    // Read before any output is written: `--tsv` may name the same file.
    let baseline = baseline.map(|path| {
        let text = std::fs::read_to_string(&path);
        (path, text)
    });

    let mut loaded = Vec::new();
    let mut failed = false;
    for p in &pairs {
        match corpus::load(p) {
            Ok(l) => loaded.push(l),
            Err(e) => {
                println!("LOAD FAILED {e}");
                failed = true;
            }
        }
    }

    print_pairing(&loaded);
    if details {
        print_unpaired(&loaded);
    }

    let scored = report::score_all(&loaded);
    report::print_fields(&scored);
    report::print_differences(&scored, if details { usize::MAX } else { 5 });
    report::print_gaps(&scored);
    report::print_sections(&scored);
    if let Some(path) = summary {
        let md = format!(
            "## DXF value oracle by spec section\n\n{}",
            report::sections_markdown(&scored)
        );
        if let Err(e) = std::fs::write(&path, md) {
            eprintln!("cannot write {}: {e}", path.display());
            failed = true;
        }
    }
    let table = report::tsv(&scored);
    if let Some(path) = tsv_out {
        if let Err(e) = std::fs::write(&path, &table) {
            eprintln!("cannot write {}: {e}", path.display());
            failed = true;
        }
    }
    if let Some((path, text)) = baseline {
        match text {
            Ok(base) => {
                println!("\n== Ratchet against {}\n", path.display());
                match report::ratchet(&scored, &base) {
                    Ok(drops) => {
                        if drops.is_empty() {
                            println!("no agreeing count fell below the baseline");
                        }
                        for d in &drops {
                            println!("DROP {d}");
                        }
                        failed |= !drops.is_empty();
                    }
                    Err(e) => {
                        println!("BASELINE UNREADABLE {}: {e}", path.display());
                        failed = true;
                    }
                }
            }
            Err(e) => {
                eprintln!("cannot read baseline {}: {e}", path.display());
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn print_pairing(loaded: &[corpus::LoadedPair]) {
    println!("== Pairing by handle (#101)\n");
    println!(
        "{:<26} {:<7} {:>5} {:>5} {:>6} {:>7} {:>8} {:>8} {:>7}",
        "file", "release", "dwg", "dxf", "paired", "differs", "dwg-only", "dxf-only", "unkeyed"
    );
    // (release, side, type, origin) -> (records, drawings)
    type Key = (String, &'static str, String, &'static str);
    let mut one_sided: BTreeMap<Key, (usize, usize)> = BTreeMap::new();
    let mut drawings_per_release: BTreeMap<String, usize> = BTreeMap::new();
    let mut differs = Vec::new();
    let mut total = [0usize; 7];
    for l in loaded {
        let p = pairing::pair(l);
        let rel = corpus::release(l.version);
        *drawings_per_release.entry(rel.clone()).or_default() += 1;
        let keyed = l.dxf.records.len() - p.dxf_unkeyed;
        let row = [
            l.records.len(),
            keyed,
            p.paired.len(),
            p.type_differs.len(),
            p.dwg_only.len(),
            p.dxf_only.len(),
            p.dxf_unkeyed,
        ];
        println!(
            "{:<26} {:<7} {:>5} {:>5} {:>6} {:>7} {:>8} {:>8} {:>7}",
            l.name, rel, row[0], row[1], row[2], row[3], row[4], row[5], row[6]
        );
        for (t, v) in total.iter_mut().zip(row) {
            *t += v;
        }
        let cut = p.last_shared_handle();
        let origin = |h: u64| if h > cut { "save-time" } else { "" };
        let mut here: BTreeMap<Key, usize> = BTreeMap::new();
        for r in &p.dwg_only {
            let k = (rel.clone(), "dwg-only", r.label.clone(), origin(r.handle));
            *here.entry(k).or_default() += 1;
        }
        for r in &p.dxf_only {
            let h = r.handle.unwrap_or(0);
            let k = (rel.clone(), "dxf-only", corpus::record_key(r), origin(h));
            *here.entry(k).or_default() += 1;
        }
        for (k, n) in here {
            let e = one_sided.entry(k).or_default();
            e.0 += n;
            e.1 += 1;
        }
        for (d, x) in &p.type_differs {
            differs.push((
                l.name.clone(),
                d.handle,
                d.label.clone(),
                corpus::record_key(x),
            ));
        }
    }
    println!(
        "{:<26} {:<7} {:>5} {:>5} {:>6} {:>7} {:>8} {:>8} {:>7}",
        "TOTAL", "", total[0], total[1], total[2], total[3], total[4], total[5], total[6]
    );

    println!("\n== One-sided records, by release and type\n");
    println!("A record is save-time when its handle is above every handle the two files");
    println!("share: the save that wrote its file created it.\n");
    println!(
        "{:<7} {:<8} {:<24} {:<9} {:>7} {:>8}",
        "release", "side", "type", "origin", "records", "drawings"
    );
    for ((rel, side, kind, origin), (n, drawings)) in &one_sided {
        let of = drawings_per_release.get(rel).copied().unwrap_or(0);
        println!("{rel:<7} {side:<8} {kind:<24} {origin:<9} {n:>7} {drawings:>5}/{of}");
    }

    println!("\n== Same handle, different type\n");
    if differs.is_empty() {
        println!("(none)");
    }
    for (file, h, dwg, dxf) in &differs {
        println!("{file:<26} {h:>6X}  DWG {dwg:<24} DXF {dxf}");
    }
}

/// Every one-sided record, one line each, for investigation.
fn print_unpaired(loaded: &[corpus::LoadedPair]) {
    println!("\n== One-sided records, one by one\n");
    for l in loaded {
        let p = pairing::pair(l);
        for r in &p.dwg_only {
            let mut d = format!("{:?}", r.decoded);
            if let Some((cut, _)) = d.char_indices().nth(110) {
                d.truncate(cut);
            }
            println!(
                "{:<26} dwg-only {:>6X} {:<20} {d}",
                l.name, r.handle, r.label
            );
        }
        for r in &p.dxf_only {
            let groups: Vec<String> = r
                .top_level()
                .filter(|g| g.code != 100)
                .take(5)
                .map(|g| format!("{}={}", g.code, g.value))
                .collect();
            println!(
                "{:<26} dxf-only {:>6X} {:<20} {}",
                l.name,
                r.handle.unwrap_or(0),
                corpus::record_key(r),
                groups.join(" ")
            );
        }
    }
}
