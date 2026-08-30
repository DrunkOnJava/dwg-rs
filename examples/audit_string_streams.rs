//! Which object types carry `TV` fields in the R2007+ string stream?
//!
//! # What this proves
//!
//! From R2007 an object's variable-text fields move out of the data
//! stream into a per-object string stream (§19.1). A decoder that still
//! reads `TV` inline does not error — it returns whatever bits sit at
//! that position, which is why `MTEXT` returned a one-character string
//! for every record before it was ported.
//!
//! This probe walks every object in a file, locates its string stream
//! and prints the strings it holds, grouped by object type. A type
//! whose records carry strings here but whose decoder reads `TV` from
//! the data cursor is silently wrong on that file, and the strings
//! printed are what it should have returned.
//!
//! # How to verify
//!
//! ```sh
//! cargo run --release --example audit_string_streams -- samples/sample_AC1032.dwg
//! ```

use dwg::DwgFile;
use dwg::error::Result;
use dwg::string_stream::{self, StringReader};
use std::collections::BTreeMap;

/// One object type's string-stream census.
#[derive(Default)]
struct Census {
    records: usize,
    with_stream: usize,
    strings: usize,
    samples: Vec<String>,
    kind: String,
}

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg>");
    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file
        .all_objects()
        .expect("this version has no object-stream walk")?;

    let mut by_type: BTreeMap<u16, Census> = BTreeMap::new();
    for object in &objects {
        let census = by_type.entry(object.type_code).or_default();
        census.kind = format!("{:?}", object.kind);
        census.records += 1;
        let Some(stream) = string_stream::locate(&object.raw, version) else {
            continue;
        };
        census.with_stream += 1;
        let Ok(mut reader) = StringReader::new(&object.raw, stream) else {
            continue;
        };
        while !reader.is_exhausted() {
            match reader.read_tv() {
                Ok(s) => {
                    census.strings += 1;
                    if census.samples.len() < 3 {
                        let short: String = s.chars().take(48).collect();
                        census.samples.push(short);
                    }
                }
                Err(_) => break,
            }
        }
    }

    println!("{path}  ({version})");
    println!(
        "{:<6} {:<22} {:>7} {:>7} {:>8}  samples",
        "code", "kind", "records", "streams", "strings"
    );
    for (code, c) in &by_type {
        println!(
            "0x{code:04X} {:<22} {:>7} {:>7} {:>8}  {:?}",
            c.kind, c.records, c.with_stream, c.strings, c.samples
        );
    }
    Ok(())
}
