//! Trace table-entry decoder boundary/order candidates.
//!
//! Usage:
//!
//! ```text
//! cargo run --example trace_table_entry -- ../../samples/sample_AC1032.dwg 0x31 8
//! ```

use dwg::bitcursor::BitCursor;
use dwg::{DwgFile, Error, Version};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: trace_table_entry <file.dwg> [type-code=0x31] [detail-limit]");
        return ExitCode::FAILURE;
    };
    let type_code = match args.next() {
        Some(s) => match parse_type_code(&s) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        },
        None => 0x31,
    };
    let detail_limit = args
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(usize::MAX);

    let file = match DwgFile::open(&path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("open failed ({path}): {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(objects_result) = file.all_objects() else {
        eprintln!("{} has no handle-driven object walk", file.version());
        return ExitCode::FAILURE;
    };
    let objects = match objects_result {
        Ok(objects) => objects,
        Err(e) => {
            eprintln!("object walk failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut matching = 0usize;
    let mut current_ok = 0usize;
    let mut current_with_preamble_ok = 0usize;
    let mut spec_block_ok = 0usize;
    let mut spec_block_no_preamble_ok = 0usize;
    let mut detail_count = 0usize;

    println!("=== {path} ===");
    println!("version: {}", file.version());
    println!("filter: type_code=0x{type_code:04X}");

    for (object_index, raw) in objects.iter().enumerate() {
        if raw.type_code != type_code {
            continue;
        }
        matching += 1;
        let header = match replay_header(&raw.raw, file.version()) {
            Ok(header) => header,
            Err(e) => {
                if detail_count < detail_limit {
                    detail_count += 1;
                    println!();
                    println!(
                        "#{object_index} handle=0x{:X} offset={} size={} header replay failed: {e}",
                        raw.handle.value, raw.stream_offset, raw.size_bytes
                    );
                }
                continue;
            }
        };

        let current = decode_current(&raw.raw, file.version(), header.header_end);
        let current_with_preamble =
            decode_current_after_preamble(&raw.raw, file.version(), header.header_end);
        let spec_block = decode_spec_block(&raw.raw, file.version(), header.header_end, true);
        let spec_block_no_preamble =
            decode_spec_block(&raw.raw, file.version(), header.header_end, false);

        if current.ok {
            current_ok += 1;
        }
        if current_with_preamble.ok {
            current_with_preamble_ok += 1;
        }
        if spec_block.ok {
            spec_block_ok += 1;
        }
        if spec_block_no_preamble.ok {
            spec_block_no_preamble_ok += 1;
        }

        if detail_count < detail_limit {
            detail_count += 1;
            println!();
            println!(
                "#{object_index} handle=0x{:X} offset={} size={} raw_bits={} kind={:?}",
                raw.handle.value,
                raw.stream_offset,
                raw.size_bytes,
                raw.raw.len() * 8,
                raw.kind
            );
            println!(
                "  header_end={} handle_stream_bits={:?} data_end={:?}",
                header.header_end, header.handle_stream_bits, header.data_end
            );
            if let Some(data_end) = header.data_end {
                println!(
                    "  data window: {}",
                    hex_window(&raw.raw, header.header_end, data_end)
                );
                println!(
                    "  first data bits: {}",
                    bits_window(&raw.raw, header.header_end, header.header_end + 48)
                );
                let hits = scan_printable_tu(&raw.raw, header.header_end, data_end);
                if !hits.is_empty() {
                    println!("  printable TU scan: {}", hits.join("; "));
                }
                println!(
                    "  tail size probes: {}",
                    tail_size_probes(&raw.raw, data_end)
                );
            }
            if let Some(split) = trace_string_split(&raw.raw, header.data_end) {
                println!(
                    "  string split guess: present={} size={} start={} end={} bytes={}",
                    split.present,
                    split.size_bits,
                    split.start_bit,
                    split.end_bit,
                    hex_window(&raw.raw, split.start_bit, split.end_bit)
                );
            }
            print_attempt("current", &current, header.data_end);
            print_attempt(
                "current+object-preamble",
                &current_with_preamble,
                header.data_end,
            );
            print_attempt("spec-block", &spec_block, header.data_end);
            print_attempt(
                "spec-block-no-preamble",
                &spec_block_no_preamble,
                header.data_end,
            );
            if type_code == 0x39 {
                let ltype_modern =
                    decode_modern_ltype_probe(&raw.raw, file.version(), header.header_end);
                print_attempt("ltype-modern-probe", &ltype_modern, header.data_end);
            }
        }
    }

    println!();
    println!("=== summary ===");
    println!("matching objects: {matching}");
    println!("current decoder ok: {current_ok}");
    println!("current+object-preamble ok: {current_with_preamble_ok}");
    println!("spec-block ok: {spec_block_ok}");
    println!("spec-block-no-preamble ok: {spec_block_no_preamble_ok}");

    ExitCode::SUCCESS
}

#[derive(Debug)]
struct HeaderReplay {
    header_end: usize,
    handle_stream_bits: Option<u64>,
    data_end: Option<usize>,
}

#[derive(Debug)]
struct Attempt {
    ok: bool,
    end_bit: Option<usize>,
    summary: String,
}

struct StringSplitTrace {
    present: bool,
    size_bits: usize,
    start_bit: usize,
    end_bit: usize,
}

fn replay_header(payload: &[u8], version: Version) -> Result<HeaderReplay, Error> {
    let mut c = BitCursor::new(payload);
    let handle_stream_bits = if version.is_r2010_plus() {
        Some(read_mc_unsigned(&mut c)?)
    } else {
        None
    };
    let _type_code = read_object_type(&mut c, version)?;
    if matches!(version, Version::R2000) {
        let _object_size_bits = c.read_rl()?;
    }
    let _handle = c.read_handle()?;
    let data_end =
        handle_stream_bits.and_then(|bits| (payload.len() * 8).checked_sub(bits as usize));
    Ok(HeaderReplay {
        header_end: c.position_bits(),
        handle_stream_bits,
        data_end,
    })
}

fn decode_current(payload: &[u8], version: Version, start_bit: usize) -> Attempt {
    let mut c = BitCursor::new(payload);
    if let Err(e) = skip_to(&mut c, start_bit) {
        return fail(e.to_string(), c.position_bits());
    }
    match dwg::tables::block_record::decode(&mut c, version) {
        Ok(block) => Attempt {
            ok: true,
            end_bit: Some(c.position_bits()),
            summary: format!(
                "name={:?} owned={:?} base=({:.3},{:.3},{:.3}) xref={:?}",
                block.header.name,
                block.num_owned_objects,
                block.base_point.x,
                block.base_point.y,
                block.base_point.z,
                block.xref_path
            ),
        },
        Err(e) => fail(e.to_string(), c.position_bits()),
    }
}

fn decode_current_after_preamble(payload: &[u8], version: Version, start_bit: usize) -> Attempt {
    let mut c = BitCursor::new(payload);
    if let Err(e) = skip_to(&mut c, start_bit).and_then(|_| read_object_preamble(&mut c, version)) {
        return fail(e.to_string(), c.position_bits());
    }
    match dwg::tables::block_record::decode(&mut c, version) {
        Ok(block) => Attempt {
            ok: true,
            end_bit: Some(c.position_bits()),
            summary: format!(
                "name={:?} owned={:?} base=({:.3},{:.3},{:.3}) xref={:?}",
                block.header.name,
                block.num_owned_objects,
                block.base_point.x,
                block.base_point.y,
                block.base_point.z,
                block.xref_path
            ),
        },
        Err(e) => fail(e.to_string(), c.position_bits()),
    }
}

fn decode_spec_block(
    payload: &[u8],
    version: Version,
    start_bit: usize,
    with_preamble: bool,
) -> Attempt {
    let mut c = BitCursor::new(payload);
    let setup = skip_to(&mut c, start_bit).and_then(|_| {
        if with_preamble {
            read_object_preamble(&mut c, version)
        } else {
            Ok(())
        }
    });
    if let Err(e) = setup {
        return fail(e.to_string(), c.position_bits());
    }

    let result = (|| -> Result<String, Error> {
        let name = read_tv(&mut c, version)?;
        let flag64 = c.read_b()?;
        let xref_index_plus_1 = c.read_bs()?;
        let xdep = c.read_b()?;
        let anonymous = c.read_b()?;
        let has_atts = c.read_b()?;
        let is_xref = c.read_b()?;
        let xref_overlay = c.read_b()?;
        let loaded = if is_r2000_plus(version) {
            Some(c.read_b()?)
        } else {
            None
        };
        let owned = if version.is_r2004_plus() {
            Some(c.read_bl()?)
        } else {
            None
        };
        let base_x = c.read_bd()?;
        let base_y = c.read_bd()?;
        let base_z = c.read_bd()?;
        let xref_path = read_tv(&mut c, version)?;
        let mut insert_count_bytes = 0usize;
        if is_r2000_plus(version) {
            loop {
                let b = c.read_rc()?;
                if b == 0 {
                    break;
                }
                insert_count_bytes += 1;
                if insert_count_bytes > 100_000 {
                    return Err(Error::SectionMap(
                        "BLOCK_HEADER insert count exceeds cap".into(),
                    ));
                }
            }
            let _description = read_tv(&mut c, version)?;
            let preview_bytes = c.read_bl_u()? as usize;
            for _ in 0..preview_bytes {
                let _ = c.read_rc()?;
            }
        }
        let insert_units = if version.is_r2007_plus() {
            Some(c.read_bs()?)
        } else {
            None
        };
        let explodable = if version.is_r2007_plus() {
            Some(c.read_b()?)
        } else {
            None
        };
        let scaling = if version.is_r2007_plus() {
            Some(c.read_rc()?)
        } else {
            None
        };
        Ok(format!(
            "name={name:?} flag64={flag64} xref_index_plus_1={xref_index_plus_1} xdep={xdep} anon={anonymous} has_atts={has_atts} is_xref={is_xref} overlay={xref_overlay} loaded={loaded:?} owned={owned:?} base=({base_x:.3},{base_y:.3},{base_z:.3}) xref={xref_path:?} insert_refs={insert_count_bytes} units={insert_units:?} explodable={explodable:?} scaling={scaling:?}"
        ))
    })();

    match result {
        Ok(summary) => Attempt {
            ok: true,
            end_bit: Some(c.position_bits()),
            summary,
        },
        Err(e) => fail(e.to_string(), c.position_bits()),
    }
}

fn read_object_preamble(c: &mut BitCursor<'_>, version: Version) -> Result<(), Error> {
    loop {
        let size = c.read_bs_u()?;
        if size == 0 {
            break;
        }
        let _appid = c.read_handle()?;
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
    }
    let _num_reactors = c.read_bl()?;
    if version.is_r2004_plus() {
        let _no_xdictionary = c.read_b()?;
    }
    Ok(())
}

fn decode_modern_ltype_probe(payload: &[u8], version: Version, start_bit: usize) -> Attempt {
    let mut c = BitCursor::new(payload);
    if let Err(e) =
        skip_to(&mut c, start_bit).and_then(|_| read_table_object_prefix_probe(&mut c, version))
    {
        return fail(e.to_string(), c.position_bits());
    }
    let prefix_end = c.position_bits();
    let result = (|| -> Result<String, Error> {
        let flag64 = c.read_b()?;
        let xref_index_plus_1 = c.read_bs()?;
        let xdep = c.read_b()?;
        let pattern_len = c.read_bd()?;
        let alignment = c.read_rc()?;
        let num_dashes = c.read_rc()? as usize;
        for _ in 0..num_dashes.min(256) {
            let _length = c.read_bd()?;
            let _shape_number = c.read_bs()?;
            let _x_offset = c.read_rd()?;
            let _y_offset = c.read_rd()?;
            let _scale = c.read_bd()?;
            let _rotation = c.read_bd()?;
            let _shape_flag = c.read_bs()?;
        }
        let string_start = c.position_bits();
        let name = read_tv(&mut c, version)?;
        let desc = read_tv(&mut c, version).unwrap_or_default();
        Ok(format!(
            "prefix_end={prefix_end} flag64={flag64} xref_index_plus_1={xref_index_plus_1} xdep={xdep} pattern_len={pattern_len:.3} alignment=0x{alignment:02X} num_dashes={num_dashes} string_start={string_start} name={name:?} desc={desc:?}"
        ))
    })();
    match result {
        Ok(summary) => Attempt {
            ok: true,
            end_bit: Some(c.position_bits()),
            summary,
        },
        Err(e) => fail(e.to_string(), c.position_bits()),
    }
}

fn read_table_object_prefix_probe(c: &mut BitCursor<'_>, version: Version) -> Result<(), Error> {
    for _ in 0..256 {
        let size = c.read_bs_u()? as usize;
        if size == 0 {
            break;
        }
        let _appid = c.read_handle()?;
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
    }
    if version.is_r2004_plus() {
        let _no_xdictionary = c.read_b()?;
    }
    if matches!(version, Version::R2013 | Version::R2018) {
        let _has_binary_data = c.read_b()?;
    }
    Ok(())
}

fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String, Error> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if version.is_r2007_plus() {
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            let lo = c.read_rc()? as u16;
            let hi = c.read_rc()? as u16;
            units.push((hi << 8) | lo);
        }
        if units.last() == Some(&0) {
            units.pop();
        }
        String::from_utf16(&units)
            .map_err(|_| Error::SectionMap("table entry name is not valid UTF-16".into()))
    } else {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(c.read_rc()?);
        }
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn print_attempt(label: &str, attempt: &Attempt, data_end: Option<usize>) {
    let delta = match (attempt.end_bit, data_end) {
        (Some(end), Some(data_end)) => {
            format!(" ({:+} vs data_end)", end as isize - data_end as isize)
        }
        _ => String::new(),
    };
    println!(
        "  {label}: {} end={:?}{delta} {}",
        if attempt.ok { "ok" } else { "ERR" },
        attempt.end_bit,
        attempt.summary
    );
}

fn trace_string_split(payload: &[u8], data_end: Option<usize>) -> Option<StringSplitTrace> {
    let data_end = data_end?;
    if data_end < 17 {
        return None;
    }
    let present = read_bit_at(payload, data_end - 1)?;
    if !present {
        return Some(StringSplitTrace {
            present,
            size_bits: 0,
            start_bit: data_end,
            end_bit: data_end,
        });
    }
    let size_start = data_end.checked_sub(17)?;
    let size_bits = read_raw_u16_at(payload, size_start)? as usize;
    let end_bit = size_start;
    let start_bit = end_bit.checked_sub(size_bits)?;
    Some(StringSplitTrace {
        present,
        size_bits,
        start_bit,
        end_bit,
    })
}

fn read_bit_at(bytes: &[u8], bit: usize) -> Option<bool> {
    let byte = *bytes.get(bit / 8)?;
    let shift = 7 - (bit % 8);
    Some(((byte >> shift) & 1) != 0)
}

fn read_raw_u16_at(bytes: &[u8], bit: usize) -> Option<u16> {
    let mut c = BitCursor::new(bytes);
    skip_to(&mut c, bit).ok()?;
    c.read_rs().ok().map(|value| value as u16)
}

fn hex_window(bytes: &[u8], start_bit: usize, end_bit: usize) -> String {
    let start = start_bit / 8;
    let end = end_bit.div_ceil(8).min(bytes.len());
    bytes
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn bits_window(bytes: &[u8], start_bit: usize, end_bit: usize) -> String {
    (start_bit..end_bit)
        .filter_map(|bit| read_bit_at(bytes, bit).map(|value| if value { '1' } else { '0' }))
        .collect()
}

fn scan_printable_tu(bytes: &[u8], start_bit: usize, end_bit: usize) -> Vec<String> {
    let mut hits = Vec::new();
    for bit in start_bit..end_bit {
        let mut c = BitCursor::new(bytes);
        if skip_to(&mut c, bit).is_err() {
            continue;
        }
        let Ok(s) = read_tv(&mut c, Version::R2018) else {
            continue;
        };
        let end = c.position_bits();
        if s.len() >= 2
            && s.len() <= 80
            && s.chars()
                .all(|ch| ch == '\0' || ch == '\n' || ch == '\r' || !ch.is_control())
        {
            hits.push(format!("bit {bit}->{end}: {s:?}"));
            if hits.len() >= 8 {
                break;
            }
        }
    }
    hits
}

fn tail_size_probes(bytes: &[u8], data_end: usize) -> String {
    let start = data_end.saturating_sub(32);
    (start..data_end)
        .filter_map(|bit| {
            let raw = read_raw_u16_at(bytes, bit)?;
            let bs = read_bs_u_at(bytes, bit)?;
            if raw <= 512 || bs <= 512 {
                Some(format!("{bit}:raw={raw},bs={bs}"))
            } else {
                None
            }
        })
        .take(12)
        .collect::<Vec<_>>()
        .join("; ")
}

fn read_bs_u_at(bytes: &[u8], bit: usize) -> Option<u16> {
    let mut c = BitCursor::new(bytes);
    skip_to(&mut c, bit).ok()?;
    c.read_bs_u().ok()
}

fn fail(message: String, bit: usize) -> Attempt {
    Attempt {
        ok: false,
        end_bit: Some(bit),
        summary: message,
    }
}

fn skip_to(c: &mut BitCursor<'_>, bit: usize) -> Result<(), Error> {
    while c.position_bits() < bit {
        let _ = c.read_b()?;
    }
    Ok(())
}

fn read_mc_unsigned(c: &mut BitCursor<'_>) -> Result<u64, Error> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let b = c.read_rc()? as u64;
        let cont = (b & 0x80) != 0;
        value |= (b & 0x7F) << shift;
        shift += 7;
        if !cont || shift >= 64 {
            return Ok(value);
        }
    }
}

fn read_object_type(c: &mut BitCursor<'_>, version: Version) -> Result<u16, Error> {
    if version.is_r2010_plus() {
        let tag = c.read_bb()?;
        match tag {
            0 => Ok(c.read_rc()? as u16),
            1 => Ok((c.read_rc()? as u16) + 0x1F0),
            _ => {
                let lsb = c.read_rc()? as u16;
                let msb = c.read_rc()? as u16;
                Ok((msb << 8) | lsb)
            }
        }
    } else {
        Ok(c.read_bs_u()?)
    }
}

fn is_r2000_plus(version: Version) -> bool {
    !matches!(version, Version::R14)
}

fn parse_type_code(s: &str) -> Result<u16, String> {
    let trimmed = s.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u16::from_str_radix(hex, 16).map_err(|e| format!("invalid hex type code {s}: {e}"))
    } else {
        trimmed
            .parse::<u16>()
            .map_err(|e| format!("invalid type code {s}: {e}"))
    }
}
