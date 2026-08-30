//! Trace common-entity/body boundary candidates for fixed geometry entities.
//!
//! Usage:
//!
//! ```text
//! cargo run --example trace_entity_boundary -- ../../samples/line_2013.dwg 0x13
//! cargo run --example trace_entity_boundary -- ../../samples/sample_AC1032.dwg 0x13 20
//! ```
//!
//! The tracer replays the dispatcher's current object-header + common-entity
//! positioning, then brute-force decodes candidate body starts between the
//! header end and data-stream end. It is diagnostic only: candidates are
//! scored by geometric plausibility, not treated as proof.

use dwg::bitcursor::BitCursor;
use dwg::common_entity::{self, CommonEntityData};
use dwg::entities::{arc, circle, line};
use dwg::{DwgFile, Error, Version};
use std::collections::BTreeMap;
use std::env;
use std::process::ExitCode;

const TYPE_ARC: u16 = 0x11;
const TYPE_CIRCLE: u16 = 0x12;
const TYPE_LINE: u16 = 0x13;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: trace_entity_boundary <file.dwg> [type-code=0x13] [detail-limit]");
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
        None => TYPE_LINE,
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

    println!("=== {path} ===");
    println!("version: {}", file.version());
    println!("filter: type_code=0x{type_code:04X}");

    let mut matching = 0usize;
    let mut current_ok = 0usize;
    let mut current_plausible = 0usize;
    let mut with_plausible_candidate = 0usize;
    let mut delta_hist: Vec<isize> = Vec::new();
    let mut start_delta_hist: BTreeMap<isize, usize> = BTreeMap::new();
    let mut end_delta_hist: BTreeMap<isize, usize> = BTreeMap::new();
    let mut current_common_bits_hist: BTreeMap<usize, usize> = BTreeMap::new();
    let mut candidate_common_bits_hist: BTreeMap<usize, usize> = BTreeMap::new();
    let mut candidate_start_hist: BTreeMap<usize, usize> = BTreeMap::new();
    let mut no_candidate_examples = Vec::new();
    let mut matching_handle_offsets: BTreeMap<u64, Vec<(usize, usize)>> = BTreeMap::new();
    let mut detail_count = 0usize;

    for (object_index, raw) in objects.iter().enumerate() {
        if raw.type_code != type_code {
            continue;
        }
        matching += 1;
        matching_handle_offsets
            .entry(raw.handle.value)
            .or_default()
            .push((object_index, raw.stream_offset));
        let trace = trace_raw_object(raw, file.version(), type_code);
        if trace.current_decode_ok {
            current_ok += 1;
        }
        if trace.current_plausible {
            current_plausible += 1;
        }
        if let Some(best) = trace.best_candidate.as_ref() {
            with_plausible_candidate += 1;
            if let Some(current) = trace.current_body_start {
                let delta = best.start_bit as isize - current as isize;
                delta_hist.push(delta);
                *start_delta_hist.entry(delta).or_insert(0) += 1;
            }
            if let Some(data_end) = trace.data_end {
                *end_delta_hist
                    .entry(best.end_bit as isize - data_end as isize)
                    .or_insert(0) += 1;
            }
            if let Some(header_end) = trace.header_end {
                *candidate_common_bits_hist
                    .entry(best.start_bit.saturating_sub(header_end))
                    .or_insert(0) += 1;
            }
            *candidate_start_hist.entry(best.start_bit).or_insert(0) += 1;
        } else if no_candidate_examples.len() < 5 {
            no_candidate_examples.push(format!(
                "#{} handle=0x{:X} current_common_end={:?}",
                object_index, raw.handle.value, trace.current_body_start
            ));
        }

        if let (Some(header_end), Some(current)) = (trace.header_end, trace.current_body_start) {
            *current_common_bits_hist
                .entry(current.saturating_sub(header_end))
                .or_insert(0) += 1;
        }

        if detail_count < detail_limit {
            detail_count += 1;
            print_trace(object_index, raw, &trace);
        }
    }

    println!();
    println!("=== summary ===");
    println!("matching objects: {matching}");
    println!("current decoder ok: {current_ok}");
    println!("current decoder plausible: {current_plausible}");
    println!("objects with plausible scanned candidate: {with_plausible_candidate}");
    if !delta_hist.is_empty() {
        let min = delta_hist.iter().min().unwrap();
        let max = delta_hist.iter().max().unwrap();
        let sum: isize = delta_hist.iter().sum();
        let avg = sum as f64 / delta_hist.len() as f64;
        println!(
            "best_candidate_start - current_common_end: min={min} max={max} avg={avg:.1} bits"
        );
    }
    print_hist("current common bits", &current_common_bits_hist);
    print_hist("candidate common bits", &candidate_common_bits_hist);
    print_hist("candidate start bit", &candidate_start_hist);
    print_hist(
        "best_candidate_start - current_common_end",
        &start_delta_hist,
    );
    print_hist("best_candidate_end - data_end", &end_delta_hist);
    if !no_candidate_examples.is_empty() {
        println!(
            "objects without plausible scanned candidate: {}",
            no_candidate_examples.join(", ")
        );
    }
    let duplicate_handles = matching_handle_offsets
        .iter()
        .filter(|(_, offsets)| offsets.len() > 1)
        .map(|(handle, offsets)| {
            let rendered_offsets = offsets
                .iter()
                .map(|(index, offset)| format!("#{index}@{offset}"))
                .collect::<Vec<_>>()
                .join("/");
            format!("0x{handle:X}={rendered_offsets}")
        })
        .collect::<Vec<_>>();
    if !duplicate_handles.is_empty() {
        println!(
            "duplicate matching handles: {}",
            duplicate_handles.join(", ")
        );
    }

    ExitCode::SUCCESS
}

