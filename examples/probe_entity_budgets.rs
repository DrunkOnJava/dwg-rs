//! How many data-stream bits does each entity record actually have?
//!
//! # What this proves
//!
//! Every record in an R2000-R2007 or R2010+ file carries a bit offset
//! at which its data fields *must* end — the `RL` object-data-size for
//! the older band, the first bit of the string stream (or the handle
//! stream minus the one `strings present` trailer bit) for the newer
//! one. The difference between that offset and the end of the common
//! entity preamble is the record's **budget**: the exact number of bits
//! its type-specific field list has to consume, no more and no less.
//!
//! `examples/probe_decode_errors.rs` prints the budget for records that
//! *error*. This probe prints it for **every** entity record, decoded,
//! unhandled or errored alike, which is what a type with no decoder at
//! all needs before one can be written:
//!
//! ```text
//! 0x0006 SeqEnd            handle=0x1E5  body@2029  data_end=2029  budget=0     strings=none
//! 0x000B Vertex3d          handle=0x1DF  body@1859  data_end=2051  budget=192   strings=none
//! ```
//!
//! A `budget` of 0 means the record's field list is empty — every bit
//! between the preamble and the boundary is already accounted for.
//!
//! ```sh
//! cargo run --release --example probe_entity_budgets -- samples/sample_AC1032.dwg
//! cargo run --release --example probe_entity_budgets -- samples/sample_AC1032.dwg 0x000B
//! ```
//!
//! The optional second argument filters by type code (`0x000B` or
//! `11`). With no filter the probe prints one line per record followed
//! by a per-type budget histogram.

use dwg::{DwgFile, RawObject, Version};
use std::collections::BTreeMap;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_entity_budgets <file.dwg> [type-code]");
        return ExitCode::FAILURE;
    };
    let filter = args.next().map(|s| {
        let s = s.trim().to_ascii_lowercase();
        let parsed = match s.strip_prefix("0x") {
            Some(hex) => u16::from_str_radix(hex, 16),
            None => s.parse::<u16>(),
        };
        parsed.expect("type code must be decimal or 0x-prefixed hex")
    });

    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("open failed ({path}): {e}");
            return ExitCode::FAILURE;
        }
    };
    let version = file.version();
    let Some(objects) = file.all_objects() else {
        eprintln!("{version} has no handle-driven object walk");
        return ExitCode::FAILURE;
    };
    let objects = match objects {
        Ok(o) => o,
        Err(e) => {
            eprintln!("object walk failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let class_map = file.class_map().and_then(std::result::Result::ok);

    println!("=== {path} ===");
    println!("version: {version}");
    println!();

    let mut histo: BTreeMap<String, BTreeMap<i64, usize>> = BTreeMap::new();
    for raw in &objects {
        if !raw.is_entity() {
            continue;
        }
        if filter.is_some_and(|f| f != raw.type_code) {
            continue;
        }
        let label = class_map
            .as_ref()
            .and_then(|m| m.by_type_code(raw.type_code))
            .map(|d| d.dxf_class_name.clone())
            .unwrap_or_else(|| format!("{}", raw.kind));
        let report = measure(raw, version);
        println!(
            "0x{:04X} {label:<24} handle=0x{:<6X} body@{:<7} data_end={:<7} budget={:<7} \
             strings={}",
            raw.type_code,
            raw.handle.value,
            render(report.body_start),
            render(report.data_end),
            render_i(report.budget),
            report.strings,
        );
        if let Some(budget) = report.budget {
            *histo
                .entry(format!("0x{:04X} {label}", raw.type_code))
                .or_default()
                .entry(budget)
                .or_default() += 1;
        }
    }

    println!();
    println!("=== budget histogram (bits between the preamble and the boundary) ===");
    for (label, buckets) in &histo {
        let total: usize = buckets.values().sum();
        let rendered = buckets
            .iter()
            .map(|(bits, n)| format!("{bits}×{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!("{label:<32} n={total:<4} {rendered}");
    }
    ExitCode::SUCCESS
}

struct Report {
    body_start: Option<usize>,
    data_end: Option<usize>,
    budget: Option<i64>,
    strings: String,
}

/// Boundary the entity's data fields must land on, per release band.
///
/// R2010+ reads it out of the string-stream trailer; R2000-R2007 from
/// the object prologue's `RL`. R13/R14 have neither.
fn entity_data_end(raw: &RawObject, version: Version) -> Option<usize> {
    if version.is_r2010_plus() {
        return dwg::string_stream::data_field_end(&raw.raw, version);
    }
    if version.is_r2007() {
        // The R2007 string stream is not locatable in this crate yet,
        // so the RL covers data + strings and is not the field boundary.
        return None;
    }
    raw.obj_size_bits.map(|b| b as usize)
}

fn measure(raw: &RawObject, version: Version) -> Report {
    let data_end = entity_data_end(raw, version);
    let strings = match version.is_r2010_plus() {
        true => match dwg::string_stream::locate(&raw.raw, version) {
            Some(s) => format!("{}bits", s.len_bits()),
            None => "none".to_string(),
        },
        false => "n/a".to_string(),
    };
    let body_start = dwg::object::body_cursor(raw, version)
        .ok()
        .and_then(|mut cursor| {
            dwg::common_entity::read_common_entity_data(&mut cursor, version).ok()?;
            Some(cursor.position_bits())
        });
    let budget = match (body_start, data_end) {
        (Some(b), Some(e)) => Some(e as i64 - b as i64),
        _ => None,
    };
    Report {
        body_start,
        data_end,
        budget,
        strings,
    }
}

fn render(v: Option<usize>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "?".into())
}

fn render_i(v: Option<i64>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "?".into())
}
