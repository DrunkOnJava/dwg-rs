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
//! DXF class name for a custom class. Exit status is non-zero when a
//! value differs or a record decodes in one file and not the other.

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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [a, b, kind] = args.as_slice() else {
        eprintln!("usage: probe_cross_release <a.dwg> <b.dwg> <KIND>");
        return ExitCode::FAILURE;
    };
    let (left, right) = match (decode_all(a, kind), decode_all(b, kind)) {
        (Ok(l), Ok(r)) => (l, r),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let (mut same, mut differ, mut unmatched) = (0usize, 0usize, 0usize);
    for (handle, l) in &left {
        let Some(r) = right.get(handle) else {
            unmatched += 1;
            continue;
        };
        let (ls, rs) = (format!("{l:?}"), format!("{r:?}"));
        let both_decoded = l.is_decoded() && r.is_decoded();
        if ls == rs && both_decoded {
            same += 1;
        } else {
            differ += 1;
            println!("0x{handle:X}\n  {a}: {ls}\n  {b}: {rs}");
        }
    }
    println!(
        "{kind}: {} in {a}, {} in {b}; {same} decode to equal values, {differ} differ, \
         {unmatched} handles only in the first",
        left.len(),
        right.len()
    );
    if differ > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
