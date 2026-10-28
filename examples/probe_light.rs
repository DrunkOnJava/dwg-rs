//! Does the derived LIGHT field list close on every corpus record?
//!
//! ODA spec v5.4.1 prints no layout for the entity; `src/entities/light.rs` was derived from
//! the bits of the one record of `3d.dwg` (CLEANROOM.md, 2026-10-28; ARCHITECTURE.md §7br).
//! For every LIGHT of every drawing under the corpus directory this reports whether the
//! decoder closed exactly on the record's data-stream boundary. No value is scored: the
//! corpus holds no DXF export of the drawing, so the values are printed, not compared.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_light -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, _rec: Option<&Record>, _t: &mut Tally) -> bool {
    let DecodedEntity::Light(ours) = decoded else {
        return false;
    };
    println!(
        "    name {:?}, type {:?}, on {}, intensity {}, position {:?}, target {:?}, \
         hotspot {}, falloff {}",
        ours.name,
        ours.light_type,
        ours.is_on,
        ours.intensity,
        ours.position,
        ours.target,
        ours.hotspot_angle,
        ours.falloff_angle
    );
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "LIGHT",
            class: Some("LIGHT"),
            kind: None,
            dxf_kind: "",
            decodes_on: |_| true,
            note: "; boundary closes, values unscored (no DXF twin): the corpus holds no DXF \
                   export of the drawing, so how the seven BD after the plot glyph split into \
                   an intensity, a position and a target, and the roles of the colour and the \
                   shadow fields, are unverified",
        },
        compare,
    )
}
