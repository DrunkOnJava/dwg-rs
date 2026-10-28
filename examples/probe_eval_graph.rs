//! Does the derived ACAD_EVALUATION_GRAPH field list close on every corpus record,
//! and do its values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for the class; `src/objects/eval_graph.rs` was
//! derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md §7bo).
//! For every record of every drawing under the corpus directory this reports whether
//! the decoder closed exactly on the record's data-stream boundary and compares each
//! decoded value with the object of the same handle (group 5) in the DXF twin. After
//! groups 96 and 97 the twin lists each node as groups 91, 93, 95, 360 and four 92,
//! then each edge as 92, 93, 94, two 91 and five 92; the decoded node and edge counts
//! (BL fields no DXF group shows) are checked by the number of groups the twin holds
//! after group 97, which must equal 8 per node plus 10 per edge. Node handles (360)
//! are in the handle stream and are not compared. R14 has no DXF twin in the corpus,
//! and AutoCAD's R2000 and R2004 exports keep one of the three graphs and omit the
//! other two, so those records are checked against their boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_eval_graph -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Group, Pairing, Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::EvalGraph(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let from = rec
        .groups
        .iter()
        .position(|g| g.code == 96)
        .unwrap_or(rec.groups.len());
    let g = &rec.groups[from..];
    let at = |i: usize| g.get(i).and_then(Group::int);
    t.int("group 96", i64::from(ours.dxf_96), at(0));
    t.int("group 97", i64::from(ours.dxf_97), at(1));
    t.int(
        "groups after 97",
        (8 * ours.nodes.len() + 10 * ours.edges.len()) as i64,
        Some(g.len().saturating_sub(2) as i64),
    );
    let mut i = 2;
    for n in &ours.nodes {
        let code = |k: usize, want: u16| g.get(i + k).is_some_and(|x| x.code == want);
        let shape = code(0, 91) && code(1, 93) && code(2, 95) && code(3, 360);
        t.int("node group order", 1, Some(i64::from(shape)));
        t.int("node 91", i64::from(n.dxf_91), at(i));
        t.int("node 93", i64::from(n.dxf_93), at(i + 1));
        t.int("node 95", i64::from(n.dxf_95), at(i + 2));
        for (k, v) in n.dxf_92.iter().enumerate() {
            t.int("node 92", i64::from(*v), at(i + 4 + k));
        }
        i += 8;
    }
    for e in &ours.edges {
        let code = |k: usize, want: u16| g.get(i + k).is_some_and(|x| x.code == want);
        let shape = code(0, 92) && code(1, 93) && code(2, 94) && code(3, 91) && code(4, 91);
        t.int("edge group order", 1, Some(i64::from(shape)));
        t.int("edge 92", i64::from(e.dxf_92), at(i));
        t.int("edge 93", i64::from(e.dxf_93), at(i + 1));
        t.int("edge 94", i64::from(e.dxf_94), at(i + 2));
        t.int("edge 91", i64::from(e.dxf_91[0]), at(i + 3));
        t.int("edge 91", i64::from(e.dxf_91[1]), at(i + 4));
        for (k, v) in e.dxf_92_links.iter().enumerate() {
            t.int("edge 92 link", i64::from(*v), at(i + 5 + k));
        }
        i += 10;
    }
    true
}

fn main() -> ExitCode {
    twin::run_with_twin(
        &Probe {
            tag: "ACAD_EVALUATION_GRAPH",
            class: Some("ACAD_EVALUATION_GRAPH"),
            kind: None,
            dxf_kind: "ACAD_EVALUATION_GRAPH",
            decodes_on: |_| true,
            note: "; unverified by value: what the link values of a node and of an edge mean, \
                   and the node handles (group 360, in the handle stream)",
        },
        Pairing::HandleOrAbsent,
        |d, r, _, t| compare(d, r, t),
    )
}
