//! Compare every symbol-table entry's object prefix with AutoCAD's DXF
//! export of the same drawing (#40).
//!
//! For each table entry the probe reads the common data that comes before
//! the record's own fields, as §20.4.x prints it for R2000 and later:
//!
//! ```text
//! EED                 (size, handle, bytes) until a zero size
//! BL  num_reactors
//! B   no_xdictionary  R2004+
//! B   has_ds_data     R2013+
//! ```
//!
//! and then the shared entry flags. Before R2007 those are inline:
//! `TV name, B 64-flag, BS xrefindex+1, B Xdep`. From R2007 the name
//! is in the string stream and two bits remain, read here as
//! `B 64-flag, B Xdep`.
//!
//! The values are matched by handle (group 5, or 105 on DIMSTYLE)
//! against the twin's record:
//!
//! - the reactor count against the number of `330` groups inside the
//!   record's `102 {ACAD_REACTORS` bracket;
//! - the R2004+ no-xdictionary bit against the absence of a
//!   `102 {ACAD_XDICTIONARY` bracket. A bracket whose dictionary is not
//!   an object of the drawing was added by the export (AutoCAD writes
//!   round-trip xrecords into DXF saved for an older release), so it is
//!   counted apart rather than as a difference;
//! - Xdep against bit 16 of the record's first group `70` (not on
//!   BLOCK_RECORD, whose `70` holds its insertion units);
//! - the inline name (before R2007) against group `2`, without the
//!   `TV`'s terminating NUL. The export numbers anonymous block names
//!   (`*D` is written `*D3`, `*Paper_Space` `*Paper_Space0`), so a name
//!   starting `*` also agrees when the twin's is it plus digits.
//!
//! Usage:
//!
//! ```sh
//! cargo run --release --example probe_table_prefix -- <file.dwg> <twin.dxf>
//! ```
//!
//! Exits non-zero if any compared value differs or if no entry has a twin.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::process::ExitCode;

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::object::body_cursor;
use dwg::{DwgFile, ObjectType, Version};

#[derive(Default)]
struct Twin {
    kind: String,
    reactors: usize,
    xdictionary: Option<u64>,
    flags_70: Option<i64>,
    name: Option<String>,
}

fn read_twins(path: &str) -> HashMap<u64, Twin> {
    let bytes = std::fs::read(path).expect("read the DXF twin");
    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut out = HashMap::new();
    let mut handle: Option<u64> = None;
    let mut twin = Twin::default();
    let mut bracket: Option<String> = None;
    let mut i = 0;
    while i + 1 < lines.len() {
        let code = lines[i].trim();
        let value = lines[i + 1].trim_end_matches('\r');
        i += 2;
        if code == "0" {
            if let Some(h) = handle.take() {
                out.insert(h, std::mem::take(&mut twin));
            }
            twin = Twin {
                kind: value.trim().to_string(),
                ..Twin::default()
            };
            bracket = None;
            continue;
        }
        let value_t = value.trim();
        match code {
            "5" | "105" if handle.is_none() => handle = u64::from_str_radix(value_t, 16).ok(),
            "102" if value_t.starts_with('{') => bracket = Some(value_t.to_string()),
            "102" if value_t == "}" => bracket = None,
            "330" if bracket.as_deref() == Some("{ACAD_REACTORS") => twin.reactors += 1,
            "360" if bracket.as_deref() == Some("{ACAD_XDICTIONARY") => {
                twin.xdictionary = u64::from_str_radix(value_t, 16).ok()
            }
            "70" if twin.flags_70.is_none() && bracket.is_none() => {
                twin.flags_70 = value_t.parse().ok()
            }
            "2" if twin.name.is_none() && bracket.is_none() => twin.name = Some(value.to_string()),
            _ => {}
        }
    }
    if let Some(h) = handle {
        out.insert(h, twin);
    }
    out
}

#[derive(Default)]
struct Tally {
    agree: usize,
    differ: usize,
}

impl Tally {
    fn add(&mut self, same: bool) {
        if same {
            self.agree += 1;
        } else {
            self.differ += 1;
        }
    }
}

