//! Block-definition name gate (#70).
//!
//! A block's name is written twice in an R2007+ drawing: on the
//! `BLOCK_HEADER` table entry, and on the `BLOCK` sentinel entity that
//! opens the definition's entity sublist. Both live in their record's
//! string stream, so a decoder that reads the wrong one still returns a
//! well-formed string — the two names simply disagree and nothing
//! errors.
//!
//! They *do* disagree in the file: the BLOCK_HEADER stores only the
//! stem of an auto-generated name (`*D`, `*T`, `*U`, `*Paper_Space`)
//! while the sentinel carries the full name with its generated numeric
//! suffix. AutoCAD's own DXF export of `arc_2013.dwg` names the two
//! paper-space BLOCK_RECORDs `*Paper_Space` (handle `6C`) and
//! `*Paper_Space0` (handle `74`) where both DWG records store
//! `*Paper_Space`, so the sentinel is the authority.
//!
//! [`dwg::graph::resolve_block_names`] performs that join; these tests
//! pin it. They skip when the sample corpus is absent.

use dwg::entities::DecodedEntity;
use dwg::{DwgFile, ObjectType};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn samples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p
}

fn open_if_present(name: &str) -> Option<DwgFile> {
    let p = samples_dir().join(name);
    if !p.exists() {
        eprintln!("skipping {name}: sample not present");
        return None;
    }
    Some(DwgFile::open(&p).unwrap_or_else(|e| panic!("{name} open failed: {e}")))
}

/// Every R2007+ file that carries blocks: the resolved name of each
/// BLOCK_HEADER must equal the name its `BLOCK` sentinel carries, the
/// resolved names must be unique (a symbol table cannot hold two
/// entries under one name), and the header's own stored string must be
/// the resolved name minus a decimal suffix.
#[test]
fn resolved_block_names_agree_with_their_block_sentinels() {
    for name in [
        "sample_AC1032.dwg",
        "arc_2010.dwg",
        "arc_2013.dwg",
        "circle_2010.dwg",
        "line_2013.dwg",
    ] {
        let Some(file) = open_if_present(name) else {
            continue;
        };
        let version = file.version();
        let objects = file
            .all_objects()
            .unwrap_or_else(|| panic!("{name}: no handle-map-driven walk"))
            .unwrap_or_else(|e| panic!("{name}: walk failed: {e}"));

        let mut sentinels: BTreeMap<u64, String> = BTreeMap::new();
        for object in &objects {
            if object.kind != ObjectType::Block {
                continue;
            }
            match dwg::entities::decode_from_raw(object, version) {
                DecodedEntity::Block(b) => {
                    sentinels.insert(object.handle.value, b.name);
                }
                other => panic!("{name}: BLOCK {} decoded as {other:?}", object.handle.value),
            }
        }

        let headers: Vec<_> = objects
            .iter()
            .filter(|o| o.kind == ObjectType::BlockHeader)
            .collect();
        assert!(
            !headers.is_empty(),
            "{name}: expected at least one BLOCK_HEADER"
        );
        assert_eq!(
            headers.len(),
            sentinels.len(),
            "{name}: one BLOCK sentinel per BLOCK_HEADER"
        );

        let resolved = dwg::graph::resolve_block_names(&objects, version);
        assert_eq!(
            resolved.len(),
            headers.len(),
            "{name}: resolve_block_names must cover every BLOCK_HEADER"
        );

        let mut seen: BTreeMap<&str, u64> = BTreeMap::new();
        for header in &headers {
            let DecodedEntity::BlockRecord(record) =
                dwg::entities::decode_from_raw(header, version)
            else {
                panic!("{name}: BLOCK_HEADER {} did not decode", header.handle.value);
            };
            let sentinel = record.block_sentinel_handle.unwrap_or_else(|| {
                panic!(
                    "{name}: BLOCK_HEADER {} names no BLOCK sentinel",
                    header.handle.value
                )
            });
            let sentinel_name = sentinels.get(&sentinel).unwrap_or_else(|| {
                panic!(
                    "{name}: BLOCK_HEADER {} points at {sentinel}, which is not a BLOCK",
                    header.handle.value
                )
            });

            // The join must reproduce the sentinel's name exactly.
            let full = &resolved[&header.handle.value];
            assert_eq!(
                full, sentinel_name,
                "{name}: BLOCK_HEADER {} resolved to {full:?}, sentinel says {sentinel_name:?}",
                header.handle.value
            );

            // The record's own string is that name minus a decimal
            // suffix — never a different string.
            let stem = &record.header.name;
            let suffix = full.strip_prefix(stem.as_str()).unwrap_or_else(|| {
                panic!(
                    "{name}: BLOCK_HEADER {} stores {stem:?}, which is not a prefix of {full:?}",
                    header.handle.value
                )
            });
            assert!(
                suffix.chars().all(|c| c.is_ascii_digit()),
                "{name}: BLOCK_HEADER {} stem {stem:?} differs from {full:?} by {suffix:?}, \
                 which is not a decimal suffix",
                header.handle.value
            );

            if let Some(other) = seen.insert(full.as_str(), header.handle.value) {
                panic!(
                    "{name}: block name {full:?} resolved for both {other} and {}",
                    header.handle.value
                );
            }
        }
    }
}

