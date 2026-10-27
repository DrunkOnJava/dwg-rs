//! Do the derived dynamic-block field lists close on every corpus record, and
//! do their values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for BLOCKGRIPLOCATIONCOMPONENT,
//! BLOCKLINEARGRIP, BLOCKVISIBILITYGRIP, BLOCKLINEARPARAMETER,
//! BLOCKVISIBILITYPARAMETER or BLOCKSCALEACTION; `src/objects/dynamic_block.rs`
//! was derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md
//! §7aw). For every record of the six classes under the corpus directory this
//! reports whether the decoder closed exactly on the record's data-stream
//! boundary (from R2007 with the string stream consumed) and compares every
//! value the DXF writes with the object of the same handle: groups 90, 98, 99,
//! 70, 140, 300, 1071, 91, 92, 93, 280, 281, the points (1010 to 1032), the
//! property entries (170 to 177, 301 to 306), the state entries (303, 94, 95)
//! and the scale entries (70, 71, 92 to 96, 301 to 305). A group the DXF leaves
//! out is counted as omitted, and one convention is measured rather than
//! assumed: where the DWG holds `f64::MAX / 10` (0x7FB9999999999999,
//! the expression component's unset value) the DXF writes `1.797693134862314E+99`. Handles (330 to 333) are in the handle stream and
//! are not compared. R14 has no DXF twin in the corpus, so its records are
//! checked against their boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_dynamic_blocks -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use dwg::objects::dynamic_block::{BlockElement, BlockGrip, DynamicBlockObject, EvalExpr};
use std::process::ExitCode;
use twin::{Group, Probe, Record, Tally};

const CLASSES: [&str; 6] = [
    "BLOCKGRIPLOCATIONCOMPONENT",
    "BLOCKLINEARGRIP",
    "BLOCKVISIBILITYGRIP",
    "BLOCKLINEARPARAMETER",
    "BLOCKVISIBILITYPARAMETER",
    "BLOCKSCALEACTION",
];

struct Dxf<'a>(&'a Record);

impl Dxf<'_> {
    fn nth(&self, code: u16, n: usize) -> Option<&Group> {
        self.0.groups.iter().filter(|g| g.code == code).nth(n)
    }

    fn int(&self, code: u16, n: usize) -> Option<i64> {
        self.nth(code, n).and_then(Group::int)
    }

    fn real(&self, code: u16, n: usize) -> Option<f64> {
        self.nth(code, n).and_then(Group::real)
    }

    fn text(&self, code: u16, n: usize) -> Option<&str> {
        self.nth(code, n).map(|g| g.value.as_str())
    }

    fn point(&self, t: &mut Tally, what: &str, p: &[f64; 3], codes: [u16; 3], n: usize) {
        for (i, axis) in ["x", "y", "z"].iter().enumerate() {
            t.real(&format!("{what} {axis}"), p[i], self.real(codes[i], n));
        }
    }
}

fn eval(d: &Dxf, t: &mut Tally, e: &EvalExpr) {
    t.int("node id (90)", i64::from(e.node_id), d.int(90, 0));
    t.int("eval 98", i64::from(e.dxf_98), d.int(98, 0));
    t.int("eval 99", i64::from(e.dxf_99), d.int(99, 0));
    if let Some(v) = e.value {
        t.int("value type (70)", i64::from(e.value_type), d.int(70, 0));
        if v == f64::MAX / 10.0 {
            t.text(
                "value (140), DBL_MAX / 10",
                "1.797693134862314E+99",
                d.text(140, 0).map(str::trim),
            );
        } else {
            t.real("value (140)", v, d.real(140, 0));
        }
    }
}

fn element(d: &Dxf, t: &mut Tally, e: &BlockElement) {
    t.text("name (300)", &e.name, d.text(300, 0));
    t.int("element 98", i64::from(e.dxf_98), d.int(98, 1));
    t.int("element 99", i64::from(e.dxf_99), d.int(99, 1));
    t.int("element 1071", i64::from(e.dxf_1071), d.int(1071, 0));
}

fn grip(d: &Dxf, t: &mut Tally, g: &BlockGrip) {
    t.int("grip 91", i64::from(g.dxf_91), d.int(91, 0));
    t.int("grip 92", i64::from(g.dxf_92), d.int(92, 0));
    d.point(t, "grip position", &g.position, [1010, 1020, 1030], 0);
    t.int("grip 280", i64::from(g.dxf_280), d.int(280, 0));
    t.int("grip 93", i64::from(g.dxf_93), d.int(93, 0));
}

