//! Does the §20.4.94 SPATIAL_FILTER field list close on every corpus
//! record, and do its values equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every `SPATIAL_FILTER` record of every drawing under the corpus
//! directory this reports whether the crate's decoder closed exactly on the
//! record's data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus
//! holds AutoCAD's DXF export of the drawing, compares each decoded value
//! with the `AcDbSpatialFilter` object whose handle (group 5) matches: the
//! point count (70), the boundary points (10, 20), the extrusion (210 to
//! 230), the origin (11 to 31), the display boundary (71), the front and back
//! clip switches (72, 73) and the groups 40 that carry the two clip
//! distances, when on, and the two 12-value matrices. R14 has no twin in the
//! corpus, so its record is checked against its boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_spatial_filter -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors, a value differs, a twin
//! lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::SpatialFilter(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let reals = |code: u16| -> Vec<f64> {
        rec.groups
            .iter()
            .filter(|g| g.code == code)
            .filter_map(Group::real)
            .collect()
    };
    t.int(
        "points",
        ours.boundary.len() as i64,
        rec.first(70).and_then(Group::int),
    );
    let (xs, ys) = (reals(10), reals(20));
    for (i, (x, y)) in ours.boundary.iter().enumerate() {
        t.real(&format!("point {i} x"), *x, xs.get(i).copied());
        t.real(&format!("point {i} y"), *y, ys.get(i).copied());
    }
    for (k, (what, codes)) in [("extrusion", [210, 220, 230]), ("origin", [11, 21, 31])]
        .iter()
        .enumerate()
    {
        let mine = if k == 0 { ours.extrusion } else { ours.origin };
        for (v, code) in mine.iter().zip(codes) {
            t.real(
                &format!("{what} group {code}"),
                *v,
                rec.first(*code).and_then(Group::real),
            );
        }
    }
    t.int(
        "display boundary",
        i64::from(ours.display_boundary),
        rec.first(71).and_then(Group::int),
    );
    t.int(
        "front clip on",
        i64::from(ours.front_clip.is_some()),
        rec.first(72).and_then(Group::int),
    );
    t.int(
        "back clip on",
        i64::from(ours.back_clip.is_some()),
        rec.first(73).and_then(Group::int),
    );
    let mut mine: Vec<f64> = ours.front_clip.into_iter().collect();
    mine.extend(ours.inverse_block_transform);
    mine.extend(ours.clip_boundary_transform);
    let theirs = reals(40);
    t.int("groups 40", mine.len() as i64, Some(theirs.len() as i64));
    for (i, v) in mine.iter().enumerate() {
        t.real(&format!("group 40 #{i}"), *v, theirs.get(i).copied());
    }
    let back = reals(41);
    t.int(
        "groups 41",
        i64::from(ours.back_clip.is_some()),
        Some(back.len() as i64),
    );
    if let Some(b) = ours.back_clip {
        t.real("back clip distance", b, back.first().copied());
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "SPATIAL_FILTER",
            class: Some("SPATIAL_FILTER"),
            kind: None,
            dxf_kind: "SPATIAL_FILTER",
            decodes_on: |_| true,
            note: "",
        },
        compare,
    )
}
