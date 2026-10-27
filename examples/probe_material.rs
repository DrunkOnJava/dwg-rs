//! MATERIAL: the data-field budget of every record, and a joint-boundary
//! test for a candidate field list (#49, #152).
//!
//! ODA spec v5.4.1 has no §20.4 prescription for MATERIAL (§20.3 lists
//! the class name only), so its field list has to come from the bytes.
//! The probe reads each record's common object data, then runs one
//! candidate field spec against **every** MATERIAL record of every file
//! named on the command line and prints, per record, the `budget` (bits
//! between the end of the common object data and the data-stream
//! boundary), the `delta` where the spec lands relative to that boundary,
//! and the record's remaining string-stream entries. A field list is
//! right only when every record of every file lands at delta 0.
//!
//! # What it shows (measured 2026-10-26, the 19-file corpus)
//!
//! It proves no field list. It pins the budgets a list must fill:
//!
//! | Band | Records per file | Budget, bits | After `TV,TV` |
//! |---|---:|---|---|
//! | R14, R2000, R2004 | 3 (`ByLayer`, `ByBlock`, `Global`) | 1340 / 1340 / 1332, the two `TV`s inline | 1264 on all three |
//! | R2007, R2010, R2013 | 3 | 1284 on all three; name and an empty description in the string stream | 1284 |
//! | R2018 (`sample_AC1032.dwg`) | 11 | the three defaults 516 / 516 / 1028 with 7 strings each; eight library materials 554 to 1130 with 2 to 4 strings (name, description, texture-map paths) | unchanged |
//!
//! 65 records in all. Once their names are read, the three default
//! materials leave the same 1264 bits from R14 to R2004 and the same
//! 1284 from R2007 to R2013. Only the R2018 file holds a material that is
//! not a default, so the per-map layout `src/objects/acad_material.rs`
//! describes cannot yet be separated from its values (#49).
//!
//! # How to verify
//!
//! ```sh
//! cargo run --release --example probe_material -- ../../samples/*.dwg
//! cargo run --release --example probe_material -- ../../samples/*.dwg 'TV,TV'
//! ```
//!
//! A spec is a comma-separated list of `B`, `BB`, `3B`, `BS`, `BSU`,
//! `BL`, `BLU`, `BLL`, `BD`, `RC`, `RS`, `RL`, `RD`, `CMC` and `TV`, with
//! `n*T` for a run. `MAT_VERBOSE=1` prints every field of the spec with
//! its offset, width and decoded value, `MAT_BITS=1` dumps the raw
//! data-field bits, and `MAT_BD=1` lists every bit offset where a
//! plausible `BD` starts.

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::string_stream::{self, StringReader};
use dwg::{DwgFile, ObjectType, Version};

fn expand(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in spec.split(',').filter(|t| !t.trim().is_empty()) {
        let token = token.trim();
        match token.split_once('*') {
            Some((n, t)) => {
                let n: usize = n.parse().expect("repeat count must be a number");
                for _ in 0..n {
                    out.push(t.to_ascii_uppercase());
                }
            }
            None => out.push(token.to_ascii_uppercase()),
        }
    }
    out
}

