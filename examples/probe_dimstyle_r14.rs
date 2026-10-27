//! Compare every R14 DIMSTYLE record with the same record of an R2000
//! save of the drawing (#74).
//!
//! AutoCAD keeps handles across "save as", so a record of the R14 save
//! and the record with the same handle in the R2000 save are the same
//! dimension style. The R14 one is read with §20.4.68's "R13 & R14 Only"
//! block, the R2000 one with the R2000+ list, and the fifteen fields
//! `DimStyleEntry` surfaces must agree. The name is compared without
//! regard to case, since R14 stores names in capitals. `DIMALTRND` has no
//! slot in the R13/R14 block, so it is left out.
//!
//! ```sh
//! cargo run --release --example probe_dimstyle_r14 -- <r14.dwg> <r2000.dwg>
//! ```
//!
//! Exits non-zero if a record of either file fails to decode, a handle
//! has no partner, a field differs, or the R14 file has no DIMSTYLE.

use std::collections::BTreeMap;
use std::process::ExitCode;

use dwg::entities::DecodedEntity;
use dwg::tables::dimstyle::DimStyleEntry;
use dwg::{DwgFile, ObjectType, Version};

fn dimstyles(path: &str, want: Version) -> Result<BTreeMap<u64, DimStyleEntry>, String> {
    let file = DwgFile::open(path).map_err(|e| format!("{path}: {e}"))?;
    let version = file.version();
    if version != want {
        return Err(format!("{path}: {version}, expected {want}"));
    }
    let objects = file
        .all_objects()
        .ok_or_else(|| format!("{path}: no object stream"))?
        .map_err(|e| format!("{path}: {e}"))?;
    let mut out = BTreeMap::new();
    for object in objects.iter().filter(|o| o.kind == ObjectType::DimStyle) {
        match dwg::entities::decode_from_raw(object, version) {
            DecodedEntity::DimStyle(d) => {
                out.insert(object.handle.value, d);
            }
            other => {
                return Err(format!(
                    "{path}: DIMSTYLE 0x{:X} did not decode: {other:?}",
                    object.handle.value
                ));
            }
        }
    }
    Ok(out)
}

fn differences(a: &DimStyleEntry, b: &DimStyleEntry) -> Vec<String> {
    let mut out = Vec::new();
    if !a.header.name.eq_ignore_ascii_case(&b.header.name) {
        out.push(format!("name {:?} / {:?}", a.header.name, b.header.name));
    }
    for (what, x, y) in [
        ("dimscale", a.dimscale, b.dimscale),
        ("dimasz", a.dimasz, b.dimasz),
        ("dimexo", a.dimexo, b.dimexo),
        ("dimexe", a.dimexe, b.dimexe),
        ("dimtxt", a.dimtxt, b.dimtxt),
        ("dimcen", a.dimcen, b.dimcen),
        ("dimtfac", a.dimtfac, b.dimtfac),
        ("dimlfac", a.dimlfac, b.dimlfac),
        ("dimaltf", a.dimaltf, b.dimaltf),
    ] {
        if x != y {
            out.push(format!("{what} {x} / {y}"));
        }
    }
    for (what, x, y) in [
        ("flag_64", a.header.flag_64, b.header.flag_64),
        (
            "xdep",
            a.header.is_xref_dependent,
            b.header.is_xref_dependent,
        ),
        ("dimtih", a.dimtih, b.dimtih),
        ("dimtoh", a.dimtoh, b.dimtoh),
        ("dimupt", a.dimupt, b.dimupt),
    ] {
        if x != y {
            out.push(format!("{what} {x} / {y}"));
        }
    }
    for (what, x, y) in [
        ("dimtad", a.dimtad, b.dimtad),
        ("dimtolj", a.dimtolj, b.dimtolj),
    ] {
        if x != y {
            out.push(format!("{what} {x} / {y}"));
        }
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [r14, r2000] = args.as_slice() else {
        eprintln!("usage: probe_dimstyle_r14 <r14.dwg> <r2000.dwg>");
        return ExitCode::from(2);
    };
    let (old, new) = match (
        dimstyles(r14, Version::R14),
        dimstyles(r2000, Version::R2000),
    ) {
        (Ok(old), Ok(new)) => (old, new),
        (Err(e), _) | (_, Err(e)) => {
            println!("{e}");
            println!("RESULT: FAIL");
            return ExitCode::FAILURE;
        }
    };
    let mut failures = 0;
    for (handle, a) in &old {
        let Some(b) = new.get(handle) else {
            failures += 1;
            println!(
                "0x{handle:X} {}: no R2000 record with this handle",
                a.header.name
            );
            continue;
        };
        let diff = differences(a, b);
        if diff.is_empty() {
            println!("0x{handle:X} {:<20} 15 fields equal", a.header.name);
        } else {
            failures += 1;
            println!("0x{handle:X} {}: {}", a.header.name, diff.join(", "));
        }
    }
    let name = r14.rsplit('/').next().unwrap_or(r14);
    println!(
        "{name}: {} R14 DIMSTYLE, {failures} differ or unpaired",
        old.len()
    );
    if old.is_empty() || failures > 0 {
        println!("RESULT: FAIL");
        ExitCode::FAILURE
    } else {
        println!("RESULT: every R14 DIMSTYLE equals its R2000 save");
        ExitCode::SUCCESS
    }
}