fn read_inline_tv(c: &mut BitCursor<'_>) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    let mut bytes = Vec::with_capacity(len);
    for _ in 0..len {
        bytes.push(c.read_rc()?);
    }
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    Ok(bytes.iter().map(|&b| b as char).collect())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dwg_path, dxf_path] = args.as_slice() else {
        eprintln!("usage: probe_table_prefix <file.dwg> <twin.dxf>");
        return ExitCode::from(2);
    };
    let file = DwgFile::open(dwg_path).expect("open the drawing");
    let version = file.version();
    if !matches!(
        version,
        Version::R2000
            | Version::R2004
            | Version::R2007
            | Version::R2010
            | Version::R2013
            | Version::R2018
    ) {
        eprintln!("{version}: the prefix measured here is R2000 and later");
        return ExitCode::from(2);
    }
    let twins = read_twins(dxf_path);
    let objects = file
        .all_objects()
        .expect("walk the object stream")
        .expect("an object stream");
    let in_drawing: HashSet<u64> = objects.iter().map(|o| o.handle.value).collect();

    let (mut entries, mut matched, mut non_zero, mut with_xdic, mut export_only) = (0, 0, 0, 0, 0);
    let mut reactors = Tally::default();
    let mut xdictionary = Tally::default();
    let mut xdep = Tally::default();
    let mut names = Tally::default();
    let mut flags: BTreeMap<String, usize> = BTreeMap::new();
    let mut errors = 0;

    for object in objects.iter().filter(|o| o.kind.is_table_entry()) {
        entries += 1;
        let read = (|| -> Result<_> {
            let mut c = body_cursor(object, version)?;
            loop {
                let size = c.read_bs_u()? as usize;
                if size == 0 {
                    break;
                }
                let _app = c.read_handle()?;
                for _ in 0..size {
                    c.read_rc()?;
                }
            }
            let num_reactors = c.read_bl()? as usize;
            let no_xdictionary = if version.is_r2004_plus() {
                Some(c.read_b()?)
            } else {
                None
            };
            if matches!(version, Version::R2013 | Version::R2018) {
                let _has_ds_data = c.read_b()?;
            }
            let (name, flag_64, xref_index_plus_1, is_xref_dependent) = if version.is_r2007_plus() {
                let flag_64 = c.read_b()?;
                let is_xref_dependent = c.read_b()?;
                (None, flag_64, None, is_xref_dependent)
            } else {
                let name = read_inline_tv(&mut c)?;
                let flag_64 = c.read_b()?;
                let xref_index_plus_1 = c.read_bs()?;
                let is_xref_dependent = c.read_b()?;
                (
                    Some(name),
                    flag_64,
                    Some(xref_index_plus_1),
                    is_xref_dependent,
                )
            };
            Ok((
                num_reactors,
                no_xdictionary,
                name,
                flag_64,
                xref_index_plus_1,
                is_xref_dependent,
            ))
        })();
        let Ok((num_reactors, no_xdictionary, name, flag_64, xref_index_plus_1, is_xref_dependent)) =
            read
        else {
            errors += 1;
            println!(
                "0x{:X} {:?}: prefix unreadable",
                object.handle.value, object.kind
            );
            continue;
        };
        let key = match xref_index_plus_1 {
            Some(x) => format!("64-flag {flag_64}, xrefindex+1 {x}, Xdep {is_xref_dependent}"),
            None => format!("64-flag {flag_64}, Xdep {is_xref_dependent}"),
        };
        *flags.entry(key).or_default() += 1;

        let Some(twin) = twins.get(&object.handle.value) else {
            continue;
        };
        matched += 1;
        if num_reactors > 0 {
            non_zero += 1;
        }
        let twin_xdictionary = match twin.xdictionary {
            Some(h) if !in_drawing.contains(&h) => {
                export_only += 1;
                false
            }
            Some(_) => {
                with_xdic += 1;
                true
            }
            None => false,
        };
        let same = num_reactors == twin.reactors;
        reactors.add(same);
        if !same {
            println!(
                "0x{:X} {}: {num_reactors} reactors, DXF {}",
                object.handle.value, twin.kind, twin.reactors
            );
        }
        if let Some(missing) = no_xdictionary {
            let same = missing != twin_xdictionary;
            xdictionary.add(same);
            if !same {
                println!(
                    "0x{:X} {}: no-xdictionary bit {missing}, DXF xdictionary {:X?}",
                    object.handle.value, twin.kind, twin.xdictionary
                );
            }
        }
        if object.kind != ObjectType::BlockHeader {
            if let Some(f) = twin.flags_70 {
                let same = is_xref_dependent == (f & 16 != 0);
                xdep.add(same);
                if !same {
                    println!(
                        "0x{:X} {}: Xdep {is_xref_dependent}, DXF 70 = {f}",
                        object.handle.value, twin.kind
                    );
                }
            }
        }
        if let (Some(name), Some(dxf_name)) = (name, twin.name.as_deref()) {
            let same = name == dxf_name
                || (name.starts_with('*')
                    && dxf_name
                        .strip_prefix(name.as_str())
                        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())));
            names.add(same);
            if !same {
                println!(
                    "0x{:X} {}: name {name:?}, DXF {dxf_name:?}",
                    object.handle.value, twin.kind
                );
            }
        }
    }

    let file_name = dwg_path.rsplit('/').next().unwrap_or(dwg_path);
    println!(
        "{file_name} ({version}): {entries} table entries, {matched} with a twin, {errors} unreadable"
    );
    println!(
        "  reactor count  agree {:>4} ({non_zero} non-zero), differ {}",
        reactors.agree, reactors.differ
    );
    if version.is_r2004_plus() {
        println!(
            "  xdictionary    agree {:>4} ({with_xdic} present, {export_only} export-only), differ {}",
            xdictionary.agree, xdictionary.differ
        );
    }
    println!(
        "  Xdep vs 70&16  agree {:>4}, differ {}  (BLOCK_RECORD excluded)",
        xdep.agree, xdep.differ
    );
    if !version.is_r2007_plus() {
        println!(
            "  name vs 2      agree {:>4}, differ {}",
            names.agree, names.differ
        );
    }
    for (bits, n) in &flags {
        println!("  {bits}: {n}");
    }
    let differ = reactors.differ + xdictionary.differ + xdep.differ + names.differ + errors;
    if matched == 0 || differ > 0 {
        println!("RESULT: {differ} difference(s), {matched} matched");
        ExitCode::FAILURE
    } else {
        println!("RESULT: every compared value equals the twin");
        ExitCode::SUCCESS
    }
}