fn values(object: &DynamicBlockObject, d: &Dxf, t: &mut Tally) {
    match object {
        DynamicBlockObject::GripLocationComponent(o) => {
            eval(d, t, &o.eval);
            t.int("group 91", i64::from(o.dxf_91), d.int(91, 0));
            t.text("name (300)", &o.name, d.text(300, 0));
        }
        DynamicBlockObject::LinearGrip(o) => {
            eval(d, t, &o.eval);
            element(d, t, &o.element);
            grip(d, t, &o.grip);
            d.point(t, "offset", &o.offset, [140, 141, 142], 0);
        }
        DynamicBlockObject::VisibilityGrip(o) => {
            eval(d, t, &o.eval);
            element(d, t, &o.element);
            grip(d, t, &o.grip);
        }
        DynamicBlockObject::LinearParameter(o) => {
            eval(d, t, &o.eval);
            element(d, t, &o.element);
            t.int("parameter 280", i64::from(o.dxf_280), d.int(280, 0));
            t.int("parameter 281", i64::from(o.dxf_281), d.int(281, 0));
            d.point(t, "point 1", &o.point1, [1010, 1020, 1030], 0);
            d.point(t, "point 2", &o.point2, [1011, 1021, 1031], 0);
            t.int(
                "property count (170)",
                o.properties.len() as i64,
                d.int(170, 0),
            );
            for (i, p) in o.properties.iter().enumerate() {
                let n = i as u16;
                t.int("property flag", i64::from(p.flag), d.int(171 + n, 0));
                t.int("property value", i64::from(p.value), d.int(92 + n, 0));
                t.text("property name", &p.name, d.text(301 + n, 0));
            }
            for (i, v) in o.dxf_91.iter().enumerate() {
                t.int("group 91", i64::from(*v), d.int(91, i));
            }
            t.int("group 177", i64::from(o.dxf_177), d.int(177, 0));
            t.text("name (305)", &o.name, d.text(305, 0));
            t.text("description (306)", &o.description, d.text(306, 0));
            t.real("group 140", o.dxf_140, d.real(140, 0));
            t.int("group 96", i64::from(o.dxf_96), d.int(96, 0));
            for (i, code) in [141u16, 142, 143].iter().enumerate() {
                t.real("group 141-143", o.dxf_141_to_143[i], d.real(*code, 0));
            }
            t.int("group 175", i64::from(o.dxf_175), d.int(175, 0));
        }
        DynamicBlockObject::VisibilityParameter(o) => {
            eval(d, t, &o.eval);
            element(d, t, &o.element);
            t.int("parameter 280", i64::from(o.dxf_280), d.int(280, 0));
            t.int("parameter 281", i64::from(o.dxf_281), d.int(281, 0));
            d.point(t, "position", &o.position, [1010, 1020, 1030], 0);
            t.int("group 170", i64::from(o.dxf_170_171[0]), d.int(170, 0));
            t.int("group 171", i64::from(o.dxf_170_171[1]), d.int(171, 0));
            t.int("group 93", i64::from(o.dxf_93), d.int(93, 0));
            t.int(
                "visibility 281",
                i64::from(o.dxf_281_visibility),
                d.int(281, 1),
            );
            t.text("name (301)", &o.name, d.text(301, 0));
            t.text("description (302)", &o.description, d.text(302, 0));
            t.int("group 91", i64::from(o.dxf_91), d.int(91, 0));
            t.int("group 93 second", i64::from(o.dxf_93_second), d.int(93, 1));
            t.int("state count (92)", o.states.len() as i64, d.int(92, 0));
            for (i, s) in o.states.iter().enumerate() {
                t.text("state name", &s.name, d.text(303, i));
                t.int("state 94", i64::from(s.dxf_94), d.int(94, i));
                t.int("state 95", i64::from(s.dxf_95), d.int(95, i));
            }
        }
        DynamicBlockObject::ScaleAction(o) => {
            eval(d, t, &o.eval);
            element(d, t, &o.element);
            d.point(t, "position", &o.position, [1010, 1020, 1030], 0);
            t.int("group 71", i64::from(o.dxf_71), d.int(71, 0));
            t.int("group 70", i64::from(o.dxf_70), d.int(70, 0));
            d.point(t, "base point", &o.base_point, [1011, 1021, 1031], 0);
            t.int("group 92", i64::from(o.dxf_92), d.int(92, 0));
            t.text("name (301)", &o.name_x, d.text(301, 0));
            t.int("group 93", i64::from(o.dxf_93), d.int(93, 0));
            t.text("name (302)", &o.name_y, d.text(302, 0));
            t.int("group 280", i64::from(o.dxf_280), d.int(280, 0));
            d.point(t, "second point", &o.second_point, [1012, 1022, 1032], 0);
            t.int("group 94", i64::from(o.dxf_94), d.int(94, 0));
            t.text("name (303)", &o.name_scale, d.text(303, 0));
            t.int("group 95", i64::from(o.dxf_95), d.int(95, 0));
            t.text("name (304)", &o.name_x_scale, d.text(304, 0));
            t.int("group 96", i64::from(o.dxf_96), d.int(96, 0));
            t.text("name (305)", &o.name_y_scale, d.text(305, 0));
        }
    }
}

fn compare(class: &str, decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::DynamicBlock(object) = decoded else {
        return false;
    };
    let ours = match (class, object.as_ref()) {
        ("BLOCKGRIPLOCATIONCOMPONENT", DynamicBlockObject::GripLocationComponent(_))
        | ("BLOCKLINEARGRIP", DynamicBlockObject::LinearGrip(_))
        | ("BLOCKVISIBILITYGRIP", DynamicBlockObject::VisibilityGrip(_))
        | ("BLOCKLINEARPARAMETER", DynamicBlockObject::LinearParameter(_))
        | ("BLOCKVISIBILITYPARAMETER", DynamicBlockObject::VisibilityParameter(_))
        | ("BLOCKSCALEACTION", DynamicBlockObject::ScaleAction(_)) => object.as_ref(),
        _ => return false,
    };
    if let Some(rec) = rec {
        values(ours, &Dxf(rec), t);
    }
    true
}

fn main() -> ExitCode {
    let mut failed = false;
    for class in CLASSES {
        let code = twin::run(
            &Probe {
                tag: class,
                class: Some(class),
                kind: None,
                dxf_kind: class,
                decodes_on: |_| true,
                note: "; the BL -1 that opens every record, the trailing byte of BLOCKSCALEACTION \
                       and the implied length of the linear parameter's property list are \
                       unverified by value",
            },
            |decoded, rec, t| compare(class, decoded, rec, t),
        );
        if format!("{code:?}") != format!("{:?}", ExitCode::SUCCESS) {
            failed = true;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
