//! Walker-completeness gate over the real sample corpus (#56).
//!
//! `AcDb:Classes` records how many instances of each custom class a
//! drawing holds (DXF group 91), so a walk that reaches fewer objects
//! of a class than the file declares is missing records. CI runs the
//! same check over the canonical fixtures via
//! `examples/probe_class_census --strict`; this is the half that gates
//! the 19-file sample corpus when it is present.
//!
//! Not every corpus file's class table is a live census, so this is a
//! **ratchet**: [`KNOWN_SHORTFALL`] pins every declared-vs-walked gap
//! that exists now, by file, class and count. A new gap fails; a
//! *closed* gap also fails, forcing the list to shrink rather than rot.

use dwg::{DwgFile, ObjectType};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Every gap between a class's declared `num_objects` and the number of
/// instances the walk reaches: `(file, class, declared, walked)`.
/// Measured 2026-08-30 on the merged tree.
///
/// **None of these is a walker gap.** `examples/probe_reference_closure`
/// runs three independent measurements on every corpus file and all
/// three are closed on all of them (#76):
///
/// * the walked records tile the whole `AcDb:AcDbObjects` section —
///   after #77 sized the record slice correctly, the only unclaimed
///   bytes on every R2004+ file are the 4-byte `0x0dca` prologue, with
///   **zero** bytes between records, so an unreferenced record has
///   nowhere to sit;
/// * every hard handle reference (§2.13 codes 3 and 5) in every
///   record's handle stream resolves to a map entry, so nothing in the
///   drawing depends on a record the walk cannot reach;
/// * the `AcDbVariableDictionary` — the sole owner of a drawing's
///   DICTIONARYVAR objects — holds exactly as many keys as the walk
///   finds DICTIONARYVAR records on **every** corpus file, including
///   the two whose class table agrees with the walk.
///
/// So the class table's instance count is not a live census on these
/// releases. The per-file numbers below stay pinned anyway: they are
/// what turns a future *drop* in walked records into a test failure.
///
/// * **DICTIONARYVAR / CELLSTYLEMAP on the R2004, R2007 and R2010
///   files** — the variable dictionary holds 10 / 6 / 5 keys and the
///   walk finds 10 / 6 / 5 records while the class table declares
///   16 / 11 / 10. `arc_2010.dwg` is the clinching case: it declares
///   one CELLSTYLEMAP, contains none, and carries no
///   `ACAD_ROUNDTRIP_2008_TABLESTYLE_CELLSTYLEMAP` dictionary key for
///   one to hang from.
/// * **TABLECONTENT / TABLEGEOMETRY on `sample_AC1032.dwg`** — the
///   drawing declares 5 of each against 2 ACAD_TABLE entities and
///   contains 2 of each (#56).
const KNOWN_SHORTFALL: &[(&str, &str, usize, usize)] = &[
    ("arc_2004.dwg", "DICTIONARYVAR", 16, 10),
    ("arc_2004.dwg", "CELLSTYLEMAP", 5, 1),
    ("arc_2007.dwg", "DICTIONARYVAR", 11, 6),
    ("arc_2007.dwg", "CELLSTYLEMAP", 3, 1),
    ("arc_2010.dwg", "DICTIONARYVAR", 10, 5),
    ("arc_2010.dwg", "CELLSTYLEMAP", 1, 0),
    ("circle_2004.dwg", "DICTIONARYVAR", 16, 10),
    ("circle_2004.dwg", "CELLSTYLEMAP", 5, 1),
    ("circle_2007.dwg", "DICTIONARYVAR", 11, 6),
    ("circle_2007.dwg", "CELLSTYLEMAP", 3, 1),
    ("circle_2010.dwg", "DICTIONARYVAR", 10, 5),
    ("circle_2010.dwg", "CELLSTYLEMAP", 1, 0),
    ("line_2004.dwg", "DICTIONARYVAR", 16, 10),
    ("line_2004.dwg", "CELLSTYLEMAP", 5, 1),
    ("line_2007.dwg", "DICTIONARYVAR", 11, 6),
    ("line_2007.dwg", "CELLSTYLEMAP", 3, 1),
    ("line_2010.dwg", "DICTIONARYVAR", 10, 5),
    ("line_2010.dwg", "CELLSTYLEMAP", 1, 0),
    ("sample_AC1032.dwg", "TABLECONTENT", 5, 2),
    ("sample_AC1032.dwg", "TABLEGEOMETRY", 5, 2),
];

fn samples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p
}

#[test]
fn every_sample_reaches_its_declared_class_instances() {
    let dir = samples_dir();
    if !dir.exists() {
        eprintln!("skipping: sample corpus not present at {}", dir.display());
        return;
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("sample dir readable")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dwg"))
        .collect();
    entries.sort();

    let mut checked = 0usize;
    let mut observed: Vec<(String, String, usize, usize)> = Vec::new();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let file = DwgFile::open(&path).unwrap_or_else(|e| panic!("{name}: open failed: {e}"));
        if file.section_by_name("AcDb:Classes").is_none() {
            continue;
        }
        let Some(Ok(classes)) = file.class_map() else {
            panic!("{name}: AcDb:Classes present but unparseable");
        };
        let Some(objects) = file.all_objects() else {
            continue;
        };
        let objects = objects.unwrap_or_else(|e| panic!("{name}: walk failed: {e}"));
        checked += 1;

        // Classes promoted to fixed object types in later releases
        // (LAYOUT, ACDBPLACEHOLDER, ...) are still registered and still
        // counted, but written with the fixed code — credit both, as
        // examples/probe_class_census.rs does.
        let mut walked_fixed: BTreeMap<String, usize> = BTreeMap::new();
        for obj in &objects {
            if !matches!(obj.kind, ObjectType::Custom(_)) {
                *walked_fixed
                    .entry(normalize(&obj.kind.to_string()))
                    .or_default() += 1;
            }
        }

        for def in &classes.classes {
            let walked = objects
                .iter()
                .filter(|o| o.type_code == def.class_number)
                .count()
                + walked_fixed
                    .get(&normalize(&def.dxf_class_name))
                    .copied()
                    .unwrap_or(0);
            if walked < def.num_objects as usize {
                observed.push((
                    name.clone(),
                    def.dxf_class_name.clone(),
                    def.num_objects as usize,
                    walked,
                ));
            }
        }
    }
    assert!(
        checked > 0,
        "corpus present but no file carried AcDb:Classes"
    );

    let mut expected: Vec<(String, String, usize, usize)> = KNOWN_SHORTFALL
        .iter()
        .map(|(f, c, d, w)| (f.to_string(), c.to_string(), *d, *w))
        .collect();
    observed.sort();
    expected.sort();
    assert_eq!(
        observed, expected,
        "class-census shortfall changed. A new entry, or a lower `walked` on an \
         existing one, is a walker regression; a missing entry means a gap \
         closed — delete it from KNOWN_SHORTFALL."
    );
}

/// Fold a class name and a built-in type name into a comparable key:
/// the class table writes `ACDBPLACEHOLDER` where the built-in type
/// table writes `ACDB_PLACEHOLDER`.
fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_uppercase())
        .collect()
}
