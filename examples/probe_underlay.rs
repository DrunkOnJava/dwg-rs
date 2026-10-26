//! Does the PDFUNDERLAY decoder close on every corpus record, on every release,
//! and do its values equal AutoCAD's DXF export?
//!
//! `src/entities/underlay.rs` reads the underlay reference; before this probe the
//! dispatcher routed it on R2010 and later only, so the R14, R2000, R2004 and R2007
//! records of the corpus were skipped (ARCHITECTURE.md §7x). For every PDFUNDERLAY
//! of every drawing under the corpus directory this reports whether the decoder
//! closed exactly on the record's data-stream boundary and compares each decoded
//! value with the `PDFUNDERLAY` of the same handle (group 5) in the DXF twin: the
//! insertion point (groups 10, 20, 30), the flags (280), the contrast (281) and the
//! fade (282). The scale (41, 42, 43), rotation (50, in degrees in the DXF), normal
//! (210, 220, 230) and the clip vertices (11, 21) are compared where the DXF
//! writes them and counted as omitted where it does not. The definition handle
//! (340) is in the handle stream and is not compared. R14 has no DXF twin in the
//! corpus, so its record is checked against its boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_underlay -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use dwg::entities::underlay::UnderlayKind;
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

fn nth(rec: &Record, code: u16, n: usize) -> Option<&Group> {
    rec.groups.iter().filter(|g| g.code == code).nth(n)
}

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::Underlay(ours) = decoded else {
        return false;
    };
    if ours.kind != UnderlayKind::Pdf {
        return false;
    }
    let Some(rec) = rec else {
        return true;
    };
    let int = |code: u16| nth(rec, code, 0).and_then(Group::int);
    let real = |code: u16| nth(rec, code, 0).and_then(Group::real);
    t.real("insertion x (10)", ours.insertion_point.x, real(10));
    t.real("insertion y (20)", ours.insertion_point.y, real(20));
    t.real("insertion z (30)", ours.insertion_point.z, real(30));
    t.int("flags (280)", i64::from(ours.flags), int(280));
    t.int("contrast (281)", i64::from(ours.contrast), int(281));
    t.int("fade (282)", i64::from(ours.fade), int(282));
    t.real("scale x (41)", ours.scale.x, real(41));
    t.real("scale y (42)", ours.scale.y, real(42));
    t.real("scale z (43)", ours.scale.z, real(43));
    t.real("rotation (50)", ours.rotation.to_degrees(), real(50));
    t.real("normal x (210)", ours.normal.x, real(210));
    t.real("normal y (220)", ours.normal.y, real(220));
    t.real("normal z (230)", ours.normal.z, real(230));
    let theirs = rec.groups.iter().filter(|g| g.code == 11).count();
    if theirs > 0 || !ours.clip_polygon.is_empty() {
        t.int(
            "clip vertex count (11)",
            ours.clip_polygon.len() as i64,
            Some(theirs as i64),
        );
        for (i, p) in ours.clip_polygon.iter().enumerate() {
            t.real("clip x (11)", p.x, nth(rec, 11, i).and_then(Group::real));
            t.real("clip y (21)", p.y, nth(rec, 21, i).and_then(Group::real));
        }
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "PDFUNDERLAY",
            class: Some("PDFUNDERLAY"),
            kind: None,
            dxf_kind: "PDFUNDERLAY",
            decodes_on: |_| true,
            note: "; unverified by value: the scale, rotation and normal (the DXF omits them \
                   at their defaults) and the encoding of a clip vertex",
        },
        compare,
    )
}
