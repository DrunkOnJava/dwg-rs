//! Does the derived MATERIAL field list close on every corpus record, and
//! do its values equal AutoCAD's own DXF export?
//!
//! # What this proves
//!
//! ODA spec v5.4.1 prints no MATERIAL layout, so `src/objects/acad_material.rs`
//! was derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md
//! §7bb). For every MATERIAL of every drawing under the corpus directory this
//! reports whether the decoder closed exactly on the record's data-stream
//! boundary, from R2007 with every string-stream entry consumed, on every
//! release from R14 to R2018, and compares each decoded value with the
//! `AcDbMaterial` whose handle (group 5) matches in AutoCAD's DXF export:
//!
//! - name (1), description (2), diffuse file (3), bump file (8);
//! - colour methods (70, 71, 76) and colours (90, 91, 92, as signed 32-bit);
//! - the five map sources (72, 77, 171, 175, 179);
//! - the six 16-value matrices (43, 47, 49, 142, 144, 147);
//! - gloss (44), opacity (140), bump blend (143), refraction index (145);
//! - the R2007+ tail: 148, 468, 93, 94.
//!
//! A group the DXF leaves out is counted as omitted, not compared against an
//! assumed default, with two measured conventions checked on every record
//! instead: an omitted map source is 1 in the DWG, and an omitted matrix is
//! the identity. Fields no DXF group ever shows are not compared and are
//! listed at the end as unverified by value. R14 has no DXF twin in the
//! corpus, so its records are checked against their boundary alone.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_material -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a record errors or is unhandled, a value
//! differs, a twin lacks the record, or no value was compared at all.

#[path = "support/twin.rs"]
mod twin;

use dwg::entities::DecodedEntity;
use dwg::objects::acad_material::{Material, MaterialColor, MaterialMap};
use std::collections::BTreeMap;
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use twin::{Group, Probe, Record, Tally};

static FIELDS: Mutex<BTreeMap<String, [usize; 3]>> = Mutex::new(BTreeMap::new());

/// Run `f`, then credit what it compared, what differed and what the DXF
/// omitted to `field`.
fn counted(t: &mut Tally, field: &str, f: impl FnOnce(&mut Tally)) {
    let before = [t.compared, t.differ, t.omitted];
    f(t);
    if let Ok(mut fields) = FIELDS.lock() {
        let row = fields.entry(field.to_string()).or_default();
        row[0] += t.compared - before[0];
        row[1] += t.differ - before[1];
        row[2] += t.omitted - before[2];
    }
}

static OMITTED_SOURCES: AtomicUsize = AtomicUsize::new(0);
static OMITTED_SOURCES_ONE: AtomicUsize = AtomicUsize::new(0);
static OMITTED_MATRICES: AtomicUsize = AtomicUsize::new(0);
static OMITTED_MATRICES_IDENTITY: AtomicUsize = AtomicUsize::new(0);

fn fail(t: &mut Tally, what: &str, detail: String) {
    t.compared += 1;
    t.differ += 1;
    println!("    {what}: {detail}");
}

fn color(t: &mut Tally, rec: &Record, what: &str, ours: &MaterialColor, method: u16, word: u16) {
    let int = |code: u16| rec.first(code).and_then(Group::int);
    counted(t, &format!("{what} method (group {method})"), |t| {
        t.int(
            &format!("{what} method"),
            i64::from(ours.method),
            int(method),
        );
    });
    counted(t, &format!("{what} colour (group {word})"), |t| {
        match (ours.color, int(word)) {
            (Some(c), theirs) => t.int(&format!("{what} colour"), i64::from(c as i32), theirs),
            (None, Some(theirs)) => fail(
                t,
                &format!("{what} colour"),
                format!("dwg has none, dxf {theirs}"),
            ),
            (None, None) => {}
        }
    });
}

fn identity(m: &[f64; 16]) -> bool {
    m.iter()
        .enumerate()
        .all(|(i, v)| *v == if i % 5 == 0 { 1.0 } else { 0.0 })
}

fn map(
    t: &mut Tally,
    rec: &Record,
    what: &str,
    ours: &MaterialMap,
    source: Option<u16>,
    matrix: u16,
) {
    if let Some(code) = source {
        counted(t, &format!("{what} source (group {code})"), |t| {
            match rec.first(code).and_then(Group::int) {
                Some(theirs) => t.int(
                    &format!("{what} source"),
                    i64::from(ours.source),
                    Some(theirs),
                ),
                None => {
                    t.omitted += 1;
                    OMITTED_SOURCES.fetch_add(1, Ordering::Relaxed);
                    if ours.source == 1 {
                        OMITTED_SOURCES_ONE.fetch_add(1, Ordering::Relaxed);
                    } else {
                        fail(
                            t,
                            &format!("{what} source"),
                            format!("dxf omits group {code}, dwg holds {}", ours.source),
                        );
                    }
                }
            }
        });
    }
    let theirs: Vec<f64> = rec
        .groups
        .iter()
        .filter(|g| g.code == matrix)
        .filter_map(Group::real)
        .collect();
    counted(
        t,
        &format!("{what} matrix (group {matrix})"),
        |t| match theirs.len() {
            16 => {
                for (i, (a, b)) in ours.matrix.iter().zip(&theirs).enumerate() {
                    t.real(&format!("{what} matrix[{i}]"), *a, Some(*b));
                }
            }
            0 => {
                t.omitted += 1;
                OMITTED_MATRICES.fetch_add(1, Ordering::Relaxed);
                if identity(&ours.matrix) {
                    OMITTED_MATRICES_IDENTITY.fetch_add(1, Ordering::Relaxed);
                } else {
                    fail(
                        t,
                        &format!("{what} matrix"),
                        format!("dxf omits group {matrix}, dwg holds {:?}", ours.matrix),
                    );
                }
            }
            n => fail(
                t,
                &format!("{what} matrix"),
                format!("dxf has {n} values of group {matrix}"),
            ),
        },
    );
}

