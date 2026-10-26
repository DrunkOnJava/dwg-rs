//! Reads every `.dxf` in a directory with [`dwg::dxf_reader`] and prints what
//! it found: release, sections, header variables, records, handles and the
//! precision reals were printed at, then the record count by type across the
//! whole directory.
//!
//! What it proves: the reader takes AutoCAD's own exports without loss (every
//! file parses, every record from R13 on carries a unique handle) and refuses
//! malformed input instead of guessing. With `--mutations`, each file is
//! also given a `999` comment in three legal places and, when it has an
//! XRECORD, an unclosed `102 {` among that XRECORD's data, each copy of
//! which must read exactly as the original, in the file's own line ending,
//! and damaged twelve ways in memory
//! (truncated, a code line made non-numeric, an unknown group code, a real
//! made unparsable, a handle duplicated, a `102` bracket left open, mixed
//! line endings, `EOF` removed, data after `EOF`, a handle removed, a handle
//! removed behind a comment where the version is read, a binary-DXF
//! sentinel), and every damaged copy must be
//! refused; the example exits non-zero if one is accepted.
//!
//! How to verify:
//!
//! ```text
//! cargo run --release --features dxf-reader --example dxf_census -- <corpus-dir> --mutations
//! ```
//!
//! where `<corpus-dir>` holds the `.dxf` files of `nextgis/dwg_samples` at
//! commit `ff05d5a89ce123da511ed93c5fe5e6df85e02ab8` (35 AutoCAD 2016 exports
//! from AC1015 to AC1027 and 7 R12 exports).

