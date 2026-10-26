//! Does the §20.4.48 MULTILEADER field list close on every release, and
//! do its values agree with AutoCAD's DXF export?
//!
//! # What this proves
//!
//! For every MULTILEADER record of a drawing this reports whether the
//! crate's decoder closed exactly on the record's data-stream boundary
//! (ARCHITECTURE.md §7i). With a second argument — AutoCAD's DXF export
//! of the same drawing — it compares the decoded values with the
//! `AcDbMLeader` group of the DXF entity whose handle (group 5) matches.
//!
//! The DXF writes the record in three parts: the embedded context
//! (`300 CONTEXT_DATA{` up to the first `302 LEADER{`), one `LEADER{`
//! block per leader root with a `LEADER_LINE{` block per line, and the
//! entity's own fields after `301 }`. Each decoded value is looked up,
//! in order, among the groups of its part: a value matches the next group
//! with its code after the previous match. The comparison covers the
//! context's scale, base point, text height, arrow and landing sizes,
//! attachments, the text label, location, direction, rotation, boundary,
//! line spacing, alignment, flow, background, column and word-break
//! fields, and the base point, direction and vertical; every leader
//! root's flags, connection point, direction, index and landing
//! distance; every leader line's points and index; and the entity's
//! override flags, leader type, line weight, landing and dog-leg flags,
//! landing distance, arrow size, content type, attachments, text frame,
//! block scale, rotation and attachment, annotative flag, and on R2007 and
//! earlier the four scalar fields of the `-R2007` tail. Colours are
//! compared from R2004 on, as the CMC `BL` word against the DXF's 32-bit
//! group; before R2004 a CMC is a bare index and is not compared.
//!
//! ```sh
//! cargo run --release --example probe_mleader -- sample_AC1018.dwg sample_AC1018_ascii.dxf
//! ```
//!
//! Exit status is non-zero when a record errors or a value differs.

use dwg::entities::DecodedEntity;
use dwg::entities::mleader::MLeader;
use dwg::{DwgFile, ObjectType, Version};
use std::collections::HashMap;
use std::process::ExitCode;

type Groups = Vec<(u16, String)>;

#[derive(Default)]
struct DxfMLeader {
    context: Groups,
    roots: Vec<(Groups, Vec<Groups>)>,
    tail: Groups,
}

fn dxf_mleaders(path: &str) -> std::io::Result<HashMap<u64, DxfMLeader>> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect());
    let lines: Vec<&str> = text.lines().collect();
    let mut out = HashMap::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if !(lines[i].trim() == "0" && lines[i + 1].trim() == "MULTILEADER") {
            i += 2;
            continue;
        }
        let mut handle = None;
        let mut m = DxfMLeader::default();
        let mut inside = false;
        // 0 context, 1 leader root, 2 leader line, 3 tail
        let mut part = 0;
        let mut j = i + 2;
        while j + 1 < lines.len() && lines[j].trim() != "0" {
            let code: u16 = lines[j].trim().parse().unwrap_or(u16::MAX);
            let value = lines[j + 1].trim().to_string();
            j += 2;
            match (code, value.as_str()) {
                (5, v) => handle = u64::from_str_radix(v, 16).ok(),
                (100, v) => inside = v == "AcDbMLeader",
                _ if !inside => {}
                (300, "CONTEXT_DATA{") => part = 0,
                (302, "LEADER{") => {
                    m.roots.push((Vec::new(), Vec::new()));
                    part = 1;
                }
                (304, "LEADER_LINE{") => {
                    if let Some(root) = m.roots.last_mut() {
                        root.1.push(Vec::new());
                    }
                    part = 2;
                }
                (305, "}") => part = 1,
                (303, "}") => part = 0,
                (301, "}") => part = 3,
                _ => {
                    let target = match part {
                        0 => &mut m.context,
                        1 => match m.roots.last_mut() {
                            Some(root) => &mut root.0,
                            None => &mut m.context,
                        },
                        2 => match m.roots.last_mut().and_then(|r| r.1.last_mut()) {
                            Some(line) => line,
                            None => &mut m.context,
                        },
                        _ => &mut m.tail,
                    };
                    target.push((code, value));
                }
            }
        }
        if let Some(h) = handle {
            out.insert(h, m);
        }
        i = j;
    }
    Ok(out)
}

fn n(v: f64) -> String {
    v.to_string()
}