fn file(t: &mut Tally, rec: &Record, what: &str, ours: &MaterialMap, code: u16) {
    let theirs = rec.first(code).map(|g| g.value.as_str());
    counted(t, &format!("{what} file (group {code})"), |t| {
        match (ours.file.as_deref(), theirs) {
            (Some(f), theirs) if !f.is_empty() || theirs.is_some() => {
                t.text(&format!("{what} file"), f, theirs);
            }
            (None, Some(theirs)) => fail(
                t,
                &format!("{what} file"),
                format!("dwg has none, dxf {theirs:?}"),
            ),
            _ => {}
        }
    });
}

fn values(m: &Material, rec: &Record, t: &mut Tally) {
    let real = |code: u16| rec.first(code).and_then(Group::real);
    let int = |code: u16| rec.first(code).and_then(Group::int);
    counted(t, "name (group 1)", |t| {
        t.text("name", &m.name, rec.first(1).map(|g| g.value.as_str()));
    });
    counted(t, "description (group 2)", |t| {
        t.text(
            "description",
            &m.description,
            rec.first(2).map(|g| g.value.as_str()),
        );
    });
    color(t, rec, "ambient", &m.ambient, 70, 90);
    color(t, rec, "diffuse", &m.diffuse, 71, 91);
    color(t, rec, "specular", &m.specular, 76, 92);
    map(t, rec, "diffuse map", &m.diffuse_map, Some(72), 43);
    map(t, rec, "specular map", &m.specular_map, Some(77), 47);
    map(t, rec, "reflection map", &m.reflection_map, Some(171), 49);
    map(t, rec, "opacity map", &m.opacity_map, Some(175), 142);
    map(t, rec, "bump map", &m.bump_map, Some(179), 144);
    map(t, rec, "refraction map", &m.refraction_map, None, 147);
    file(t, rec, "diffuse", &m.diffuse_map, 3);
    file(t, rec, "bump", &m.bump_map, 8);
    counted(t, "gloss (group 44)", |t| {
        t.real("gloss", m.gloss, real(44))
    });
    counted(t, "opacity (group 140)", |t| {
        t.real("opacity", m.opacity, real(140));
    });
    counted(t, "bump blend (group 143)", |t| {
        t.real("bump blend", m.bump_map.blend, real(143));
    });
    counted(t, "refraction index (group 145)", |t| {
        t.real("refraction index", m.refraction_index, real(145));
    });
    match &m.tail {
        Some(tail) => {
            counted(t, "tail BD (group 148)", |t| {
                t.real("group 148", tail.dxf_148, real(148));
            });
            counted(t, "tail BD (group 468)", |t| {
                t.real("group 468", tail.dxf_468, real(468));
            });
            counted(t, "tail BS (group 93)", |t| {
                t.int("group 93", i64::from(tail.dxf_93), int(93));
            });
            counted(t, "tail BS (group 94)", |t| {
                t.int("group 94", i64::from(tail.dxf_94), int(94));
            });
        }
        None => {
            for code in [93, 94] {
                if let Some(theirs) = int(code) {
                    fail(
                        t,
                        &format!("group {code}"),
                        format!("dwg has no tail, dxf {theirs}"),
                    );
                }
            }
            for code in [148, 468] {
                if let Some(theirs) = real(code) {
                    fail(
                        t,
                        &format!("group {code}"),
                        format!("dwg has no tail, dxf {theirs}"),
                    );
                }
            }
        }
    }
}

fn compare(decoded: &DecodedEntity, rec: Option<&Record>, t: &mut Tally) -> bool {
    let DecodedEntity::Material(ours) = decoded else {
        return false;
    };
    if let Some(rec) = rec {
        values(ours, rec, t);
    }
    true
}

fn main() -> ExitCode {
    let code = twin::run(
        &Probe {
            tag: "MATERIAL",
            class: Some("MATERIAL"),
            kind: None,
            dxf_kind: "MATERIAL",
            decodes_on: |_| true,
            note: "; unverified by value (no DXF group in any record): the three colour factors, \
                   the blend of five maps, the three RC after each blend, the two zero tail slots, \
                   the matrix and file of every map the DXF never gives a non-default value, \
                   and which of ambient, diffuse and specular owns which of groups 70/71/76 and 90/91/92",
        },
        compare,
    );
    println!();
    println!("field                          compared  differ  omitted");
    if let Ok(fields) = FIELDS.lock() {
        for (field, row) in fields.iter() {
            println!("{field:<30} {:>8}  {:>6}  {:>7}", row[0], row[1], row[2]);
        }
    }
    println!(
        "measured conventions: {} of {} omitted map sources are 1 in the DWG; {} of {} omitted \
         matrices are the identity",
        OMITTED_SOURCES_ONE.load(Ordering::Relaxed),
        OMITTED_SOURCES.load(Ordering::Relaxed),
        OMITTED_MATRICES_IDENTITY.load(Ordering::Relaxed),
        OMITTED_MATRICES.load(Ordering::Relaxed),
    );
    code
}
