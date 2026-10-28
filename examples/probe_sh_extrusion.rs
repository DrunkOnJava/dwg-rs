//! Does the derived ACSH_EXTRUSION_CLASS field list close on every corpus record, and
//! do its values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for the class; `src/objects/sh_history.rs` was
//! derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md §7bo).
//! For every record of every drawing under the corpus directory this reports whether the
//! decoder closed exactly on the record's data-stream boundary (from R2007 with the string
//! stream consumed) and compares each decoded value with the object of the same handle
//! (group 5) in the DXF twin: the evaluation base (98, 99, and group 90 for the node id),
//! the history node (90, 91, the 16 transform values of groups 40 to 55 with groups 50 to
//! 55 in degrees, 62, 92), the sweep base (90, 91, 10, 20, 30, 92, the bit count 90, the
//! binary data of groups 310, 93, the six scalars 42 to 45, 48, 49, the flags 290, 70, 71,
//! 292 to 296, 11, 21, 31) and the two 16-value matrices of groups 46 and 47. The DXF
//! writes the profile data as one byte more than its bit count holds; that is checked as a
//! measured convention. Handles (347 and the rest) are in the handle stream and are not
//! compared. R14 has no DXF twin in the corpus, and AutoCAD's R2000 and R2004 exports hold
//! no ACSH_EXTRUSION_CLASS, so those records are checked against their boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_sh_extrusion -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value differs, a twin
//! lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

fn nth(rec: &Record, code: u16, n: usize) -> Option<&Group> {
    rec.groups.iter().filter(|g| g.code == code).nth(n)
}

fn hex_bytes(rec: &Record) -> Vec<u8> {
    let text: String = rec
        .groups
        .iter()
        .filter(|g| g.code == 310)
        .map(|g| g.value.trim())
        .collect();
    (0..text.len() / 2)
        .filter_map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok())
        .collect()
}

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::ShExtrusion(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let int = |code: u16, n: usize| nth(rec, code, n).and_then(Group::int);
    let real = |code: u16, n: usize| nth(rec, code, n).and_then(Group::real);
    t.int("node id (90)", i64::from(ours.eval.node_id), int(90, 0));
    t.int("eval 98", i64::from(ours.eval.dxf_98), int(98, 0));
    t.int("eval 99", i64::from(ours.eval.dxf_99), int(99, 0));
    t.int("node 90", i64::from(ours.node_90), int(90, 1));
    t.int("node 91", i64::from(ours.node_91), int(91, 0));
    for (i, v) in ours.transform.iter().enumerate() {
        let code = 40 + i as u16;
        let v = if code >= 50 { v.to_degrees() } else { *v };
        t.real(&format!("transform group {code}"), v, real(code, 0));
    }
    let color = match ours.color.word {
        Some(word) if word >> 24 == 0xC0 => 256,
        Some(word) => i64::from(word),
        None => i64::from(ours.color.index),
    };
    t.int("colour (62)", color, int(62, 0));
    t.int("node 92", i64::from(ours.node_92), int(92, 0));
    t.int("sweep 90", i64::from(ours.sweep_90), int(90, 2));
    t.int("sweep 91", i64::from(ours.sweep_91), int(91, 1));
    for (i, code) in [10u16, 20, 30].iter().enumerate() {
        t.real("sweep point", ours.point[i], real(*code, 0));
    }
    t.int("sweep 92", i64::from(ours.dxf_92), int(92, 1));
    t.int(
        "profile bit count (90)",
        8 * ours.profile.len() as i64,
        int(90, 3),
    );
    let theirs = hex_bytes(rec);
    t.int(
        "profile bytes the DXF holds beyond the bit count",
        1,
        Some(theirs.len() as i64 - ours.profile.len() as i64),
    );
    let same = theirs.starts_with(&ours.profile);
    t.int("profile data (310)", 1, Some(i64::from(same)));
    t.int("sweep 93", i64::from(ours.dxf_93), int(93, 0));
    for (i, code) in [42u16, 43, 44, 45, 48, 49].iter().enumerate() {
        t.real("scalar", ours.scalars[i], real(*code, 1));
    }
    t.int("group 290", i64::from(ours.dxf_290), int(290, 0));
    t.int("group 70", i64::from(ours.dxf_70), int(70, 0));
    t.int("group 71", i64::from(ours.dxf_71), int(71, 0));
    for (i, code) in [292u16, 293, 294, 295, 296].iter().enumerate() {
        t.int("flag", i64::from(ours.flags[i]), int(*code, 0));
    }
    for (i, code) in [11u16, 21, 31].iter().enumerate() {
        t.real("vector", ours.vector[i], real(*code, 0));
    }
    for i in 0..16 {
        t.real("matrix 46", ours.matrix_46[i], real(46, i + 1));
        t.real("matrix 47", ours.matrix_47[i], real(47, i + 1));
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "ACSH_EXTRUSION_CLASS",
            class: Some("ACSH_EXTRUSION_CLASS"),
            kind: None,
            dxf_kind: "ACSH_EXTRUSION_CLASS",
            decodes_on: |_| true,
            note: "; unverified by value: the roles of the equal zeros among groups 42 to 45 and 49, \
                   of groups 293, 294 and 296 and of 11, 21 and 31, the meaning of group 92 = 77, \
                   how a profile bit count that is not a multiple of 8 is stored, the profile's \
                   own structure, the BL -1 that opens the evaluation base and the leading BS of \
                   the R2004+ colour",
        },
        compare,
    )
}