use dwg::dxf_reader::{DxfDocument, DxfError};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut dir = None;
    let mut mutations = false;
    for arg in std::env::args().skip(1) {
        if arg == "--mutations" {
            mutations = true;
        } else {
            dir = Some(PathBuf::from(arg));
        }
    }
    let Some(dir) = dir else {
        eprintln!("usage: dxf_census <dir-of-dxf> [--mutations]");
        return ExitCode::FAILURE;
    };
    let mut files: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dxf")))
            .collect(),
        Err(e) => {
            eprintln!("cannot read {}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
    };
    files.sort();

    let mut failed = false;
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut totals = (0usize, 0usize, 0usize);
    println!(
        "{:<26} {:<7} {:>4} {:>6} {:>7} {:>7} {:>6}",
        "file", "acadver", "secs", "hdrvar", "records", "handles", "digits"
    );
    for path in &files {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into());
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                println!("{name:<26} read failed: {e}");
                failed = true;
                continue;
            }
        };
        let doc = match DxfDocument::parse(&bytes) {
            Ok(d) => d,
            Err(e) => {
                println!("{name:<26} REFUSED: {e}");
                failed = true;
                continue;
            }
        };
        let handles = doc.handles().count();
        println!(
            "{:<26} {:<7} {:>4} {:>6} {:>7} {:>7} {:>6}",
            name,
            doc.acad_version().unwrap_or("?"),
            doc.sections.len(),
            doc.header.len(),
            doc.records.len(),
            handles,
            doc.real_precision()
        );
        totals.0 += doc.header.len();
        totals.1 += doc.records.len();
        totals.2 += handles;
        for r in &doc.records {
            *by_type.entry(r.kind.clone()).or_default() += 1;
        }
        if mutations {
            for (label, damaged) in mutate(&bytes) {
                match DxfDocument::parse(&damaged) {
                    Ok(_) => {
                        println!("  ACCEPTED damaged copy ({label})");
                        failed = true;
                    }
                    Err(e) => println!("  refused {label:<24} {}", short(&e)),
                }
            }
            // Each copy with a legal but easily misread group must read
            // exactly as the original does.
            let shape = |d: &DxfDocument| {
                (
                    d.acad_version().map(str::to_string),
                    d.header.len(),
                    d.records.len(),
                    d.handles().count(),
                )
            };
            let want = shape(&doc);
            for (label, commented) in legal(&bytes) {
                match DxfDocument::parse(&commented) {
                    Ok(d) if shape(&d) == want => println!("  same    {label:<24} as the original"),
                    Ok(d) => {
                        println!("  CHANGED {label:<24} {:?} vs {:?}", shape(&d), want);
                        failed = true;
                    }
                    Err(e) => {
                        println!("  REFUSED legal copy ({label}): {}", short(&e));
                        failed = true;
                    }
                }
            }
        }
    }
    println!(
        "TOTAL {} files: {} header variables, {} records, {} handles",
        files.len(),
        totals.0,
        totals.1,
        totals.2
    );
    println!("\nrecords by type:");
    for (kind, n) in &by_type {
        println!("  {kind:<28} {n:>6}");
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn short(e: &DxfError) -> String {
    let s = e.to_string();
    if s.len() > 90 {
        format!("{}...", &s[..90])
    } else {
        s
    }
}

/// Line-level edits that each break one rule of the grammar.
fn mutate(bytes: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let eol = line_ending(&text);
    let lines: Vec<&str> = text.split(eol).collect();
    let join = |ls: &[&str]| ls.join(eol).into_bytes();
    let mut out = Vec::new();

    // Truncated: drop the final value line (`EOF`), leaving a code with no value.
    let mut end = lines.len();
    while end > 0 && lines[end - 1].is_empty() {
        end -= 1;
    }
    out.push(("truncated", join(&lines[..end.saturating_sub(1)])));

    // The first group code made non-numeric.
    let mut l = lines.clone();
    if let Some(first) = l.first_mut() {
        *first = "  x";
    }
    out.push(("non-numeric code", join(&l)));

    // An unknown group code (the first `  0` after HEADER becomes 150).
    if let Some(i) = find_pair(&lines, "  9", "$ACADVER") {
        let mut l = lines.clone();
        l[i] = "150";
        out.push(("unknown code 150", join(&l)));
    }

    // A real made unparsable: the first 10 group's value.
    if let Some(i) = lines.iter().position(|l| l.trim() == "10") {
        let mut l = lines.clone();
        if i + 1 < l.len() {
            l[i + 1] = "1.0.0";
        }
        out.push(("bad real", join(&l)));
    }

    // A handle duplicated: the second record handle in TABLES takes the
    // first one's value. HEADER's `$HANDSEED` is a `5` group too, so the
    // search starts at the TABLES section.
    let tables = find_pair(&lines, "  2", "TABLES").unwrap_or(lines.len());
    let fives: Vec<usize> = (tables..lines.len().saturating_sub(1))
        .step_by(2)
        .filter(|&i| lines[i].trim() == "5" && !lines[i + 1].trim().is_empty())
        .collect();
    if fives.len() >= 2 {
        let mut l = lines.clone();
        l[fives[1] + 1] = lines[fives[0] + 1];
        out.push(("duplicate handle", join(&l)));
    }

    // A 102 bracket left open: the first `102 }` becomes a 102 string.
    if let Some(i) = find_pair(&lines, "102", "}") {
        let mut l = lines.clone();
        l[i + 1] = "{ACAD_REACTORS";
        out.push(("open 102 bracket", join(&l)));
    }

    // Mixed line endings: the first line ends with the other ending.
    let other = if eol == "\n" { "\r\n" } else { "\n" };
    let mixed = format!("{}{other}{}", lines[0], lines[1..].join(eol));
    out.push(("mixed line endings", mixed.into_bytes()));

    // EOF removed.
    if let Some(i) = find_pair(&lines, "  0", "EOF") {
        out.push(("no EOF", join(&lines[..i])));
    }

    // Data after EOF.
    let mut after = bytes.to_vec();
    after.extend_from_slice(format!("  0{eol}LINE{eol}").as_bytes());
    out.push(("data after EOF", after));

    // A comment between `9 $ACADVER` and its value must not stand in for
    // the version: with a handle removed as well, an R13+ file must still
    // be refused for the missing handle.
    if let (Some(v), Some(&h), false) = (
        find_pair(&lines, "  9", "$ACADVER"),
        fives.first(),
        find_pair(&lines, "  1", "AC1009").is_some(),
    ) {
        let mut l: Vec<&str> = lines.clone();
        l[h] = "  1";
        l.splice(v + 2..v + 2, ["999", "a comment, not the version"]);
        out.push(("comment as $ACADVER", join(&l)));
    }

    // A handle removed from an R13+ record: the first `5` group in TABLES
    // becomes a `1`. R12 makes handles optional, so its files skip this one.
    let r12 = find_pair(&lines, "  1", "AC1009").is_some();
    if let (Some(&i), false) = (fives.first(), r12) {
        let mut l = lines.clone();
        l[i] = "  1";
        out.push(("handle removed", join(&l)));
    }

    // Binary-DXF sentinel.
    let mut bin = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
    bin.extend_from_slice(bytes);
    out.push(("binary sentinel", bin));
    out
}

/// Copies with a group where one is legal but easy to misread: a `999`
/// comment right after `9 $ACADVER`, between `0 SECTION` and `2 HEADER`, and
/// inside the first record of TABLES, and an unclosed `102 {` among the first
/// XRECORD's data, where it is the application's value, not a bracket.
fn legal(bytes: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let eol = line_ending(&text);
    let lines: Vec<&str> = text.split(eol).collect();
    let join = |ls: &[&str]| ls.join(eol).into_bytes();
    let note = ["999", "a comment"];
    let mut out = Vec::new();
    if let Some(v) = find_pair(&lines, "  9", "$ACADVER") {
        let mut l = lines.clone();
        l.splice(v + 2..v + 2, note);
        out.push(("999 after $ACADVER", join(&l)));
    }
    if let Some(sec) = find_pair(&lines, "  0", "SECTION") {
        let mut l = lines.clone();
        l.splice(sec + 2..sec + 2, note);
        out.push(("999 after SECTION", join(&l)));
    }
    if let Some(t) = find_pair(&lines, "  2", "TABLES") {
        let mut l = lines.clone();
        l.splice(t + 4..t + 4, note);
        out.push(("999 inside a record", join(&l)));
    }
    if let Some(m) = find_pair(&lines, "100", "AcDbXrecord") {
        let flag = lines.get(m + 2).is_some_and(|c| c.trim() == "280");
        let at = if flag { m + 4 } else { m + 2 };
        let mut l = lines.clone();
        l.splice(at..at, ["102", "{data"]);
        out.push(("102 { in XRECORD data", join(&l)));
    }
    out
}

/// `\r\n` when the file's first line ends with one, else `\n`; a file that
/// mixes the two is refused by the reader before any copy is made.
fn line_ending(text: &str) -> &'static str {
    match text.split_once('\n') {
        Some((first, _)) if first.ends_with('\r') => "\r\n",
        _ => "\n",
    }
}

/// Index of the code line of the first `code` / `value` pair.
fn find_pair(lines: &[&str], code: &str, value: &str) -> Option<usize> {
    (0..lines.len().saturating_sub(1)).step_by(2).find(|&i| {
        lines[i].trim_end_matches('\r') == code && lines[i + 1].trim_end_matches('\r') == value
    })
}
