//! Does the derived RAPIDRTRENDERENVIRONMENT field list close on every corpus record?
//!
//! ODA spec v5.4.1 prints no layout for the class; `src/objects/render_environment.rs` was
//! derived from the bits of the eight records of `3d.dwg` and `Draw2.dwg` (CLEANROOM.md,
//! 2026-10-28; ARCHITECTURE.md §7bs). For every record of every drawing under the corpus
//! directory this reports whether the decoder closed exactly on the record's data-stream
//! boundary. No value is scored: the corpus holds no DXF export of either drawing, so the
//! values are printed, not compared.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_render_environment -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, _rec: Option<&Record>, _t: &mut Tally) -> bool {
    let DecodedEntity::RenderEnvironment(ours) = decoded else {
        return false;
    };
    println!(
        "    version {}, enabled {}, display image {}, rotation {}, image {:?}",
        ours.version, ours.enabled, ours.display_image, ours.rotation, ours.image
    );
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "RAPIDRTRENDERENVIRONMENT",
            class: Some("RAPIDRTRENDERENVIRONMENT"),
            kind: None,
            dxf_kind: "",
            decodes_on: |_| true,
            note: "; boundary closes, values unscored (no DXF twin): the corpus holds no DXF \
                   export of either drawing, so the roles of the version, the two flags and \
                   the rotation are unverified",
        },
        compare,
    )
}
