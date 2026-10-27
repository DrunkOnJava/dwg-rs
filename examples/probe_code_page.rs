//! Does a pre-R2007 string decode, in the code page the file header names, to
//! the text it holds?
//!
//! # What this proves
//!
//! ODA v5.4.1 §3.2.5 says only that the short at 0x13 of the file header "is a
//! raw short indicating the value of the code page"; it prints no table of the
//! values. This probe is where the table in `src/code_page.rs` is measured and
//! kept honest. For every drawing under the corpus directory that predates
//! R2007 it reports three things.
//!
//! 1. **The table.** The header short against the `$DWGCODEPAGE` of the
//!    drawing's DXF export. It fails when the table names a short otherwise, or
//!    when an export names a page the table has no row for.
//! 2. **The census.** Every string the crate decodes (every record through the
//!    path `DwgFile::decoded_entities` takes, the header variables, the class
//!    table and the metadata sections), by release: how many there are and how
//!    many have a byte above 0x7F. Each one that does is listed with its bytes,
//!    what UTF-8 with U+FFFD made of them (the decode before the code page was
//!    applied) and what the code page makes of them, and is looked up among the
//!    text groups of the DXF record of the same handle, with `\U+XXXX` escapes
//!    and `^` control notation undone on both sides (the twin's own bytes are
//!    read in the same code page). The pinned corpus holds none: AutoCAD wrote
//!    the degree sign of `sample_AC1015.dwg`'s MTEXT as the nine ASCII
//!    characters `94\U+00B0`, and its export copies them.
//! 3. **A patched copy.** Because the corpus holds no such string, the path is
//!    proved on a real drawing altered in memory (nothing is written): the
//!    bytes of one MTEXT string and of one table-entry name of an uncompressed
//!    drawing are overwritten, at the bit they sit at, with the bytes 0xE9,
//!    0xB0, 0xC0 and 0xFC. With the header short set to 30 they must read
//!    `é°Àü` (Windows-1252), set to 29 `й°Аь` (Windows-1251), and set to a
//!    number the table lacks as before (U+FFFD) and counted as unknown. The
//!    table-entry name must then reach `dwg-to-dxf`'s TABLES as the `\U+XXXX`
//!    escapes of those characters, the form `src/dxf_header.rs` gives every
//!    pre-R2007 character above 0x7F.
//!
//! The expected characters are the Windows code page charts' own, not the
//! decoder's. R2007 and later store UTF-16 and are only counted.
//!
//! ```sh
//! python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
//! cargo run --release --example probe_code_page -- ../dwg-corpus
//! ```
//!
//! Exit status is non-zero when a header short disagrees with its export, a
//! string with a byte above 0x7F differs from the export's, or a patched copy
//! does not decode as above. `NOT MEASURED` is printed, with a non-zero exit,
//! when there is no pre-R2007 drawing or no patchable string.

#[path = "support/twin.rs"]
mod twin;

use dwg::classes::ClassMap;
use dwg::code_page::{CodePage, Observed, observe};
use dwg::dxf::DxfVersion;
use dwg::entities::{DecodedEntity, decode_from_raw, decode_from_raw_with_class_map};
use dwg::file_writer::{WriterScaffold, assemble_dwg_bytes_with_page};
use dwg::object::RawObject;
use dwg::{DwgFile, ObjectType};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use twin::{Record, Twin};

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() && depth < 4 {
            collect(&path, depth + 1, out)?;
        } else if path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("dwg"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn twin_of(path: &Path, page: CodePage) -> Option<Twin> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let parent = path.parent()?;
    let mut names = vec![format!("{stem}.dxf"), format!("{stem}_ascii.dxf")];
    if let Some(base) = stem.strip_suffix("_R14") {
        names.push(format!("{base}_R12.dxf"));
    }
    let found = names.iter().map(|n| parent.join(n)).find(|p| p.exists())?;
    let bytes = std::fs::read(found).ok()?;
    Some(Twin::parse(page.decode(&bytes).as_bytes()))
}

