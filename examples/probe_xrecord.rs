//! Does the XRECORD field list close on every record's boundary, and
//! what does the group-code chunk stream inside the body look like?
//!
//! Two measurements per record:
//!
//! 1. **Boundary** — `BL num_databytes`, that many `RC`, then `BS
//!    cloning_flags` (R2000+). The delta printed is
//!    `cursor - data_end`; a correct field list gives 0 on every
//!    record of every release band that publishes a boundary.
//! 2. **Chunk stream** — the body is a sequence of DXF-style
//!    (group code, value) pairs. This walks it with a candidate
//!    code→width table and reports where it lands relative to the
//!    declared byte count.
//!
//! ```sh
//! cargo run --release --example probe_xrecord -- samples/*.dwg
//! ```


use dwg::{DwgFile, ObjectType, Version};

/// Width in bytes of the value that follows a DXF group code in an
/// XRECORD body, or `None` for the variable-length string forms.
fn fixed_width(code: u16) -> Option<usize> {
    match code {
        0..=9 => None,               // string
        10..=39 => Some(8 * 3),      // 3D point (x, y, z doubles)
        40..=59 => Some(8),          // double
        60..=79 => Some(2),          // 16-bit int
        90..=99 => Some(4),          // 32-bit int
        100..=109 => None,           // string
        110..=119 => Some(8 * 3),    // point
        120..=129 => Some(8 * 3),
        130..=139 => Some(8 * 3),
        140..=149 => Some(8),        // double
        160..=169 => Some(8),        // 64-bit int
        170..=179 => Some(2),
        210..=239 => Some(8 * 3),
        270..=279 => Some(2),
        280..=289 => Some(1),        // 8-bit int (measured)
        290..=299 => Some(1),        // bool
        300..=309 => None,           // string
        310..=319 => Some(usize::MAX - 1), // binary chunk: RC len + bytes
        320..=369 => Some(8),        // handle (measured)
        370..=389 => Some(2),
        390..=399 => Some(8),        // handle (measured)
        400..=409 => Some(2),
        410..=419 => None,           // string
        420..=429 => Some(4),
        430..=439 => None,           // string
        440..=449 => Some(4),
        450..=459 => Some(4),
        460..=469 => Some(8),
        470..=479 => None,           // string
        480..=481 => Some(8),        // handle (measured)
        999 => None,
        1000..=1009 => None,
        1010..=1059 => Some(8),
        1060..=1070 => Some(2),
        1071 => Some(4),
        _ => Some(usize::MAX),
    }
}

/// Walk the body as a chunk stream, returning a description.
fn walk_chunks(body: &[u8], version: Version, seen: &mut std::collections::BTreeMap<u16, usize>) -> String {
    let mut i = 0usize;
    let mut out = String::new();
    let mut n = 0usize;
    while i < body.len() {
        // group code: RS on R13+ bodies (the body is byte-aligned raw)
        if i + 2 > body.len() {
            out.push_str(&format!(" [trunc-code@{i}]"));
            return out;
        }
        let code = u16::from_le_bytes([body[i], body[i + 1]]);
        i += 2;
        let w = fixed_width(code);
        let (desc, adv) = match w {
            None => {
                // string: RS length then that many bytes (+ codepage on
                // pre-R2007; UTF-16 on R2007)
                if i + 2 > body.len() {
                    out.push_str(&format!(" [trunc-strlen@{i}]"));
                    return out;
                }
                let len = u16::from_le_bytes([body[i], body[i + 1]]) as usize;
                i += 2;
                // pre-R2007: one RC codepage byte, then `len` bytes.
                // R2007+: `len` UTF-16 code units, no codepage byte.
                let cp = usize::from(!version.is_r2007_plus());
                let nbytes = if version.is_r2007_plus() { len * 2 } else { len };
                if i + nbytes + cp > body.len() {
                    out.push_str(&format!(" [trunc-str len={len}@{i}]"));
                    return out;
                }
                i += cp;
                let s: String = if version.is_r2007_plus() {
                    String::from_utf16_lossy(
                        &body[i..i + nbytes]
                            .chunks(2)
                            .map(|c| u16::from_le_bytes([c[0], c[1]]))
                            .collect::<Vec<_>>(),
                    )
                } else {
                    String::from_utf8_lossy(&body[i..i + nbytes]).into_owned()
                };
                (format!("{code}=\"{s}\""), nbytes)
            }
            Some(w) if w == usize::MAX - 1 => {
                if i >= body.len() {
                    out.push_str(&format!(" [trunc-binlen@{i}]"));
                    return out;
                }
                let len = body[i] as usize;
                i += 1;
                if i + len > body.len() {
                    out.push_str(&format!(" [trunc-bin len={len}@{i}]"));
                    return out;
                }
                (format!("{code}#{len}"), len)
            }
            Some(usize::MAX) => {
                out.push_str(&format!(" [unknown-code {code}@{}]", i - 2));
                return out;
            }
            Some(w) => {
                if i + w > body.len() {
                    out.push_str(&format!(" [trunc-val code={code} w={w}@{i}]"));
                    return out;
                }
                (format!("{code}:{w}"), w)
            }
        };
        i += adv;
        *seen.entry(code).or_default() += 1;
        if n < 12 {
            out.push(' ');
            out.push_str(&desc);
        }
        n += 1;
    }
    format!("OK({n}){out}")
}

