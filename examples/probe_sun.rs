//! Does the derived SUN field list close on every corpus record, and do its values
//! equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for SUN; `src/entities/sun.rs` was derived from
//! AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md §7y). For every SUN
//! of every drawing under the corpus directory this reports whether the decoder
//! closed exactly on the record's data-stream boundary (with the string stream
//! consumed) and compares each decoded value with the `SUN` whose handle (group 5)
//! matches in the DXF twin: groups 90, 290, 421 (the low 24 bits of the colour
//! word), 40, 291, 91, 92, 292, 70, 71 and 280. Group 63 is not a field of the data
//! bits and is not compared. The owner handle (330) is in the handle stream. The
//! corpus holds one SUN, in an R2018 drawing.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_sun -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::Sun(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let g = |code: u16| rec.first(code);
    let int = |code: u16| g(code).and_then(Group::int);
    t.int("version (90)", i64::from(ours.dxf_90), int(90));
    t.int("status (290)", i64::from(ours.dxf_290), int(290));
    t.int(
        "colour (421)",
        i64::from(ours.color.word.unwrap_or(0) & 0x00FF_FFFF),
        int(421),
    );
    t.real("intensity (40)", ours.dxf_40, g(40).and_then(Group::real));
    t.int("shadows (291)", i64::from(ours.dxf_291), int(291));
    t.int("julian day (91)", i64::from(ours.dxf_91), int(91));
    t.int("time (92)", i64::from(ours.dxf_92), int(92));
    t.int("daylight savings (292)", i64::from(ours.dxf_292), int(292));
    t.int("shadow type (70)", i64::from(ours.dxf_70), int(70));
    t.int("shadow map size (71)", i64::from(ours.dxf_71), int(71));
    t.int("shadow softness (280)", i64::from(ours.dxf_280), int(280));
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "SUN",
            class: Some("SUN"),
            kind: None,
            dxf_kind: "SUN",
            decodes_on: |_| true,
            note: "; unverified by value: group 63 (not a field of the data bits), the leading BS \
                   of the colour, the order of the two booleans that read 0 (groups 290 and \
                   292), and every release but R2018 (the corpus holds one SUN)",
        },
        compare,
    )
}
