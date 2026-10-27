//! Does `write_class_map` reproduce the `AcDb:Classes` bytes AutoCAD wrote?
//!
//! # What this proves
//!
//! For every drawing it is given, the probe reads the `AcDb:Classes` section
//! as stored (decompressed on R2004+), parses it with [`ClassMap::parse`],
//! writes the table back with [`write_class_map`] for the drawing's own
//! release and compares the two byte strings. A drawing **matches** when
//! they are identical: the sentinel, the size words, the bit-packed class
//! list (and, from R2007, its string block and string-stream trailer), the
//! CRC and the end sentinel. This is the evidence for #45 (the writer was
//! "not a byte-exact AutoCAD emitter"): the writer is byte-exact on exactly
//! the drawings this probe says match, and where it is not, the probe names
//! the first differing byte, the part of the section it falls in and the two
//! values.
//!
//! The layout follows ODA specification v5.4.1 §10.1 (R13 to R15) and §10.2
//! (R18 and later), with the measurements recorded in `src/classes.rs`.
//!
//! # How to verify
//!
//! ```bash
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus      # the pinned drawings
//! cargo run --release --example probe_class_roundtrip -- ../dwg-corpus
//! cargo run --release --example probe_class_roundtrip -- ../dwg-corpus --strict
//! ```
//!
//! Arguments are files or directories (searched recursively for `.dwg`, in
//! any case). Each drawing gets one line; a summary by release follows. The
//! pre-R2007 class names are read and written under the code page the
//! drawing's own header names ([`DwgFile::code_page`]), so a non-ASCII name
//! tests the writer and not a mismatch between two encodings.
//!
//! A drawing with no `AcDb:Classes` section at all (the canonical fixtures of
//! `tests/fixtures/canonical` on R2004 and later) is counted as having none.
//! Every other way a drawing can fail to be compared is a **failure**, counted
//! on its own: the file cannot be opened, the section is listed but cannot be
//! read, [`ClassMap::parse`] errors, or it finds no class in a section that
//! holds bytes. With `--strict` the exit code is 1 on any difference, any
//! failure, and when no drawing matched (a run that proves nothing is not a
//! pass), so a parser regression that empties every class map turns a strict
//! run red instead of leaving it green.

use dwg::bitwriter::BitWriter;
use dwg::classes::{ClassMap, write_class_map};
use dwg::code_page::with_code_page;
use dwg::{DwgFile, Version};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The 16-byte sentinel that closes a class section (§10.1, §10.2).
const END_SENTINEL: [u8; 16] = [
    0x72, 0x5E, 0x3B, 0x47, 0x3B, 0x56, 0x07, 0x3A, 0x3F, 0x23, 0x0B, 0xA0, 0x18, 0x30, 0x49, 0x75,
];

/// How one drawing's class section compared.
enum Outcome {
    /// No `AcDb:Classes` section in the drawing.
    NoSection,
    /// The section is listed but could not be compared: it cannot be read,
    /// [`ClassMap::parse`] errors, or it finds no class in it. The text says which.
    Failed(String),
    /// The rewritten bytes equal the stored ones.
    Match,
    /// They differ; the description says where and how.
    Differ(String),
}

/// Every `.dwg` under `path`, sorted.
fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        children.sort();
        for child in children {
            collect(&child, out);
        }
    } else if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dwg"))
    {
        out.push(path.to_path_buf());
    }
}

/// Which part of the section byte `at` of the stored bytes falls in.
fn region(stored: &[u8], at: usize, version: Version) -> String {
    let crc_at = stored
        .windows(16)
        .rposition(|w| w == END_SENTINEL)
        .map(|end| end.saturating_sub(2));
    let stream_at = match version {
        Version::R14 | Version::R2000 | Version::R2004 => 20,
        Version::R2007 => 24,
        _ => 28,
    };
    if at < 16 {
        "the start sentinel".into()
    } else if at < 20 {
        "the size in bytes".into()
    } else if at < stream_at {
        "the size words before the class list".into()
    } else if crc_at.is_some_and(|c| at < c) {
        format!("the class list, byte {} of it", at - stream_at)
    } else if crc_at.is_some_and(|c| at < c + 2) {
        "the CRC".into()
    } else if crc_at.is_some_and(|c| at < c + 18) {
        "the end sentinel".into()
    } else {
        "the bytes after the end sentinel".into()
    }
}