fn b(v: bool) -> String {
    u8::from(v).to_string()
}

fn p3(out: &mut Vec<(u16, String)>, base: u16, p: dwg::entities::Point3D) {
    out.extend([(base, n(p.x)), (base + 10, n(p.y)), (base + 20, n(p.z))]);
}

fn context_expect(m: &MLeader, version: Version) -> Vec<(u16, String)> {
    let c = &m.context;
    let mut out = vec![(40, n(c.overall_scale))];
    p3(&mut out, 10, c.content_base_point);
    out.extend([
        (41, n(c.text_height)),
        (140, n(c.arrow_head_size)),
        (145, n(c.landing_gap)),
        (174, c.left_attachment.to_string()),
        (175, c.right_attachment.to_string()),
        (176, c.text_align_type.to_string()),
        (177, c.attachment_type.to_string()),
        (290, b(c.text.is_some())),
    ]);
    if let Some(t) = &c.text {
        out.push((304, t.label.clone()));
        p3(&mut out, 11, t.normal);
        p3(&mut out, 12, t.location);
        p3(&mut out, 13, t.direction);
        out.extend([
            (42, n(t.rotation)),
            (43, n(t.boundary_width)),
            (44, n(t.boundary_height)),
            (45, n(t.line_spacing_factor)),
            (170, t.line_spacing_style.to_string()),
        ]);
        if version.is_r2004_plus() {
            out.push((90, (t.color.rgb as i32).to_string()));
        }
        out.extend([
            (171, t.alignment.to_string()),
            (172, t.flow_direction.to_string()),
        ]);
        if version.is_r2004_plus() {
            out.push((91, (t.background_color.rgb as i32).to_string()));
        }
        out.extend([
            (141, n(t.background_scale)),
            (92, t.background_transparency.to_string()),
            (291, b(t.background_fill_enabled)),
            (292, b(t.background_mask_fill_on)),
            (173, t.column_type.to_string()),
            (293, b(t.text_height_automatic)),
            (142, n(t.column_width)),
            (143, n(t.column_gutter)),
            (294, b(t.column_flow_reversed)),
            (295, b(t.word_break)),
        ]);
    } else {
        out.push((296, b(c.block.is_some())));
    }
    p3(&mut out, 110, c.base_point);
    p3(&mut out, 111, c.base_direction);
    p3(&mut out, 112, c.base_vertical);
    out.push((297, b(c.normal_reversed)));
    out
}

fn tail_expect(m: &MLeader, version: Version) -> Vec<(u16, String)> {
    let mut out = vec![
        (90, (m.override_flags as i32).to_string()),
        (170, m.leader_type.to_string()),
    ];
    if version.is_r2004_plus() {
        out.push((91, (m.leader_color.rgb as i32).to_string()));
    }
    out.extend([
        (171, m.line_weight.to_string()),
        (290, b(m.landing_enabled)),
        (291, b(m.dogleg_enabled)),
        (41, n(m.landing_distance)),
        (42, n(m.arrow_head_size)),
        (172, m.content_type.to_string()),
        (173, m.left_attachment.to_string()),
        (95, m.right_attachment.to_string()),
        (174, m.text_angle_type.to_string()),
        (175, m.unknown_175.to_string()),
    ]);
    if version.is_r2004_plus() {
        out.push((92, (m.text_color.rgb as i32).to_string()));
    }
    out.push((292, b(m.text_frame_enabled)));
    if version.is_r2004_plus() {
        out.push((93, (m.block_color.rgb as i32).to_string()));
    }
    p3(&mut out, 10, m.block_scale);
    out.extend([
        (43, n(m.block_rotation)),
        (176, m.block_attachment_type.to_string()),
        (293, b(m.is_annotative)),
    ]);
    if let Some(l) = &m.legacy {
        out.extend([
            (294, b(l.text_direction_negative)),
            (178, l.ipe_align.to_string()),
            (179, l.justification.to_string()),
            (45, n(l.scale_factor)),
        ]);
    }
    out
}

fn same(ours: &str, theirs: &str) -> bool {
    match (ours.parse::<f64>(), theirs.parse::<f64>()) {
        (Ok(a), Ok(b)) => (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0),
        _ => ours == theirs,
    }
}

