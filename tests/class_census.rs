//! Walker-completeness gate over the real sample corpus (#56).
//!
//! `AcDb:Classes` records how many instances of each custom class a
//! drawing holds (DXF group 91), so a walk that reaches fewer objects
//! of a class than the file declares is missing records. CI runs the
//! same check over the canonical fixtures via
//! `examples/probe_class_census --strict`; this is the half that gates
//! the 19-file sample corpus when it is present.
//!
//! The corpus is not clean today, so this is a **ratchet**:
//! [`KNOWN_SHORTFALL`] pins every under-walk that exists now, by file,
//! class and count. A new under-walk fails; a *fixed* under-walk also
//! fails, forcing the list to shrink rather than rot.

use dwg::{DwgFile, ObjectType};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Every under-walk the sample corpus shows today: `(file, class,
/// declared, walked)`. Measured 2026-08-30 on the merged tree.
///
/// Two groups, and they are not the same kind of thing:
///
/// * **TABLECONTENT / TABLEGEOMETRY on `sample_AC1032.dwg`** —
///   explained. The drawing declares 5 of each against 2 ACAD_TABLE
///   entities and contains 2 of each; the object stream is 99.92 %
///   covered with no unclaimed run over 4 bytes and no handle
///   reference the walk cannot answer, so the missing six records are
///   not in the file. See `examples/probe_class_census.rs`
///   `ALLOWLIST`.
/// * **DICTIONARYVAR / CELLSTYLEMAP on the R2004, R2007 and R2010
///   files** — an open walker gap, not explained, and pre-existing on
///   `main`. Pinned here so it cannot grow.
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
        "class-census shortfall changed. A new entry is a walker regression; \
         a missing entry means a gap was fixed — delete it from KNOWN_SHORTFALL."
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