/// Compare the stored class section of `file` with the rewritten one.
fn compare(file: &DwgFile) -> Outcome {
    if file.section_by_name("AcDb:Classes").is_none() {
        return Outcome::NoSection;
    }
    let stored = match file.read_section("AcDb:Classes") {
        Some(Ok(bytes)) => bytes,
        Some(Err(e)) => return Outcome::Failed(format!("section cannot be read: {e}")),
        None => return Outcome::Failed("section is listed but the reader returns none".into()),
    };
    let version = file.version();
    let page = file.code_page();
    let map = match with_code_page(page, || ClassMap::parse(&stored, version)) {
        Ok(map) => map,
        Err(e) => return Outcome::Failed(format!("classes do not parse: {e}")),
    };
    if map.classes.is_empty() {
        return Outcome::Failed(format!(
            "no class parsed from a section of {} bytes",
            stored.len()
        ));
    }
    let Ok(written) = with_code_page(page, || {
        write_class_map(&map, &mut BitWriter::new(), version)
    }) else {
        return Outcome::Differ("write_class_map failed".into());
    };
    if written == stored {
        return Outcome::Match;
    }
    let common = stored.len().min(written.len());
    let at = (0..common)
        .find(|&i| stored[i] != written[i])
        .unwrap_or(common);
    let mut text = format!(
        "{} classes, stored {} bytes, written {} bytes; first difference at byte {at}, in {}",
        map.classes.len(),
        stored.len(),
        written.len(),
        region(&stored, at, version),
    );
    match (stored.get(at), written.get(at)) {
        (Some(s), Some(w)) => text.push_str(&format!(": stored {s:#04x}, written {w:#04x}")),
        (Some(s), None) => text.push_str(&format!(": stored {s:#04x}, written ends")),
        (None, Some(w)) => text.push_str(&format!(": stored ends, written {w:#04x}")),
        (None, None) => {}
    }
    Outcome::Differ(text)
}

fn main() -> ExitCode {
    let mut paths = Vec::new();
    let mut strict = false;
    for arg in std::env::args().skip(1) {
        if arg == "--strict" {
            strict = true;
        } else {
            collect(Path::new(&arg), &mut paths);
        }
    }
    if paths.is_empty() {
        eprintln!("usage: probe_class_roundtrip <file.dwg|dir>... [--strict]");
        return ExitCode::FAILURE;
    }

    // release -> (match, differ, failed, no section)
    let mut by_release: BTreeMap<String, [usize; 4]> = BTreeMap::new();
    let mut cannot_open = 0usize;
    for path in &paths {
        let name = path.display().to_string();
        let file = match DwgFile::open(path) {
            Ok(f) => f,
            Err(e) => {
                println!("{name:<60} cannot open: {e}");
                cannot_open += 1;
                continue;
            }
        };
        let release = file.version().release().to_string();
        let counts = by_release.entry(release.clone()).or_default();
        match compare(&file) {
            Outcome::Match => {
                counts[0] += 1;
                println!("{name:<60} {release:<6} MATCH");
            }
            Outcome::Differ(why) => {
                counts[1] += 1;
                println!("{name:<60} {release:<6} DIFFER {why}");
            }
            Outcome::Failed(why) => {
                counts[2] += 1;
                println!("{name:<60} {release:<6} FAILED {why}");
            }
            Outcome::NoSection => {
                counts[3] += 1;
                println!("{name:<60} {release:<6} no AcDb:Classes section");
            }
        }
    }

    println!();
    println!("release  match  differ    failed  no section");
    let mut total = [0usize; 4];
    for (release, c) in &by_release {
        println!(
            "{release:<8} {:>5}  {:>6}  {:>8}  {:>10}",
            c[0], c[1], c[2], c[3]
        );
        for (t, v) in total.iter_mut().zip(c) {
            *t += v;
        }
    }
    println!(
        "TOTAL    {:>5}  {:>6}  {:>8}  {:>10}   of {} drawings, {} could not be opened",
        total[0],
        total[1],
        total[2],
        total[3],
        paths.len(),
        cannot_open
    );
    if strict && (total[1] + total[2] + cannot_open > 0 || total[0] == 0) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
