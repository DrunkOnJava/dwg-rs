//! Does the §20.4.37 SHAPE field list close on every corpus record, and do
//! its values equal AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every SHAPE (type 0x21) of every drawing under the corpus directory
//! this reports whether the crate's decoder closed exactly on the record's
//! data-stream boundary (ARCHITECTURE.md §7i) and, where the corpus holds
//! AutoCAD's DXF export of the drawing, compares each decoded value with the
//! `AcDbShape` entity whose handle (group 5) matches: the insertion point
//! (10, 20, 30), scale (40), rotation (50), width factor (41), oblique angle
//! (51), thickness (39) and extrusion (210, 220, 230). A group the DXF leaves
//! out (AutoCAD omits a default) is counted as omitted, not compared against
//! an assumed default. The shape number has no DXF group, the DXF writing the
//! shape's name (group 2) instead, so it is not compared. R14 has no twin in
//! the corpus, so its record is checked against its boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_shape -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors, a value differs, a twin
//! lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::ObjectType;
use dwg::entities::DecodedEntity;
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::Shape(ours) = decoded else {
        return false;
    };
    let Some(rec) = rec else {
        return true;
    };
    let real = |code: u16| rec.first(code).and_then(Group::real);
    for (what, v, code) in [
        ("insertion x", ours.insertion_point.x, 10),
        ("insertion y", ours.insertion_point.y, 20),
        ("insertion z", ours.insertion_point.z, 30),
        ("scale", ours.scale, 40),
        ("rotation", ours.rotation, 50),
        ("width factor", ours.width_factor, 41),
        ("oblique", ours.oblique, 51),
        ("thickness", ours.thickness, 39),
        ("extrusion x", ours.extrusion.x, 210),
        ("extrusion y", ours.extrusion.y, 220),
        ("extrusion z", ours.extrusion.z, 230),
    ] {
        t.real(what, v, real(code));
    }
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "SHAPE",
            class: None,
            kind: Some(ObjectType::Shape),
            dxf_kind: "SHAPE",
            decodes_on: |_| true,
            note: "; the shape number has no DXF group (the DXF holds the name)",
        },
        compare,
    )
}