fn header_code_page(twin: &Twin) -> Option<String> {
    let section = twin
        .records_of("SECTION")
        .into_iter()
        .find(|r| r.first(2).is_some_and(|g| g.value.trim() == "HEADER"))?;
    let at = section
        .groups
        .iter()
        .position(|g| g.code == 9 && g.value.trim() == "$DWGCODEPAGE")?;
    section
        .groups
        .get(at + 1)
        .filter(|g| g.code == 3)
        .map(|g| g.value.trim().to_string())
}

/// A DXF string as the export means it: `\U+XXXX` as the character, `^J` and
/// the like as the control character, `^ ` as a caret.
fn norm(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let escape_at = |i: usize| {
        chars.get(i) == Some(&'\\')
            && chars.get(i + 1) == Some(&'U')
            && chars.get(i + 2) == Some(&'+')
    };
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if escape_at(i) {
            let mut units = Vec::new();
            while escape_at(i) {
                let hex: String = chars.iter().skip(i + 3).take(4).collect();
                match u16::from_str_radix(&hex, 16) {
                    Ok(u) if hex.len() == 4 => {
                        units.push(u);
                        i += 7;
                    }
                    _ => break,
                }
            }
            if units.is_empty() {
                out.push('\\');
                i += 1;
            } else {
                out.extend(char::decode_utf16(units).map(|c| c.unwrap_or('\u{FFFD}')));
            }
        } else if chars[i] == '^' {
            match chars.get(i + 1) {
                Some(' ') => {
                    out.push('^');
                    i += 2;
                }
                Some(&n) if ('@'..='_').contains(&n) => {
                    out.push(char::from(n as u8 - 0x40));
                    i += 2;
                }
                _ => {
                    out.push('^');
                    i += 1;
                }
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// What the export's record holds as text: each group's value, and the MTEXT
/// way of splitting one text over groups 3 and a closing 1.
fn candidates(rec: &Record) -> Vec<String> {
    let mut out: Vec<String> = rec
        .groups
        .iter()
        .map(|g| norm(g.value.trim_end_matches('\r')))
        .collect();
    let joined: String = rec
        .groups
        .iter()
        .filter(|g| g.code == 3)
        .chain(rec.groups.iter().filter(|g| g.code == 1))
        .map(|g| norm(g.value.trim_end_matches('\r')))
        .collect();
    out.push(joined);
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Default)]
struct Tally {
    strings: usize,
    non_ascii: usize,
    unknown_page: usize,
    changed: usize,
    compared: usize,
    new_equal: usize,
    old_equal: usize,
}

/// One decode's strings: tally them, and list and look up each that leaves ASCII.
fn census(
    tally: &mut Tally,
    release: &str,
    name: &str,
    what: &str,
    rec: Option<&Record>,
    seen: &[Observed],
    failed: &mut bool,
) {
    for o in seen {
        tally.strings += 1;
        if o.bytes.is_ascii() {
            continue;
        }
        tally.non_ascii += 1;
        let old = String::from_utf8_lossy(&o.bytes).into_owned();
        let Some(new) = o.text.as_deref() else {
            tally.unknown_page += 1;
            println!(
                "  {release} {name} {what}: page {} not in the table, bytes {} kept as {old:?}",
                o.page.header_value(),
                hex(&o.bytes)
            );
            continue;
        };
        tally.changed += usize::from(new != old);
        let verdict = match rec.map(candidates) {
            Some(c) => {
                tally.compared += 1;
                tally.old_equal += usize::from(c.iter().any(|t| *t == norm(&old)));
                if c.iter().any(|t| *t == norm(new)) {
                    tally.new_equal += 1;
                    "equals the export"
                } else {
                    *failed = true;
                    "DIFFERS from the export"
                }
            }
            None => "no DXF record of this handle",
        };
        println!(
            "  {release} {name} {what}: bytes {} before {old:?} after {new:?} {verdict}",
            hex(&o.bytes),
        );
    }
}

/// Every bit offset at which `needle` sits in `hay`, most significant bit first.
fn bit_find(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    if needle.is_empty() || hay.len() < needle.len() + 1 {
        return found;
    }
    for shift in 0..8usize {
        for i in 0..hay.len() - needle.len() {
            let byte_at = |k: usize| -> u8 {
                if shift == 0 {
                    hay[i + k]
                } else {
                    (hay[i + k] << shift) | (hay[i + k + 1] >> (8 - shift))
                }
            };
            if (0..needle.len()).all(|k| byte_at(k) == needle[k]) {
                found.push(i * 8 + shift);
            }
        }
    }
    found
}

fn put_bits(buf: &mut [u8], bit: usize, value: u8) {
    for k in 0..8 {
        let at = bit + k;
        let mask = 0x80u8 >> (at % 8);
        if value & (0x80 >> k) != 0 {
            buf[at / 8] |= mask;
        } else {
            buf[at / 8] &= !mask;
        }
    }
}

/// The four bytes written over a string's first characters, and what each is
/// in Windows-1252 and in Windows-1251 (the code page charts).
const PATCH: [u8; 4] = [0xE9, 0xB0, 0xC0, 0xFC];
const AS_1252: &str = "é°Àü";
const AS_1251: &str = "й°Аь";

struct Target {
    handle: u64,
    /// The record's kind, by name (a class number is resolved to its DXF class).
    kind: String,
    table_entry: bool,
    text: Vec<u8>,
    at: usize,
}

fn kind_name(raw: &RawObject, classes: Option<&ClassMap>) -> String {
    match (raw.kind, classes) {
        (ObjectType::Custom(code), Some(map)) => map
            .by_type_code(code)
            .map_or_else(|| format!("Custom({code})"), |c| c.dxf_class_name.clone()),
        (kind, _) => format!("{kind:?}"),
    }
}

fn decode_record(
    raw: &RawObject,
    version: dwg::Version,
    classes: Option<&ClassMap>,
) -> Vec<Observed> {
    observe(|| match (classes, raw.kind) {
        (Some(map), ObjectType::Custom(code)) => {
            decode_from_raw_with_class_map(raw, version, map, code)
        }
        _ => decode_from_raw(raw, version),
    })
    .1
}

/// The bytes the object records' bit offsets index: the object stream, which is the
/// whole file up to R2000.
fn stream(file: &DwgFile) -> Vec<u8> {
    file.object_stream()
        .and_then(Result::ok)
        .unwrap_or_default()
}

fn decode_record_entity(
    raw: &RawObject,
    version: dwg::Version,
    classes: Option<&ClassMap>,
) -> DecodedEntity {
    match (classes, raw.kind) {
        (Some(map), ObjectType::Custom(code)) => {
            decode_from_raw_with_class_map(raw, version, map, code)
        }
        _ => decode_from_raw(raw, version),
    }
}

/// For each kind of record the drawing holds, its first string that is `min`
/// bytes or longer, ASCII, and sits once in the object stream.
fn targets(file: &DwgFile, min: usize) -> Vec<Target> {
    let version = file.version();
    let hay = stream(file);
    let Some(Ok(objects)) = file.all_objects() else {
        return Vec::new();
    };
    let classes = file.class_map().and_then(Result::ok);
    let mut out: Vec<Target> = Vec::new();
    for raw in &objects {
        let kind = kind_name(raw, classes.as_ref());
        if out.iter().any(|t| t.kind == kind) {
            continue;
        }
        for o in decode_record(raw, version, classes.as_ref()) {
            if o.bytes.len() >= min && o.bytes.is_ascii() {
                if let [at] = bit_find(&hay, &o.bytes)[..] {
                    out.push(Target {
                        handle: raw.handle.value,
                        kind,
                        table_entry: raw.kind.is_table_entry(),
                        text: o.bytes,
                        at,
                    });
                    break;
                }
            }
        }
    }
    out
}

/// A copy of `file` with the first characters of `target` overwritten and the
/// header short set to `short`.
fn patched(file: &DwgFile, target: &Target, short: u16) -> Option<DwgFile> {
    let mut bytes = stream(file);
    for (k, v) in PATCH.iter().enumerate() {
        put_bits(&mut bytes, target.at + 8 * k, *v);
    }
    if !file.version().is_r2004_family() {
        bytes[0x13..0x15].copy_from_slice(&short.to_le_bytes());
        return DwgFile::from_bytes(bytes).ok();
    }
    // The R2004 family: the DWG writer assembles the file, writing `short` as the
    // header's code page, so the patched string reads in it only if the writer wrote it.
    let mut scaffold = WriterScaffold::new(file.version());
    for section in file.sections() {
        let data = if section.name == "AcDb:AcDbObjects" {
            bytes.clone()
        } else {
            file.read_section(&section.name)?.ok()?
        };
        scaffold.add_section(section.name.clone(), data);
    }
    let built = scaffold.build_sections().ok()?;
    let out =
        assemble_dwg_bytes_with_page(&built, file.version(), CodePage::from_header(short)).ok()?;
    DwgFile::from_bytes(out).ok()
}

/// What the table says `PATCH` plus `rest` reads as under header short `short`.
fn expected(short: u16, rest: &str) -> Option<String> {
    match CodePage::from_header(short) {
        CodePage::Ansi1252 => Some(format!("{AS_1252}{rest}")),
        CodePage::Ansi1251 => Some(format!("{AS_1251}{rest}")),
        CodePage::Unknown(_) => None,
    }
}

/// Read `target` back from the patched copy under header short `short`: whether
/// it is what the code page chart says, and the string as read.
fn read_back(file: &DwgFile, target: &Target, short: u16) -> (bool, String) {
    let Some(copy) = patched(file, target, short) else {
        return (false, "the copy does not open".to_string());
    };
    let Some(Ok(objects)) = copy.all_objects() else {
        return (false, "the copy has no objects".to_string());
    };
    let classes = copy.class_map().and_then(Result::ok);
    let seen = objects
        .iter()
        .find(|r| r.handle.value == target.handle)
        .map(|raw| decode_record(raw, copy.version(), classes.as_ref()))
        .unwrap_or_default();
    let Some(o) = seen.iter().find(|o| o.bytes.starts_with(&PATCH)) else {
        return (false, "the string was not read".to_string());
    };
    let rest = String::from_utf8_lossy(&target.text[PATCH.len()..]).into_owned();
    let shown = match &o.text {
        Some(t) => format!("{t:?}"),
        None => format!("kept as {:?}", String::from_utf8_lossy(&o.bytes)),
    };
    (
        o.page == CodePage::from_header(short) && o.text == expected(short, &rest),
        shown,
    )
}

/// What `dwg-to-dxf` writes (R2000 target, bytes) for the patched string read under
/// header short `short`, and whether its TABLES hold it: a named page writes the
/// bytes of the characters it reads (here the patched bytes themselves, for both
/// pages), a page the table lacks the escapes of what it read.
fn dxf_has_it(file: &DwgFile, target: &Target, short: u16) -> (bool, String) {
    let rest = &target.text[PATCH.len()..];
    let want: Vec<u8> = match CodePage::from_header(short) {
        CodePage::Unknown(_) => {
            String::from_utf8_lossy(&PATCH.iter().chain(rest).copied().collect::<Vec<u8>>())
                .chars()
                .flat_map(|c| {
                    if c.is_ascii() {
                        c.to_string().into_bytes()
                    } else {
                        format!("\\U+{:04X}", u32::from(c)).into_bytes()
                    }
                })
                .collect()
        }
        _ => PATCH.iter().chain(rest).copied().collect(),
    };
    let found = patched(file, target, short)
        .and_then(|copy| dwg::dxf_convert::convert_dwg_to_dxf_bytes(&copy, DxfVersion::R2000).ok())
        .is_some_and(|out| out.windows(want.len()).any(|w| w == want.as_slice()));
    (found, hex(&want))
}

#[derive(Default)]
struct KindTally {
    targets: usize,
    reads_ok: usize,
    reads: usize,
    dxf_ok: usize,
    dxf: usize,
}

/// `encode` is `decode`'s inverse on every byte of each named page, and writes the
/// escape AutoCAD's own saves hold (`\U+2205` for the diameter sign, in MTEXT and in
/// an XRECORD of the R2000 and R2004 saves) for a character a page cannot hold.
fn encode_section(failed: &mut bool) {
    println!("\nthe writers' encoding (CodePage::encode):");
    for page in [CodePage::Ansi1251, CodePage::Ansi1252] {
        let round = (0x80..=0xFFu8)
            .filter(|b| page.encode(&page.decode(&[*b])) == [*b])
            .count();
        *failed |= round != 128;
        println!(
            "  {}: {round} of 128 bytes above 0x7F decode and encode back to themselves",
            page.dxf_name().unwrap_or("?")
        );
    }
    let cases: [(CodePage, &str, &[u8]); 5] = [
        (CodePage::Ansi1252, "\u{2205}45,6", b"\\U+220545,6"),
        (CodePage::Ansi1252, "\u{0439}", b"\\U+0439"),
        (CodePage::Ansi1251, "\u{00E9}", b"\\U+00E9"),
        (CodePage::Unknown(5), "\u{00E9}", b"\\U+00E9"),
        (CodePage::Ansi1252, "94\\U+00B0", b"94\\U+00B0"),
    ];
    for (page, text, want) in cases {
        let got = page.encode(text);
        *failed |= got != want;
        println!(
            "  page {} {text:?} -> {:?} {}",
            page.header_value(),
            String::from_utf8_lossy(&got),
            if got == want { "ok" } else { "FAIL" }
        );
    }
}

/// An R2004-family drawing re-assembled by the DWG writer keeps the header's code
/// page, and with it the text of its strings.
fn rewrite_section(paths: &[PathBuf], failed: &mut bool) {
    println!("\nDwgFile::to_bytes, R2004 family:");
    let (mut total, mut page_kept, mut text_kept, mut pre_r2007, mut refused) = (0, 0, 0, 0, 0);
    for path in paths {
        let Ok(file) = DwgFile::open(path) else {
            continue;
        };
        if !file.version().is_r2004_family() {
            continue;
        }
        let Ok(bytes) = file.to_bytes() else {
            refused += 1;
            continue;
        };
        let Ok(again) = DwgFile::from_bytes(bytes) else {
            println!("  {}: the rewritten file does not open", path.display());
            *failed = true;
            continue;
        };
        total += 1;
        page_kept += usize::from(again.code_page() == file.code_page());
        if !file.version().is_r2007_plus() {
            pre_r2007 += 1;
            let strings = |f: &DwgFile| {
                f.decoded_entities()
                    .and_then(Result::ok)
                    .map(|(entities, _)| format!("{entities:?}"))
            };
            text_kept += usize::from(strings(&file) == strings(&again));
        }
    }
    println!(
        "  {page_kept} of {total} rewritten drawings keep their header's code page; {text_kept} of {pre_r2007} pre-R2007 ones decode to the same entities ({refused} R2004-family drawings are refused by the experimental writer for their section map, as before)"
    );
    *failed |= page_kept != total || text_kept != pre_r2007 || total == 0;
}

/// The text a drawing's MTEXT and TEXT records hold, by handle.
fn texts_by_handle(file: &DwgFile) -> std::collections::BTreeMap<u64, String> {
    let mut out = std::collections::BTreeMap::new();
    let Some(Ok(objects)) = file.all_objects() else {
        return out;
    };
    let classes = file.class_map().and_then(Result::ok);
    for raw in &objects {
        match decode_record_entity(raw, file.version(), classes.as_ref()) {
            DecodedEntity::MText(m) => {
                out.insert(raw.handle.value, m.text);
            }
            DecodedEntity::Text(t) => {
                out.insert(raw.handle.value, t.text);
            }
            _ => {}
        }
    }
    out
}

/// AutoCAD saved `sample_AC1015.dwg` (R2000) and the R2007 and later samples from
/// one drawing, with the same entity handles. Where a text holds the diameter
/// sign U+2205 as the character, the R2000 save holds the escape `\U+2205`: so
/// `CodePage::encode` of the R2007 text, under the R2000 file's own page, must give
/// the bytes of the R2000 text of the same handle.
fn paired_texts_section(paths: &[PathBuf], failed: &mut bool) {
    println!(
        "\nR2007 and later texts encoded in the R2000 save's page, against the R2000 save's bytes:"
    );
    let find = |n: &str| paths.iter().find(|p| p.file_name().is_some_and(|f| f == n));
    let Some(old) = find("sample_AC1015.dwg").and_then(|p| DwgFile::open(p).ok()) else {
        println!("  NOT MEASURED: no sample_AC1015.dwg in the corpus");
        *failed = true;
        return;
    };
    let theirs = texts_by_handle(&old);
    let page = old.code_page();
    for name in [
        "sample_AC1021.dwg",
        "sample_AC1024.dwg",
        "sample_AC1027.dwg",
        "sample_AC1032.dwg",
    ] {
        let Some(new) = find(name).and_then(|p| DwgFile::open(p).ok()) else {
            continue;
        };
        let ours = texts_by_handle(&new);
        let (mut compared, mut equal, mut escaped, mut rewritten) = (0, 0, 0, 0);
        for (handle, text) in &ours {
            let Some(old_text) = theirs.get(handle) else {
                continue;
            };
            // AutoCAD rewrites MTEXT's own formatting between the releases (a line
            // break is `\P` in R2000, a stacked fraction is written another way):
            // that is not a character set matter.
            if text.contains('\n') || text.contains('{') {
                rewritten += 1;
                continue;
            }
            compared += 1;
            let bytes = page.encode(text);
            if bytes == old_text.as_bytes() {
                equal += 1;
                escaped += usize::from(!text.is_ascii());
            } else {
                *failed = true;
                println!(
                    "  {name} #{handle:X}: {text:?} encodes to {:?}, the R2000 save holds {old_text:?}",
                    String::from_utf8_lossy(&bytes)
                );
            }
        }
        println!(
            "  {name}: {equal} of {compared} texts equal the R2000 save's bytes ({escaped} of them hold a character the page lacks, written as \\U+XXXX; {rewritten} more are MTEXT that AutoCAD formatted differently in the two releases)"
        );
        *failed |= compared == 0 || escaped == 0;
    }
}

/// No text of the pinned corpus leaves ASCII in a pre-R2007 DXF, so the bytes the page
/// writer returns must be exactly the text the `String` writer returns, for every
/// drawing and every pre-R2007 target that writes one.
fn bytes_api_section(paths: &[PathBuf], failed: &mut bool) {
    println!("\nconvert_dwg_to_dxf_bytes against convert_dwg_to_dxf:");
    let (mut total, mut same) = (0, 0);
    for path in paths {
        let Ok(file) = DwgFile::open(path) else {
            continue;
        };
        for target in [DxfVersion::R2000, DxfVersion::R2004] {
            let text = dwg::dxf_convert::convert_dwg_to_dxf(&file, target);
            let bytes = dwg::dxf_convert::convert_dwg_to_dxf_bytes(&file, target);
            let (Ok(text), Ok(bytes)) = (text, bytes) else {
                continue;
            };
            total += 1;
            if text.as_bytes() == bytes.as_slice() {
                same += 1;
            } else if total - same <= 3 {
                let at = text
                    .bytes()
                    .zip(bytes.iter().copied())
                    .position(|(a, b)| a != b)
                    .unwrap_or(text.len().min(bytes.len()));
                println!(
                    "  {} {target:?}: {} against {} bytes, first difference at {at}: {:?} against {:?}",
                    path.display(),
                    text.len(),
                    bytes.len(),
                    String::from_utf8_lossy(
                        &text.as_bytes()[at.saturating_sub(30)..(at + 30).min(text.len())]
                    ),
                    String::from_utf8_lossy(
                        &bytes[at.saturating_sub(30)..(at + 30).min(bytes.len())]
                    ),
                );
            }
        }
    }
    println!("  {same} of {total} conversions (R2000 and R2004 targets) write the same bytes");
    *failed |= same != total || total == 0;
}

fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: probe_code_page <corpus-dir>");
        return ExitCode::FAILURE;
    };
    let mut paths = Vec::new();
    if let Err(e) = collect(Path::new(&dir), 0, &mut paths) {
        eprintln!("{dir}: {e}");
        return ExitCode::FAILURE;
    }
    paths.sort();

    let mut failed = false;
    let mut pairs: BTreeMap<(u16, String), usize> = BTreeMap::new();
    let mut unpaired: BTreeMap<(String, u16), usize> = BTreeMap::new();
    let mut by_release: BTreeMap<String, Tally> = BTreeMap::new();
    let mut utf16 = 0usize;
    let mut drawings = 0usize;
    let mut patchable: Vec<(String, DwgFile)> = Vec::new();

    println!("strings with a byte above 0x7F:");
    for path in &paths {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let file = match DwgFile::open(path) {
            Ok(f) => f,
            Err(e) => {
                println!("{name}: open: {e}");
                failed = true;
                continue;
            }
        };
        let version = file.version();
        if version.is_r2007_plus() {
            utf16 += 1;
            continue;
        }
        drawings += 1;
        let release = version.to_string();
        let page = file.code_page();
        let twin = twin_of(path, page);

        match twin.as_ref().and_then(header_code_page) {
            Some(theirs) => {
                *pairs
                    .entry((page.header_value(), theirs.clone()))
                    .or_default() += 1;
                if page.dxf_name() != Some(theirs.as_str()) {
                    println!(
                        "{name}: header short {} is {}, the export's $DWGCODEPAGE is {theirs}",
                        page.header_value(),
                        page.dxf_name().unwrap_or("not in the table"),
                    );
                    failed = true;
                }
            }
            None => {
                *unpaired
                    .entry((release.clone(), page.header_value()))
                    .or_default() += 1
            }
        }

        let tally = by_release.entry(release.clone()).or_default();
        let objects = match file.all_objects() {
            Some(Ok(o)) => o,
            _ => Vec::new(),
        };
        let classes: Option<ClassMap> = file.class_map().and_then(Result::ok);
        for raw in &objects {
            let ((), seen) = observe(|| {
                let _ = match (classes.as_ref(), raw.kind) {
                    (Some(map), ObjectType::Custom(code)) => {
                        decode_from_raw_with_class_map(raw, version, map, code)
                    }
                    _ => decode_from_raw(raw, version),
                };
            });
            let rec = twin.as_ref().and_then(|t| t.record(raw.handle.value));
            let what = format!("#{:X} {:?}", raw.handle.value, raw.kind);
            census(tally, &release, &name, &what, rec, &seen, &mut failed);
        }
        let (_, seen) = observe(|| file.header_vars());
        census(
            tally,
            &release,
            &name,
            "AcDb:Header",
            None,
            &seen,
            &mut failed,
        );
        let (_, seen) = observe(|| file.summary_info());
        census(
            tally,
            &release,
            &name,
            "AcDb:SummaryInfo",
            None,
            &seen,
            &mut failed,
        );
        let (_, seen) = observe(|| file.app_info());
        census(
            tally,
            &release,
            &name,
            "AcDb:AppInfo",
            None,
            &seen,
            &mut failed,
        );
        let (_, seen) = observe(|| file.file_dep_list());
        census(
            tally,
            &release,
            &name,
            "AcDb:FileDepList",
            None,
            &seen,
            &mut failed,
        );
        let (_, seen) = observe(|| file.class_map());
        census(
            tally,
            &release,
            &name,
            "AcDb:Classes",
            None,
            &seen,
            &mut failed,
        );

        if !version.is_r2007_plus() {
            patchable.push((name, file));
        }
    }

    println!("\nheader short against the export's $DWGCODEPAGE (pre-R2007 drawings with a twin):");
    for ((short, name), n) in &pairs {
        println!(
            "  {short:>3}  {name:<10} {n:>3} drawings  table: {}",
            CodePage::from_header(*short).dxf_name().unwrap_or("no row"),
        );
    }
    for ((release, short), n) in &unpaired {
        println!("  {short:>3}  (no export names it) {n:>3} {release} drawings");
    }
    println!("\nstrings decoded, by release:");
    println!(
        "  {:<14} {:>8} {:>10} {:>8} {:>8} {:>8} {:>10} {:>10}",
        "release",
        "strings",
        "non-ASCII",
        "changed",
        "unknown",
        "compared",
        "old equal",
        "new equal"
    );
    let (mut non_ascii, mut compared, mut new_equal) = (0, 0, 0);
    for (release, t) in &by_release {
        println!(
            "  {release:<14} {:>8} {:>10} {:>8} {:>8} {:>8} {:>10} {:>10}",
            t.strings, t.non_ascii, t.changed, t.unknown_page, t.compared, t.old_equal, t.new_equal
        );
        non_ascii += t.non_ascii;
        compared += t.compared;
        new_equal += t.new_equal;
    }
    println!(
        "  {drawings} pre-R2007 drawings read; {utf16} R2007 and later drawings store UTF-16 and are not read here"
    );
    println!(
        "  {new_equal} of {compared} compared strings equal AutoCAD's export; {non_ascii} strings in all leave ASCII"
    );

    println!(
        "\npatched copies (in memory, never written), the drawing's own page then 29, 30, 31:"
    );
    let mut by_kind: BTreeMap<String, KindTally> = BTreeMap::new();
    let mut proved = 0;
    for (name, file) in &patchable {
        let base =
            dwg::dxf_convert::convert_dwg_to_dxf(file, DxfVersion::R2000).unwrap_or_default();
        let own = file.code_page().header_value();
        for t in targets(file, 6) {
            let tally = by_kind.entry(t.kind.clone()).or_default();
            tally.targets += 1;
            proved += 1;
            let detail = name == "sample_AC1015.dwg" && (t.kind == "MText" || t.kind == "Ltype");
            if detail {
                println!(
                    "  {name} #{:X} {} {:?} at bit {}",
                    t.handle,
                    t.kind,
                    String::from_utf8_lossy(&t.text),
                    t.at
                );
            }
            for (k, short) in [own, 29, 30, 31].into_iter().enumerate() {
                let (ok, shown) = read_back(file, &t, short);
                tally.reads += 1;
                tally.reads_ok += usize::from(ok);
                failed |= !ok;
                if detail || !ok {
                    println!(
                        "      {}{} {short}: {shown}",
                        if ok {
                            String::new()
                        } else {
                            format!("FAIL {name} #{:X} {}: ", t.handle, t.kind)
                        },
                        if k == 0 { "own page" } else { "page" },
                    );
                }
            }
            if t.table_entry && base.contains(&*String::from_utf8_lossy(&t.text)) {
                for short in [own, 29, 30, 31] {
                    let (ok, want) = dxf_has_it(file, &t, short);
                    tally.dxf += 1;
                    tally.dxf_ok += usize::from(ok);
                    failed |= !ok;
                    if (detail && short != own) || !ok {
                        println!(
                            "      {}dwg-to-dxf bytes under page {short} hold {want}",
                            if ok {
                                String::new()
                            } else {
                                format!("FAIL {name} #{:X} {}: ", t.handle, t.kind)
                            },
                        );
                    }
                }
            }
        }
    }
    println!(
        "\n  {:<28} {:>7} {:>12} {:>14}",
        "kind of record", "strings", "reads right", "dwg-to-dxf too"
    );
    for (kind, t) in &by_kind {
        let dxf = if t.dxf == 0 {
            "-".to_string()
        } else {
            format!("{} of {}", t.dxf_ok, t.dxf)
        };
        println!(
            "  {kind:<28} {:>7} {:>9} of {:<2} {dxf:>14}",
            t.targets, t.reads_ok, t.reads
        );
    }

    encode_section(&mut failed);
    rewrite_section(&paths, &mut failed);
    paired_texts_section(&paths, &mut failed);
    bytes_api_section(&paths, &mut failed);

    if drawings == 0 || proved == 0 {
        println!("NOT MEASURED: no pre-R2007 drawing with a patchable string");
        return ExitCode::FAILURE;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
