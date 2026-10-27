//! Which R13/R14 entity records fail, and what do their preambles have in
//! common? (ODA spec §20.4.1)
//!
//! # What this proves
//!
//! `coverage_report` says how many records of a type error; this probe
//! says what the erroring ones look like before their entity-specific
//! fields start. For every entity record of an R13 or R14 drawing it
//! re-reads the §20.4.1 common entity data and prints one row:
//!
//! ```text
//! handle type  body@ common@ rl  payload mode react eed gfx lt invis verdict
//! ```
//!
//! `body@` is the bit after the handle, `common@` the bit the common
//! entity data ends on, `rl` the §20.4.1 "R13-R14 Only: Obj size" that is
//! the record's data-stream boundary, and `verdict` is `ok`, `error` or
//! `unhandled` from the real dispatcher. A closing table groups the rows
//! by type and verdict so a feature that only the failing records share
//! (EED, a graphics block, reactors, a non-default linetype) stands out.
//!
//! ```sh
//! cargo run --release --example probe_r13_entities -- drawing.dwg [type-filter]
//! ```

use dwg::common_entity::read_common_entity_data;
use dwg::entities::{DecodedEntity, decode_from_raw};
use dwg::{BitCursor, DwgFile, ObjectType};
use std::collections::BTreeMap;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_r13_entities <file.dwg> [type-filter]");
        return ExitCode::FAILURE;
    };
    let filter = args.next().map(|s| s.to_ascii_uppercase());
    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot open {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let version = file.version();
    if !version.is_r13_r14() {
        eprintln!("{path} is {version}: this probe reads the R13-R14 preamble");
        return ExitCode::FAILURE;
    }
    let Some(Ok(raws)) = file.all_objects() else {
        eprintln!("{path}: no object walk");
        return ExitCode::FAILURE;
    };

    println!("== {path} ({version}) ==");
    println!(
        "{:<14} {:>7} {:>6} {:>7} {:>6} {:>7} {:>4} {:>5} {:>4} {:>4} {:>3} {:>5}  verdict",
        "type",
        "handle",
        "body@",
        "common@",
        "rl",
        "payload",
        "mode",
        "react",
        "eed",
        "gfx",
        "lt",
        "invis"
    );
    let mut groups: BTreeMap<(String, String, bool, bool, bool, bool), usize> = BTreeMap::new();
    for raw in &raws {
        let kind = ObjectType::from_code(raw.type_code);
        if !kind.is_entity() {
            continue;
        }
        let label = format!("{kind}");
        if let Some(f) = &filter {
            if !label.to_ascii_uppercase().contains(f) {
                continue;
            }
        }
        let verdict = match decode_from_raw(raw, version) {
            DecodedEntity::Error { message, .. } => format!("error: {message}"),
            DecodedEntity::Unhandled { .. } => "unhandled".to_string(),
            _ => "ok".to_string(),
        };
        let mut c = BitCursor::new(&raw.raw);
        let _ = c.read_bs_u();
        let _ = c.read_handle();
        let body = c.position_bits();
        let common = read_common_entity_data(&mut c, version);
        let (common_at, rl, mode, react, eed, gfx, lt, invis) = match &common {
            Ok(d) => (
                c.position_bits().to_string(),
                d.data_end_bits.map_or("-".into(), |v| v.to_string()),
                d.raw_mode.to_string(),
                d.num_reactors.to_string(),
                d.had_extended_data,
                d.had_graphics,
                d.non_fixed_ltype,
                d.invisibility.to_string(),
            ),
            Err(e) => (
                format!("ERR {e}"),
                "-".into(),
                "-".into(),
                "-".into(),
                false,
                false,
                false,
                "-".into(),
            ),
        };
        let ok = verdict == "ok";
        let reacts = react != "0" && react != "-";
        *groups
            .entry((
                label.clone(),
                if ok { "ok".into() } else { "fail".into() },
                eed,
                gfx,
                lt,
                reacts,
            ))
            .or_default() += 1;
        if !ok || filter.is_some() {
            println!(
                "{:<14} {:>7} {:>6} {:>7} {:>6} {:>7} {:>4} {:>5} {:>4} {:>4} {:>3} {:>5}  {}",
                label,
                format!("0x{:X}", raw.handle.value),
                body,
                common_at,
                rl,
                raw.raw.len() * 8,
                mode,
                react,
                eed as u8,
                gfx as u8,
                lt as u8,
                invis,
                verdict,
            );
        }
    }
    println!();
    println!(
        "{:<14} {:<5} {:>3} {:>3} {:>3} {:>5}  records",
        "type", "res", "eed", "gfx", "lt", "react"
    );
    for ((label, res, eed, gfx, lt, react), n) in &groups {
        println!(
            "{label:<14} {res:<5} {:>3} {:>3} {:>3} {:>5}  {n}",
            *eed as u8, *gfx as u8, *lt as u8, *react as u8
        );
    }
    ExitCode::SUCCESS
}