fn main() -> dwg::error::Result<()> {
    for path in std::env::args().skip(1) {
        let file = DwgFile::open(&path)?;
        let version = file.version();
        let Some(objects) = file.all_objects() else {
            println!("{path}: no object walk");
            continue;
        };
        let objects = objects?;
        let class_map = file.class_map().and_then(std::result::Result::ok);
        let name = std::path::Path::new(&path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        let mut total = 0usize;
        let mut closed = 0usize;
        let mut chunk_ok = 0usize;
        let mut custom = 0usize;
        let mut seen: std::collections::BTreeMap<u16, usize> = std::collections::BTreeMap::new();
        for object in &objects {
            let label = match object.kind {
                ObjectType::Custom(code) => class_map
                    .as_ref()
                    .and_then(|m| m.by_type_code(code))
                    .map(|d| d.dxf_class_name.clone())
                    .unwrap_or_default(),
                other => format!("{other}"),
            };
            if !label.eq_ignore_ascii_case("XRECORD") && object.kind != ObjectType::XRecord {
                continue;
            }
            total += 1;
            if matches!(object.kind, ObjectType::Custom(_)) {
                custom += 1;
            }
            let mut data_end = dwg::object::data_end_bit(object, version);
            // Re-run the decoder's field list by hand so the intermediate
            // values are visible even when the boundary check fails.
            let mut c = dwg::object::body_cursor(object, version)?;
            // common object prefix
            let mut ok = true;
            for _ in 0..256 {
                match c.read_bs_u() {
                    Ok(0) => break,
                    Ok(sz) => {
                        if c.read_handle().is_err() {
                            ok = false;
                            break;
                        }
                        for _ in 0..sz {
                            if c.read_rc().is_err() {
                                ok = false;
                                break;
                            }
                        }
                    }
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                println!("{name} 0x{:X}: EED walk failed", object.handle.value);
                continue;
            }
            if matches!(version, Version::R14) {
                match c.read_rl() {
                    Ok(v) => data_end = Some(v as usize),
                    Err(_) => continue,
                }
            }
            if c.read_bl().is_err() {
                continue;
            }
            if version.is_r2004_plus() && c.read_b().is_err() {
                continue;
            }
            if matches!(version, Version::R2013 | Version::R2018) && c.read_b().is_err() {
                continue;
            }
            let nbytes = match c.read_bl() {
                Ok(v) => v as usize,
                Err(e) => {
                    println!("{name} 0x{:X}: BL: {e}", object.handle.value);
                    continue;
                }
            };
            let mut body = Vec::with_capacity(nbytes.min(1 << 20));
            let mut bad = false;
            for _ in 0..nbytes {
                match c.read_rc() {
                    Ok(b) => body.push(b),
                    Err(_) => {
                        bad = true;
                        break;
                    }
                }
            }
            if bad {
                println!(
                    "{name} 0x{:X}: body {nbytes} bytes overruns payload",
                    object.handle.value
                );
                continue;
            }
            let cloning = if matches!(version, Version::R14) {
                0
            } else {
                match c.read_bs() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("{name} 0x{:X}: BS cloning failed", object.handle.value);
                        continue;
                    }
                }
            };
            let delta = data_end.map(|e| c.position_bits() as isize - e as isize);
            if delta == Some(0) {
                closed += 1;
            }
            if let Ok(want) = std::env::var("XR_HEX") {
                let want = u64::from_str_radix(want.trim_start_matches("0x"), 16).unwrap_or(0);
                if object.handle.value == want {
                    println!("{name} 0x{:X} nbytes={nbytes}", object.handle.value);
                    for (i, chunk) in body.chunks(16).enumerate() {
                        let hex: Vec<String> =
                            chunk.iter().map(|b| format!("{b:02x}")).collect();
                        let asc: String = chunk
                            .iter()
                            .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                            .collect();
                        println!("  {:04}  {:<48}  {asc}", i * 16, hex.join(" "));
                    }
                }
            }
            let chunks = walk_chunks(&body, version, &mut seen);
            if chunks.starts_with("OK") {
                chunk_ok += 1;
            }

            if std::env::var("XR_VERBOSE").is_ok() || delta.unwrap_or(0) != 0 || !chunks.starts_with("OK") {
                println!(
                    "{name} 0x{:<5X} nbytes={nbytes:<5} clone={cloning:<3} delta={:<6} {chunks}",
                    object.handle.value,
                    delta.map(|d| d.to_string()).unwrap_or("-".into()),
                );
            }
        }
        if std::env::var("XR_CODES").is_ok() {
            let list: Vec<String> = seen.iter().map(|(k, v)| format!("{k}x{v}")).collect();
            println!("{name} codes: {}", list.join(" "));
        }
        println!("{name} ({version}): {closed}/{total} close at delta 0, {chunk_ok}/{total} chunk streams walk clean, {custom} custom-class");
    }
    Ok(())
}
