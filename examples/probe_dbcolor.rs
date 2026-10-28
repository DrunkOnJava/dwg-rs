//! Does the derived DBCOLOR field list close on every corpus record, and do
//! its values equal AutoCAD's DXF export?
//!
//! ODA spec v5.4.1 prints no layout for DBCOLOR; `src/objects/small_classes.rs`
//! was derived from AutoCAD's output (CLEANROOM.md, 2026-10-26; ARCHITECTURE.md
//! §7be). For every DBCOLOR of every drawing under the corpus directory this
//! reports whether the decoder closed exactly on the record's data-stream
//! boundary (from R2007 with the string stream consumed) and compares the true colour (group 420, the low 24 bits of the colour word), the colour word (xdata 1071, signed) and the book and name (group 430, `book$name`) from R2004; R14 and R2000 write none of them, so their record is checked against its boundary alone. The leading `BS` is not compared: group 62 says 42 where it reads 0
//! with the `DBCOLOR` whose handle (group 5) matches in the DXF twin. A group the
//! DXF leaves out is counted as omitted, not compared against an assumed
//! default.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_dbcolor -- ../dwg-corpus
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
    let DecodedEntity::DbColor(ours) = decoded else {
        return false;
    };
    let (Some(rec), Some(word)) = (rec, ours.color_word) else {
        return true;
    };
    t.int(
        "true colour",
        i64::from(word & 0x00FF_FFFF),
        rec.first(420).and_then(Group::int),
    );
    t.int(
        "colour word",
        i64::from(word as i32),
        rec.first(1071).and_then(Group::int),
    );
    let book = format!(
        "{}${}",
        ours.book.as_deref().unwrap_or(""),
        ours.name.as_deref().unwrap_or("")
    );
    t.text(
        "book and name",
        &book,
        rec.first(430).map(|g| g.value.as_str()),
    );
    true
}

fn main() -> ExitCode {
    twin::run(
        &Probe {
            tag: "DBCOLOR",
            class: Some("DBCOLOR"),
            kind: None,
            dxf_kind: "DBCOLOR",
            decodes_on: |_| true,
            note: "; the leading BS from R2004 is unverified (group 62 is 42, it reads 0)",
        },
        compare,
    )
}
