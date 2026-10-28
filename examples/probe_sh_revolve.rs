//! Does the derived ACSH_REVOLVE_CLASS field list close on every corpus record?
//!
//! ODA spec v5.4.1 prints no layout for the class; `src/objects/sh_history.rs` was derived
//! from the bits of the one record of `3d.dwg` (CLEANROOM.md, 2026-10-28; ARCHITECTURE.md
//! §7bq). For every record of every drawing under the corpus directory this reports whether
//! the decoder closed exactly on the record's data-stream boundary. No value is scored: the
//! corpus holds no DXF export of the drawing, so the values are printed, not compared.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_sh_revolve -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, _rec: Option<&Record>, _t: &mut Tally) -> bool {
    let DecodedEntity::ShRevolve(ours) = decoded else {
        return false;
    };
    println!(
        "    node 92 = {}, point {:?}, axis {:?}, angle {}, group 92 = {}, profile {} bytes",
        ours.node_92,
        ours.point,
        ours.axis,
        ours.angle,
        ours.dxf_92,
        ours.profile.len()
    );
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "ACSH_REVOLVE_CLASS",
            class: Some("ACSH_REVOLVE_CLASS"),
            kind: None,
            dxf_kind: "",
            decodes_on: |_| true,
            note: "; boundary closes, values unscored (no DXF twin): the corpus holds no DXF \
                   export of the drawing, so the roles of the point, axis and angle values and \
                   the types of the six zero slots are unverified",
        },
        compare,
    )
}
