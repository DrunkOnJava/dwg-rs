//! Do two saves of the same drawing decode to the same values?
//!
//! # What this proves
//!
//! AutoCAD keeps an object's handle when it saves a drawing as another
//! release, so two saves of one drawing hold the same records under the
//! same handles. For every handle both files decode to a record of the
//! given kind, this compares the two decoded values (`{:?}` of the
//! `DecodedEntity`) and prints any that differ. Where one release has no
//! DXF export (R14 in the public corpus), agreement with a release that
//! has one carries that release's DXF check over by handle.
//!
//! ```sh
//! cargo run --release --example probe_cross_release -- sample_AC1014.dwg sample_AC1015.dwg LWPOLYLINE
//! ```
//!
//! The kind is matched against the record's `ObjectType` label, or its
//! DXF class name for a custom class. A fourth argument,
//! `--ignore=name,name`, leaves out struct fields one of the two
//! releases does not store (for example the R2000+ fields of a
//! dimension's common data on R14), so the rest can still be compared.
//! Exit status is non-zero when a value differs or a record decodes in
//! one file and not the other.

use dwg::entities::DecodedEntity;
use dwg::{DwgFile, ObjectType};
use std::collections::BTreeMap;
use std::process::ExitCode;

fn decode_all(path: &str, kind: &str) -> Result<BTreeMap<u64, DecodedEntity>, String> {
    let file = DwgFile::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let version = file.version();
    let objects = match file.all_objects() {
        Some(Ok(o)) => o,
        _ => return Err(format!("{path}: no object walk")),
    };
    let class_map = file.class_map().and_then(Result::ok);
    let mut out = BTreeMap::new();
    for object in &objects {
        let label = match (object.kind, class_map.as_ref()) {
            (ObjectType::Custom(code), Some(cm)) => cm
                .by_type_code(code)
                .map(|d| d.dxf_class_name.clone())
                .unwrap_or_default(),
            (k, _) => k.short_label().to_string(),
        };
        if label != kind {
            continue;
        }
        let decoded = match (object.kind, class_map.as_ref()) {
            (ObjectType::Custom(code), Some(cm)) => {
                dwg::entities::decode_from_raw_with_class_map(object, version, cm, code)
            }
            _ => dwg::entities::decode_from_raw(object, version),
        };
        out.insert(object.handle.value, decoded);
    }
    Ok(out)
}

/// Replace the value of every `name: value` pair named in `ignore` in a
/// `{:?}` rendering with `_`. A value runs to the next `,` or `}` at
/// its own nesting depth.
fn mask(debug: &str, ignore: &[&str]) -> String {
    let mut out = debug.to_string();
    for name in ignore {
        let key = format!("{name}: ");
        let mut from = 0;
        while let Some(found) = out[from..].find(&key) {
            let start = from + found + key.len();
            let preceded = out[..from + found]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            let mut depth = 0i32;
            let mut end = out.len();
            for (i, ch) in out[start..].char_indices() {
                match ch {
                    '{' | '(' | '[' => depth += 1,
                    '}' | ')' | ']' if depth == 0 => {
                        end = start + i;
                        break;
                    }
                    '}' | ')' | ']' => depth -= 1,
                    ',' if depth == 0 => {
                        end = start + i;
                        break;
                    }
                    _ => {}
                }
            }
            if preceded {
                out.replace_range(start..end, "_");
                from = start + 1;
            } else {
                from = start;
            }
        }
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (files, ignore): (Vec<&String>, Vec<&String>) =
        args.iter().partition(|a| !a.starts_with("--ignore="));
    let ignore: Vec<&str> = ignore
        .iter()
        .flat_map(|a| a.trim_start_matches("--ignore=").split(','))
        .filter(|s| !s.is_empty())
        .collect();
    let [a, b, kind] = files.as_slice() else {
        eprintln!("usage: probe_cross_release <a.dwg> <b.dwg> <KIND> [--ignore=field,field]");
        return ExitCode::FAILURE;
    };
    let (left, right) = match (decode_all(a, kind), decode_all(b, kind)) {
        (Ok(l), Ok(r)) => (l, r),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let (mut same, mut differ, mut only_left) = (0usize, 0usize, 0usize);
    for (handle, l) in &left {
        let Some(r) = right.get(handle) else {
            only_left += 1;
            println!("0x{handle:X} only in {a}");
            continue;
        };
        let (ls, rs) = (
            mask(&format!("{l:?}"), &ignore),
            mask(&format!("{r:?}"), &ignore),
        );
        let both_decoded = l.is_decoded() && r.is_decoded();
        if ls == rs && both_decoded {
            same += 1;
        } else {
            differ += 1;
            println!("0x{handle:X}\n  {a}: {ls}\n  {b}: {rs}");
        }
    }
    let only_right = right.keys().filter(|h| !left.contains_key(h)).count();
    for handle in right.keys().filter(|h| !left.contains_key(h)) {
        println!("0x{handle:X} only in {b}");
    }
    println!(
        "{kind}: {} in {a}, {} in {b}; {same} decode to equal values, {differ} differ, \
         {only_left} handles only in the first, {only_right} only in the second",
        left.len(),
        right.len()
    );
    if differ > 0 || only_left > 0 || only_right > 0 || same == 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
