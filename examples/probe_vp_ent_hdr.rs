//! Does the §20.4.70 VIEWPORT ENTITY HEADER field list close on every corpus
//! record?
//!
//! # What this proves
//!
//! For every VP_ENT_HDR (type 0x47) of every drawing under the corpus
//! directory this reports whether the crate's decoder closed exactly on the
//! record's data-stream boundary (ARCHITECTURE.md §7i), and prints the entry
//! name and flags it read. Only R14 and R2000 drawings hold one, and
//! AutoCAD's DXF exports of the corpus (R12 for R14, R2000's own) hold no
//! record for it, so there is nothing to compare values with: the evidence is
//! the boundary.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_vp_ent_hdr -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is not decoded.

#[path = "support/twin.rs"]
mod twin;

use dwg::ObjectType;
use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, _rec: Option<&Record>, _t: &mut Tally) -> bool {
    let DecodedEntity::VpEntHdr(ours) = decoded else {
        return false;
    };
    println!(
        "    name {:?}, 64-flag {}, xref index + 1 {}, xdep {}, 1 flag {}",
        ours.name, ours.flag_64, ours.xref_index_plus_1, ours.is_xref_dependent, ours.flag_1
    );
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "VP_ENT_HDR",
            class: None,
            kind: Some(ObjectType::VpEntHdr),
            dxf_kind: "",
            decodes_on: |_| true,
            note: "; the DXF exports hold no VIEWPORT ENTITY HEADER, so only the boundary is checked",
        },
        compare,
    )
}
