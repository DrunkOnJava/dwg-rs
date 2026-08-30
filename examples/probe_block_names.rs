//! Every `TV` slot of every BLOCK_HEADER and BLOCK record, in order,
//! next to the name each decoder returns.
//!
//! # What this settles (#70)
//!
//! A block's name is written twice in an R2007+ drawing: once on the
//! `BLOCK_HEADER` table entry that owns the definition, and once on the
//! `BLOCK` sentinel entity that opens the definition's entity sublist.
//! Both live in their record's string stream (§19.1), so a decoder that
//! reads the wrong slot still returns a well-formed string — the two
//! names simply disagree, and neither read errors.
//!
//! This probe reads each record's string stream *positionally* — from
//! the bit [`dwg::string_stream::locate`] reports, every slot in turn —
//! and prints it next to the record's own decode, so a disagreement can
//! be attributed to a slot rather than guessed at. The trailing table
//! prints the join [`dwg::graph::resolve_block_names`] performs: the
//! BLOCK_HEADER's stored stem, the sentinel handle its handle stream
//! names, and the full name that sentinel carries.
//!
//! ```sh
//! cargo run --release --example probe_block_names -- samples/sample_AC1032.dwg
//! ```

use dwg::entities::DecodedEntity;
use dwg::error::Result;
use dwg::string_stream::{self, StringReader};
use dwg::{DwgFile, ObjectType};

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg>");
    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file
        .all_objects()
        .expect("this version has no object-stream walk")?;

    println!("file    : {path}");
    println!("version : {version}");
    println!();

    for object in &objects {
        if !matches!(object.kind, ObjectType::BlockHeader | ObjectType::Block) {
            continue;
        }
        let kind = if object.kind == ObjectType::Block {
            "BLOCK       "
        } else {
            "BLOCK_HEADER"
        };
        let located = string_stream::locate(&object.raw, version);
        let decoded = match dwg::entities::decode_from_raw(object, version) {
            DecodedEntity::Block(b) => b.name,
            DecodedEntity::BlockRecord(b) => b.header.name,
            other => format!("<{other:?}>"),
        };
        print!(
            "{kind} handle {:>6}  stream {:?}  decoded {decoded:?}  slots: ",
            object.handle.value,
            located.map(|s| (s.start_bit, s.end_bit)),
        );
        match located {
            Some(stream) => {
                let mut reader = StringReader::new(&object.raw, stream)?;
                let mut slots: Vec<String> = Vec::new();
                while !reader.is_exhausted() {
                    match reader.read_tv() {
                        Ok(s) => slots.push(s),
                        Err(e) => {
                            slots.push(format!("<err: {e}>"));
                            break;
                        }
                    }
                }
                println!("{slots:?}");
            }
            None => println!("(no string stream)"),
        }
    }

    println!();
    println!(
        "{:<8} {:<24} {:>10}  resolved name",
        "header", "stored stem", "sentinel"
    );
    for (handle, name) in dwg::graph::resolve_block_names(&objects, version) {
        let stem = objects
            .iter()
            .find(|o| o.handle.value == handle)
            .map(|o| match dwg::entities::decode_from_raw(o, version) {
                DecodedEntity::BlockRecord(b) => (
                    b.header.name,
                    dwg::tables::block_record::block_sentinel_handle_of(o, version),
                ),
                _ => (String::new(), None),
            })
            .unwrap_or_default();
        println!(
            "{handle:<8} {:<24} {:>10}  {name}",
            stem.0,
            stem.1.map(|h| h.to_string()).unwrap_or_else(|| "-".into())
        );
    }
    Ok(())
}
