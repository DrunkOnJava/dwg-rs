//! Which records error, and where does each one sit in its payload?
//!
//! # What this proves
//!
//! `examples/coverage_report.rs` reports *how many* records error, by
//! type. That is the right granularity for CI, and the wrong
//! granularity for fixing a decoder: a field-list investigation needs
//! the failing record's **handle** (to feed `probe_field_list` /
//! `probe_entity_field_list`), the bit at which its body starts, and
//! the bit at which its data stream must end.
//!
//! This probe prints exactly that, one line per erroring record:
//!
//! ```text
//! 0x0271 MULTILEADER      handle=0x66E  body@6885  data_end=8687  budget=1802
//!        MLEADER max_leader_segments_points 3055747395 exceeds cap 1000
//! ```
//!
//! `budget` is `data_end - body_start` — the number of bits the
//! record's own field list has to consume exactly. A decoder that ends
//! anywhere else has mis-read a field.
//!
//! ```sh
//! cargo run --release --example probe_decode_errors -- samples/sample_AC1032.dwg
//! cargo run --release --example probe_decode_errors -- samples/sample_AC1032.dwg HATCH
//! ```
//!
//! The optional second argument filters by the printed type label
//! (case-insensitive substring), so `HATCH`, `MULTI`, or `0x004E` all
//! narrow the listing.

use dwg::entities::DecodedEntity;
use dwg::{DwgFile, ObjectType};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_decode_errors <file.dwg> [type-filter]");
        return ExitCode::FAILURE;
    };
    let filter = args.next().map(|s| s.to_ascii_uppercase());

    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("open failed ({path}): {e}");
            return ExitCode::FAILURE;
        }
    };
    let version = file.version();
    let Some(Ok(objects)) = file.all_objects() else {
        eprintln!("{path}: no object-stream walk for {version}");
        return ExitCode::FAILURE;
    };
    let class_map = file.class_map().and_then(std::result::Result::ok);

    println!("{path}  ({version})");
    let mut shown = 0usize;
    for object in &objects {
        let decoded = match (class_map.as_ref(), object.kind) {
            (Some(cm), ObjectType::Custom(code)) => {
                dwg::entities::decode_from_raw_with_class_map(object, version, cm, code)
            }
            _ => dwg::entities::decode_from_raw(object, version),
        };
        let DecodedEntity::Error { message, .. } = &decoded else {
            continue;
        };
        let label = match object.kind {
            ObjectType::Custom(code) => class_map
                .as_ref()
                .and_then(|m| m.by_type_code(code))
                .map(|d| d.dxf_class_name.clone())
                .unwrap_or_else(|| format!("{}", object.kind)),
            other => format!("{other}"),
        };
        if let Some(want) = filter.as_deref() {
            if !label.to_ascii_uppercase().contains(want)
                && !format!("0x{:04X}", object.type_code).contains(want)
            {
                continue;
            }
        }
        let body = dwg::object::body_cursor(object, version)
            .map(|c| c.position_bits())
            .ok();
        let data_end = dwg::object::data_end_bit(object, version);
        let budget = match (body, data_end) {
            (Some(b), Some(e)) => format!("{}", e as isize - b as isize),
            _ => "?".into(),
        };
        println!(
            "0x{:04X} {label:<24} handle=0x{:<6X} body@{:<7} data_end={:<7} budget={budget}",
            object.type_code,
            object.handle.value,
            body.map(|b| b.to_string()).unwrap_or_else(|| "?".into()),
            data_end
                .map(|e| e.to_string())
                .unwrap_or_else(|| "?".into()),
        );
        println!("       {message}");
        shown += 1;
    }
    println!("{shown} erroring record(s)");
    ExitCode::SUCCESS
}