/// Ground truth: AutoCAD's own DXF twin of `arc_2013.dwg` in the public
/// `nextgis/dwg_samples` repository lists exactly three BLOCK_RECORDs —
/// handle `6C` `*Paper_Space`, `70` `*Model_Space`, `74`
/// `*Paper_Space0`. Both `6C` and `74` store `*Paper_Space` in the DWG,
/// so this is the assertion the old reader failed.
#[test]
fn arc_2013_block_names_match_the_autocad_dxf_twin() {
    let Some(file) = open_if_present("arc_2013.dwg") else {
        return;
    };
    let version = file.version();
    let objects = file.all_objects().expect("walk").expect("walk");
    let resolved = dwg::graph::resolve_block_names(&objects, version);
    let expected: BTreeMap<u64, String> = [
        (0x6C, "*Paper_Space"),
        (0x70, "*Model_Space"),
        (0x74, "*Paper_Space0"),
    ]
    .into_iter()
    .map(|(h, n)| (h, n.to_string()))
    .collect();
    assert_eq!(resolved, expected);
}

/// The 27 block definitions of `sample_AC1032.dwg` (AutoCAD 2025
/// output), pinned by name. Before the #70 fix these collapsed to 20
/// distinct strings, with eight records all reading `*D`.
#[test]
fn sample_ac1032_resolves_27_distinct_block_names() {
    let Some(file) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    let version = file.version();
    let objects = file.all_objects().expect("walk").expect("walk");
    let resolved = dwg::graph::resolve_block_names(&objects, version);

    let mut names: Vec<&str> = resolved.values().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "*D10",
            "*D11",
            "*D14",
            "*D22",
            "*D23",
            "*D3",
            "*D4",
            "*D5",
            "*D6",
            "*D7",
            "*D8",
            "*Model_Space",
            "*Paper_Space",
            "*Paper_Space0",
            "*Paper_Space1",
            "*T16",
            "*T9",
            "*U18",
            "*U19",
            "*U25",
            "MyBlock",
            "_ArchTick",
            "_BoxBlank",
            "_ClosedBlank",
            "my-dynamic-block",
            "my_block",
            "my_block_v2",
        ]
    );
}

/// The committed canonical corpus carries no block definitions, so the
/// gate above cannot run there. Pin that fact: the moment
/// `examples/build_fixtures.rs` grows a BLOCK_HEADER, this test fails
/// and the fixture path must be added to the gate.
#[test]
fn canonical_fixtures_carry_no_block_definitions() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/canonical");
    for name in [
        "synthetic_2004.dwg",
        "synthetic_2010.dwg",
        "synthetic_2013.dwg",
        "synthetic_2018.dwg",
    ] {
        let file = DwgFile::open(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let objects = file.all_objects().expect("walk").expect("walk");
        let blocks = objects
            .iter()
            .filter(|o| matches!(o.kind, ObjectType::BlockHeader | ObjectType::Block))
            .count();
        assert_eq!(
            blocks, 0,
            "{name} now carries block records — add it to \
             resolved_block_names_agree_with_their_block_sentinels"
        );
    }
}