/// Match each expected group in order against `theirs`; returns
/// (compared, mismatches).
fn walk(expect: &[(u16, String)], theirs: &Groups, what: &str) -> (usize, Vec<String>) {
    let mut at = 0;
    let mut compared = 0;
    let mut bad = Vec::new();
    for (code, ours) in expect {
        match theirs[at..].iter().position(|(c, _)| c == code) {
            Some(k) => {
                compared += 1;
                let v = &theirs[at + k].1;
                if !same(ours, v) {
                    bad.push(format!("{what} {code}: dwg {ours:?} dxf {v:?}"));
                }
                at += k + 1;
            }
            None => bad.push(format!("{what} {code}: dwg {ours:?}, not in the DXF")),
        }
    }
    (compared, bad)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe_mleader <file.dwg> [twin.dxf]");
        return ExitCode::FAILURE;
    };
    let dxf = args.next();
    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("open failed ({path}): {e}");
            return ExitCode::FAILURE;
        }
    };
    let version = file.version();
    let (Some(Ok(objects)), Some(Ok(class_map))) = (file.all_objects(), file.class_map()) else {
        eprintln!("{path}: no object walk or class map");
        return ExitCode::FAILURE;
    };
    let twin = match dxf.as_deref().map(dxf_mleaders).transpose() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("read failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(path.clone());

    let (mut records, mut closed, mut compared, mut differ) = (0usize, 0usize, 0usize, 0usize);
    for object in &objects {
        let ObjectType::Custom(code) = object.kind else {
            continue;
        };
        if !class_map
            .by_type_code(code)
            .is_some_and(|d| d.dxf_class_name == "MULTILEADER")
        {
            continue;
        }
        records += 1;
        let handle = object.handle.value;
        let m = match dwg::entities::decode_from_raw_with_class_map(
            object, version, &class_map, code,
        ) {
            DecodedEntity::MLeader(m) => m,
            DecodedEntity::Error { message, .. } => {
                println!("{name} ({version}) MULTILEADER 0x{handle:X} ERROR {message}");
                continue;
            }
            other => {
                println!(
                    "{name} ({version}) MULTILEADER 0x{handle:X} decoded as {:?}",
                    other.type_code()
                );
                continue;
            }
        };
        closed += 1;
        let Some(twin) = &twin else { continue };
        let Some(theirs) = twin.get(&handle) else {
            println!("{name} ({version}) MULTILEADER 0x{handle:X}: not in the DXF");
            differ += 1;
            continue;
        };
        let mut bad = Vec::new();
        let (c1, b1) = walk(&context_expect(&m, version), &theirs.context, "context");
        compared += c1;
        bad.extend(b1);
        if m.context.leader_roots.len() != theirs.roots.len() {
            bad.push(format!(
                "leader roots: dwg {} dxf {}",
                m.context.leader_roots.len(),
                theirs.roots.len()
            ));
        }
        for (root, (root_groups, line_groups)) in m.context.leader_roots.iter().zip(&theirs.roots) {
            let mut expect = vec![(290, b(root.content_valid)), (291, b(root.unknown_flag))];
            p3(&mut expect, 10, root.connection_point);
            p3(&mut expect, 11, root.direction);
            expect.extend([(90, root.index.to_string()), (40, n(root.landing_distance))]);
            let (c, bb) = walk(&expect, root_groups, "leader");
            compared += c;
            bad.extend(bb);
            if root.lines.len() != line_groups.len() {
                bad.push(format!(
                    "leader lines: dwg {} dxf {}",
                    root.lines.len(),
                    line_groups.len()
                ));
            }
            for (line, groups) in root.lines.iter().zip(line_groups) {
                let mut expect = Vec::new();
                for p in &line.points {
                    p3(&mut expect, 10, *p);
                }
                expect.push((91, line.index.to_string()));
                let (c, bb) = walk(&expect, groups, "leader line");
                compared += c;
                bad.extend(bb);
            }
        }
        let (c3, b3) = walk(&tail_expect(&m, version), &theirs.tail, "entity");
        compared += c3;
        bad.extend(b3);
        for line in &bad {
            println!("{name} ({version}) MULTILEADER 0x{handle:X} {line}");
        }
        differ += bad.len();
    }
    let mut line = format!("{name} ({version}) MULTILEADER: {records} records, {closed} close");
    if twin.is_some() {
        line.push_str(&format!(
            "; DXF: {compared} values compared, {differ} differ"
        ));
    }
    println!("{line}");
    if closed < records || differ > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
