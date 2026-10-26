//! Which record types do the R13 drawings hold, and does each one's decoder
//! close on its boundary there? (#119: "every §20.4 decoder's R13 branch
//! exercised or listed as unmeasured")
//!
//! # What this proves
//!
//! For every drawing named on the command line it walks the object map,
//! dispatches each record through the real decoder and counts, per record
//! type (a custom class under its DXF class name), the records, the
//! drawings that hold the type, and how many records decoded, errored and
//! had no decoder. A type that appears here has its R13 branch exercised on
//! real bytes, to the record's own boundary and no further (no AutoCAD DXF
//! of an R13 drawing exists); a §20.4 type that does not appear is
//! unmeasured on R13.
//!
//! ```sh
//! cargo run --release --example probe_r13_census -- a.dwg b.dwg
//! ```
//!
//! Exit status is non-zero when a drawing does not open or has no object
//! walk, so a drawing that went missing cannot pass as a smaller census.

use dwg::entities::DecodedEntity;
use dwg::{DwgFile, ObjectType};
use std::collections::BTreeMap;
use std::process::ExitCode;

#[derive(Default)]
struct Row {
    records: usize,
    files: usize,
    decoded: usize,
    errored: usize,
    unhandled: usize,
}

fn main() -> ExitCode {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: probe_r13_census <file.dwg>...");
        return ExitCode::FAILURE;
    }
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let mut opened = 0usize;
    let mut failed = false;
    for path in &paths {
        let file = match DwgFile::open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{path}: {e}");
                failed = true;
                continue;
            }
        };
        let (Some(Ok(raws)), Some(Ok((decoded, _)))) = (file.all_objects(), file.decoded_entities())
        else {
            eprintln!("{path}: no object walk");
            failed = true;
            continue;
        };
        let classes = file.class_map().and_then(Result::ok);
        opened += 1;
        let mut seen: BTreeMap<String, ()> = BTreeMap::new();
        for (raw, dec) in raws.iter().zip(decoded.iter()) {
            let label = match raw.kind {
                ObjectType::Custom(code) => classes
                    .as_ref()
                    .and_then(|c| c.by_type_code(code))
                    .map_or_else(|| format!("CUSTOM({code})"), |c| c.dxf_class_name.clone()),
                kind => format!("{kind}"),
            };
            let row = rows.entry(label.clone()).or_default();
            row.records += 1;
            if seen.insert(label, ()).is_none() {
                row.files += 1;
            }
            match dec {
                DecodedEntity::Error { .. } => row.errored += 1,
                DecodedEntity::Unhandled { .. } => row.unhandled += 1,
                _ => row.decoded += 1,
            }
        }
    }
    println!(
        "{:<36} {:>8} {:>7} {:>8} {:>8} {:>10}",
        "type", "records", "files", "decoded", "errored", "no decoder"
    );
    for (label, r) in &rows {
        println!(
            "{label:<36} {:>8} {:>7} {:>8} {:>8} {:>10}",
            r.records, r.files, r.decoded, r.errored, r.unhandled
        );
    }
    println!("{} types in {opened} of {} drawings", rows.len(), paths.len());
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