struct BoundaryTrace {
    header_end: Option<usize>,
    handle_stream_bits: Option<u64>,
    data_end: Option<usize>,
    current_body_start: Option<usize>,
    common: Option<CommonEntityData>,
    common_error: Option<String>,
    current_decode: String,
    current_decode_ok: bool,
    current_plausible: bool,
    best_candidate: Option<Candidate>,
    candidates: Vec<Candidate>,
}

struct Candidate {
    start_bit: usize,
    end_bit: usize,
    over_data_end: isize,
    plausible: bool,
    summary: String,
    score: i32,
}

fn print_hist<K>(label: &str, hist: &BTreeMap<K, usize>)
where
    K: std::fmt::Display + Ord,
{
    if hist.is_empty() {
        return;
    }
    let rendered = hist
        .iter()
        .map(|(value, count)| format!("{value}:{count}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!("{label}: {rendered}");
}

fn trace_raw_object(
    raw: &dwg::object::RawObject,
    version: Version,
    type_code: u16,
) -> BoundaryTrace {
    let header = replay_header(&raw.raw, version);
    let (header_end, handle_stream_bits, data_end) = match header {
        Ok(header) => (
            Some(header.header_end),
            header.handle_stream_bits,
            header.data_end,
        ),
        Err(e) => {
            return BoundaryTrace {
                header_end: None,
                handle_stream_bits: None,
                data_end: None,
                current_body_start: None,
                common: None,
                common_error: Some(format!("header replay: {e}")),
                current_decode: "header replay failed".to_string(),
                current_decode_ok: false,
                current_plausible: false,
                best_candidate: None,
                candidates: Vec::new(),
            };
        }
    };

    let mut cursor = BitCursor::new(&raw.raw);
    skip_to(&mut cursor, header_end.unwrap()).ok();
    let (current_body_start, common, common_error) =
        match common_entity::read_common_entity_data(&mut cursor, version) {
            Ok(common) => (Some(cursor.position_bits()), Some(common), None),
            Err(e) => (Some(cursor.position_bits()), None, Some(e.to_string())),
        };

    let (current_decode, current_decode_ok, current_plausible) = if common_error.is_none() {
        decode_candidate(&raw.raw, type_code, cursor.position_bits(), data_end)
            .map(|hit| (hit.summary, true, hit.plausible))
            .unwrap_or_else(|e| (format!("ERR {e}"), false, false))
    } else {
        ("common preamble failed".to_string(), false, false)
    };

    let mut candidates = Vec::new();
    if let (Some(header_end), Some(data_end)) = (header_end, data_end) {
        for start_bit in header_end..=data_end.min(raw.raw.len() * 8) {
            if let Ok(mut hit) = decode_candidate(&raw.raw, type_code, start_bit, Some(data_end)) {
                if hit.plausible {
                    let distance_to_data_end = (hit.end_bit as isize - data_end as isize).abs();
                    hit.score += 1000 - distance_to_data_end.min(1000) as i32;
                    if Some(start_bit) == current_body_start {
                        hit.score += 5000;
                    }
                    candidates.push(hit);
                }
            }
        }
    }
    candidates.sort_by(|a, b| b.score.cmp(&a.score).then(a.start_bit.cmp(&b.start_bit)));
    candidates.truncate(5);
    let best_candidate = candidates.first().map(|c| Candidate {
        start_bit: c.start_bit,
        end_bit: c.end_bit,
        over_data_end: c.over_data_end,
        plausible: c.plausible,
        summary: c.summary.clone(),
        score: c.score,
    });

    BoundaryTrace {
        header_end,
        handle_stream_bits,
        data_end,
        current_body_start,
        common,
        common_error,
        current_decode,
        current_decode_ok,
        current_plausible,
        best_candidate,
        candidates,
    }
}

struct HeaderReplay {
    header_end: usize,
    handle_stream_bits: Option<u64>,
    data_end: Option<usize>,
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

fn decode_candidate(
    payload: &[u8],
    type_code: u16,
    start_bit: usize,
    data_end: Option<usize>,
) -> Result<Candidate, String> {
    let mut c = BitCursor::new(payload);
    skip_to(&mut c, start_bit).map_err(|e| e.to_string())?;
    let (summary, plausible) = match type_code {
        TYPE_LINE => {
            let l = line::decode(&mut c).map_err(|e| e.to_string())?;
            let dx = l.end.x - l.start.x;
            let dy = l.end.y - l.start.y;
            let dz = l.end.z - l.start.z;
            let len = (dx * dx + dy * dy + dz * dz).sqrt();
            let plausible = finite_abs(l.start.x)
                && finite_abs(l.start.y)
                && finite_abs(l.start.z)
                && finite_abs(l.end.x)
                && finite_abs(l.end.y)
                && finite_abs(l.end.z)
                && finite_abs(l.thickness)
                && len.is_finite()
                && len > 1e-9
                && len < 1e9;
            (
                format!(
                    "LINE 2d={} start=({:.6},{:.6},{:.6}) end=({:.6},{:.6},{:.6}) len={:.6} th={:.3e}",
                    l.is_2d,
                    l.start.x,
                    l.start.y,
                    l.start.z,
                    l.end.x,
                    l.end.y,
                    l.end.z,
                    len,
                    l.thickness
                ),
                plausible,
            )
        }
        TYPE_CIRCLE => {
            let circ = circle::decode(&mut c).map_err(|e| e.to_string())?;
            let plausible = finite_abs(circ.center.x)
                && finite_abs(circ.center.y)
                && finite_abs(circ.center.z)
                && circ.radius.is_finite()
                && circ.radius > 1e-9
                && circ.radius < 1e9
                && finite_abs(circ.thickness);
            (
                format!(
                    "CIRCLE center=({:.6},{:.6},{:.6}) radius={:.6} th={:.3e}",
                    circ.center.x, circ.center.y, circ.center.z, circ.radius, circ.thickness
                ),
                plausible,
            )
        }
        TYPE_ARC => {
            let a = arc::decode(&mut c).map_err(|e| e.to_string())?;
            let plausible = finite_abs(a.center.x)
                && finite_abs(a.center.y)
                && finite_abs(a.center.z)
                && a.radius.is_finite()
                && a.radius > 1e-9
                && a.radius < 1e9
                && finite_abs(a.thickness)
                && a.start_angle.is_finite()
                && a.end_angle.is_finite();
            (
                format!(
                    "ARC center=({:.6},{:.6},{:.6}) radius={:.6} start={:.6} end={:.6}",
                    a.center.x, a.center.y, a.center.z, a.radius, a.start_angle, a.end_angle
                ),
                plausible,
            )
        }
        _ => return Err(format!("unsupported scan type 0x{type_code:04X}")),
    };
    let end_bit = c.position_bits();
    let over_data_end = data_end
        .map(|end| end_bit as isize - end as isize)
        .unwrap_or(0);
    Ok(Candidate {
        start_bit,
        end_bit,
        over_data_end,
        plausible,
        summary,
        score: if plausible { 100 } else { 0 },
    })
}

fn finite_abs(v: f64) -> bool {
    v.is_finite() && v.abs() < 1e9
}

fn print_trace(object_index: usize, raw: &dwg::object::RawObject, trace: &BoundaryTrace) {
    println!();
    println!(
        "#{} handle=0x{:X} offset={} size={} type=0x{:04X} {:?}",
        object_index, raw.handle.value, raw.stream_offset, raw.size_bytes, raw.type_code, raw.kind
    );
    println!(
        "  header_end={:?} handle_stream_bits={:?} data_end={:?} current_common_end={:?}",
        trace.header_end, trace.handle_stream_bits, trace.data_end, trace.current_body_start
    );
    if let Some(common) = trace.common.as_ref() {
        println!(
            "  common mode={:?} reactors={} no_xdict={} has_ds_data={} legacy_layer={} legacy_non_fixed_ltype={} plot={} material={} shadow={} invis={} lineweight=0x{:02X}",
            common.mode,
            common.num_reactors,
            common.no_xdictionary,
            common.binary_chain,
            common.is_on_layer,
            common.non_fixed_ltype,
            common.plotstyle_flag,
            common.material_flag,
            common.shadow_flags,
            common.invisibility,
            common.lineweight,
        );
    }
    if let Some(e) = trace.common_error.as_ref() {
        println!("  common error: {e}");
    }
    println!(
        "  current decode: {}{}",
        if trace.current_plausible {
            "PLAUSIBLE "
        } else if trace.current_decode_ok {
            "decoded-but-implausible "
        } else {
            ""
        },
        trace.current_decode
    );
    if trace.candidates.is_empty() {
        println!("  plausible scan candidates: none");
    } else {
        println!("  plausible scan candidates:");
        for c in &trace.candidates {
            println!(
                "    bit {:>3} -> {:>3} ({:+} vs data_end): {}",
                c.start_bit, c.end_bit, c.over_data_end, c.summary
            );
        }
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
        let data = b & 0x7F;
        value |= data << shift;
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