fn read_tv(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    _version: Version,
) -> Result<String> {
    if let Some(reader) = strings.as_mut() {
        return reader.read_tv();
    }
    let len = c.read_bs_u()? as usize;
    let mut bytes = Vec::with_capacity(len);
    for _ in 0..len {
        bytes.push(c.read_rc()?);
    }
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (paths, spec): (Vec<String>, String) = {
        let mut paths = Vec::new();
        let mut spec = String::new();
        for a in args {
            if a.ends_with(".dwg") {
                paths.push(a);
            } else {
                spec = a;
            }
        }
        (paths, spec)
    };
    let fields = expand(&spec);
    let verbose = std::env::var("MAT_VERBOSE").is_ok();

    let mut closed = 0usize;
    let mut total = 0usize;
    for path in &paths {
        let file = DwgFile::open(path)?;
        let version = file.version();
        let Some(objects) = file.all_objects() else {
            continue;
        };
        let objects = objects?;
        let class_map = file.class_map().and_then(std::result::Result::ok);
        let name = std::path::Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        for object in &objects {
            let label = match object.kind {
                ObjectType::Custom(code) => class_map
                    .as_ref()
                    .and_then(|m| m.by_type_code(code))
                    .map(|d| d.dxf_class_name.clone())
                    .unwrap_or_default(),
                other => format!("{other}"),
            };
            if !label.eq_ignore_ascii_case("MATERIAL") {
                continue;
            }
            total += 1;

            let mut data_end = dwg::object::data_end_bit(object, version);
            let mut strings = match string_stream::locate(&object.raw, version) {
                Some(stream) => Some(StringReader::new(&object.raw, stream)?),
                None if version.is_r2007_plus() => Some(StringReader::empty(&object.raw)),
                None => None,
            };
            let mut c = dwg::object::body_cursor(object, version)?;
            // Common object data (§20.1), as `objects::modern`
            // reads it: EED chain, R14 `RL` size, `BL` reactor count,
            // R2004+ xdictionary bit, R2013+ AcDs bit (no further bits).
            let mut guard = 0;
            loop {
                guard += 1;
                if guard > 256 {
                    break;
                }
                let size = c.read_bs_u()? as usize;
                if size == 0 {
                    break;
                }
                let _ = c.read_handle()?;
                for _ in 0..size {
                    let _ = c.read_rc()?;
                }
            }
            if version.is_r13_r14() {
                data_end = Some(c.read_rl()? as usize);
            }
            let num_reactors = c.read_bl()?;
            if version.is_r2004_plus() {
                let _ = c.read_b()?;
            }
            let mut acds = false;
            if matches!(version, Version::R2013 | Version::R2018) {
                acds = c.read_b()?;
            }
            let body_start = c.position_bits();
            let budget = data_end.map(|e| e as isize - body_start as isize);

            let mut trace = String::new();
            let mut failed = None;
            for (index, token) in fields.iter().enumerate() {
                let at = c.position_bits();
                let value: Result<String> = (|| {
                    Ok(match token.as_str() {
                        "B" => format!("{}", c.read_b()?),
                        "BB" => format!("{}", c.read_bb()?),
                        "3B" => format!("{}", c.read_3b()?),
                        "BS" => format!("{}", c.read_bs()?),
                        "BSU" => format!("{}", c.read_bs_u()?),
                        "BL" => format!("{}", c.read_bl()?),
                        "BLU" => format!("{}", c.read_bl_u()?),
                        "BLL" => format!("{}", c.read_bll()?),
                        "BD" => format!("{}", c.read_bd()?),
                        "RC" => format!("{}", c.read_rc()?),
                        "RS" => format!("{}", c.read_rs()?),
                        "RL" => format!("{}", c.read_rl()?),
                        "RD" => format!("{}", c.read_rd()?),
                        "CMC" => {
                            let idx = c.read_bs()?;
                            let mut extra = String::new();
                            if version.is_r2004_plus() {
                                let rgb = c.read_bl_u()?;
                                let flag = c.read_rc()?;
                                extra = format!(" rgb {rgb:#010X} flag {flag}");
                                if flag & 1 != 0 {
                                    let _ = read_tv(&mut c, &mut strings, version)?;
                                }
                                if flag & 2 != 0 {
                                    let _ = read_tv(&mut c, &mut strings, version)?;
                                }
                            }
                            format!("{idx}{extra}")
                        }
                        "TV" => format!("{:?}", read_tv(&mut c, &mut strings, version)?),
                        other => panic!("unknown field token {other}"),
                    })
                })();
                match value {
                    Ok(v) => {
                        if verbose {
                            trace.push_str(&format!(
                                "\n  [{index:>3}] {token:<4} @{at:<5} w{:<3} = {v}",
                                c.position_bits() - at
                            ));
                        }
                    }
                    Err(e) => {
                        failed = Some(format!("field {index} {token}: {e}"));
                        break;
                    }
                }
            }
            let at = c.position_bits();
            let delta = data_end.map(|e| at as isize - e as isize);
            if delta == Some(0) && failed.is_none() {
                closed += 1;
            }

            let mut rest = Vec::new();
            if let Some(reader) = strings.as_mut() {
                while !reader.is_exhausted() {
                    match reader.read_tv() {
                        Ok(s) => rest.push(s),
                        Err(_) => break,
                    }
                }
            }
            println!(
                "{name:<20} 0x{:<5X} react={num_reactors} acds={} budget={:<6} delta={:<7} rest={:?}{}{}",
                object.handle.value,
                u8::from(acds),
                budget.map(|b| b.to_string()).unwrap_or("-".into()),
                delta.map(|d| d.to_string()).unwrap_or("-".into()),
                rest.iter().take(10).collect::<Vec<_>>(),
                failed.map(|f| format!(" FAILED {f}")).unwrap_or_default(),
                trace,
            );

            if std::env::var("MAT_BITS").is_ok() {
                if let Some(end) = data_end {
                    let mut line = String::new();
                    for bit in body_start..end {
                        let byte = object.raw.get(bit / 8).copied().unwrap_or(0);
                        line.push(if (byte >> (7 - (bit % 8))) & 1 != 0 {
                            '1'
                        } else {
                            '0'
                        });
                        if line.len() == 64 {
                            println!("  @{:<6} {line}", bit + 1 - 64 - body_start);
                            line.clear();
                        }
                    }
                    if !line.is_empty() {
                        println!("  @{:<6} {line}", end - line.len() - body_start);
                    }
                }
            }
            if std::env::var("MAT_BD").is_ok() {
                if let Some(end) = data_end {
                    let mut hits = Vec::new();
                    for start in body_start..end.saturating_sub(2) {
                        let mut p = BitCursor::new(&object.raw);
                        for _ in 0..start {
                            if p.read_b().is_err() {
                                break;
                            }
                        }
                        if let Ok(v) = p.read_bd() {
                            if v != 0.0 && v != 1.0 && v.is_finite() && v.abs() > 1e-12 {
                                hits.push(format!("@{}={v}", start - body_start));
                            }
                        }
                    }
                    println!("  BD hits: {}", hits.join(" "));
                }
            }
        }
    }
    println!("{closed}/{total} MATERIAL records close at delta 0");
    Ok(())
}
