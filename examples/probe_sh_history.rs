//! Do the derived ACSH_HISTORY_CLASS and ACSH_BOX_CLASS field lists close on
//! every corpus record, and do their values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for either; `src/objects/sh_history.rs` was
//! derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md
//! §7bj). For every record of the two classes under the corpus directory this
//! reports whether the decoder closed exactly on the record's data-stream
//! boundary (from R2007 with the string stream consumed) and compares each
//! decoded value with the object of the same handle in the DXF twin: for the
//! history, groups 90, 91, 92, 280 and 281; for the box, the evaluation base
//! (90, 98, 99), the history node (90, 91, the 16 transform values of groups
//! 40 to 55, 92), the colour and the box (90, 91, 40, 41, 42). Two conventions
//! are measured rather than assumed: the DXF writes groups 50 to 55 in degrees,
//! and writes group 62 = 256 for the colour word 0xC0000000 (ByLayer) from
//! R2004, where the pre-R2004 `BS` is the 256 itself. Handles (347, 360) are in
//! the handle stream and are not compared. R14 has no DXF twin in the corpus, so
//! its records are checked against their boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_sh_history -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

fn nth(rec: &Record, code: u16, n: usize) -> Option<&Group> {
    rec.groups.iter().filter(|g| g.code == code).nth(n)
}

fn history(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::ShHistory(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let int = |code: u16| nth(rec, code, 0).and_then(Group::int);
    t.int("group 90", i64::from(ours.dxf_90), int(90));
    t.int("group 91", i64::from(ours.dxf_91), int(91));
    t.int("group 92", i64::from(ours.dxf_92), int(92));
    t.int("group 280", i64::from(ours.dxf_280), int(280));
    t.int("group 281", i64::from(ours.dxf_281), int(281));
    true
}

fn sh_box(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::ShBox(ours) = decoded else {
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
    t.int("box 90", i64::from(ours.box_90), int(90, 2));
    t.int("box 91", i64::from(ours.box_91), int(91, 1));
    for (i, code) in [40u16, 41, 42].iter().enumerate() {
        t.real("box size", ours.size[i], real(*code, 1));
    }
    true
}

fn main() -> ExitCode {
    let history_code = twin::run(
        &Probe {
            tag: "ACSH_HISTORY_CLASS",
            class: Some("ACSH_HISTORY_CLASS"),
            kind: None,
            dxf_kind: "ACSH_HISTORY_CLASS",
            decodes_on: |_| true,
            note: "",
        },
        history,
    );
    let box_code = twin::run(
        &Probe {
            tag: "ACSH_BOX_CLASS",
            class: Some("ACSH_BOX_CLASS"),
            kind: None,
            dxf_kind: "ACSH_BOX_CLASS",
            decodes_on: |_| true,
            note: "; the BL -1 that opens the evaluation base and the leading BS of the \
                   R2004+ colour (0, where group 62 says 256) are unverified by value",
        },
        sh_box,
    );
    let ok = format!("{:?}", ExitCode::SUCCESS);
    if format!("{history_code:?}") == ok && format!("{box_code:?}") == ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
