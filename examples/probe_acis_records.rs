//! Census every §20.4.41 ACIS record (REGION / 3DSOLID / BODY) in a
//! corpus and arbitrate the width of the R2013+ `has AcDs binary data`
//! marker from it.
//!
//! # What this proves
//!
//! Two facts, in one pass.
//!
//! **1 — the census.** How many ACIS records the corpus actually holds,
//! which release each belongs to, and which of them set the R2013+
//! `has AcDs binary data` bit of the common entity data (§20.4.1). On
//! the 19-file corpus the answer is three records, all in
//! `sample_AC1032.dwg`, all setting the bit.
//!
//! **2 — the arbitration (#54).** The spec lists that bit and stops; it
//! does not say what, if anything, follows when it is set. This crate
//! carried three different readings — `common_entity` consumed 0 bits,
//! `objects::modern` 16, `tables::modern` an `RC`. The probe re-reads
//! the common entity preamble with each candidate width, then runs the
//! §20.4.41 field list ([`dwg::entities::modeler::decode_record`]) from
//! wherever that lands, and prints how far the list finishes from the
//! record's own data-stream boundary. **Only `delta 0` is a valid
//! reading**, and only one width produces it.
//!
//! ```sh
//! cargo run --release --example probe_acis_records -- samples/
//! cargo run --release --example probe_acis_records -- samples/sample_AC1032.dwg
//! ```
//!
//! Expected output on the corpus (abridged):
//!
//! ```text
//! sample_AC1032.dwg 0xD65 3DSOLID ds=true data_end=437
//!    marker  0 bits: preamble ends 82   colour 0x0100 lts 1 invis 0 lw 0x1D
//!                      delta 0     isolines 4 point (17.7767, -220.8501, 2.5000) guid 833111a1-…
//!    marker  8 bits: preamble does not read at all (…)
//!    marker 16 bits: preamble ends 164  colour 0xEE10 lts 0 invis 2640 lw 0x12
//!                      field list does not close (…)
//! ```
//!
//! Two things fail at once for every non-zero width: the *preamble
//! values* stop being the ones every other entity in the file carries
//! (`colour 0x0100`, `linetype scale 1.0`, `invisibility 0`,
//! `lineweight 0x1D` — BYLAYER throughout), and the §20.4.41 list that
//! follows cannot close.

use dwg::bitcursor::BitCursor;
use dwg::entities::modeler;
use dwg::error::Result;
use dwg::{DwgFile, ObjectType, Version};
use std::path::PathBuf;
use std::process::ExitCode;

/// Candidate widths, in bits, for whatever follows the R2013+
/// `has AcDs binary data` flag — the three readings this crate has
/// carried at one time or another.
const CANDIDATE_MARKER_BITS: [usize; 3] = [0, 8, 16];

/// The preamble fields whose decoded values are the second half of
/// the arbitration: on this file every entity carries `colour 0x0100`,
/// `linetype scale 1.0`, `invisibility 0`, `lineweight 0x1D`.
struct Preamble {
    has_ds_binary_data: bool,
    colour: u16,
    linetype_scale: f64,
    invisibility: i16,
    lineweight: u8,
}

/// Re-read the common entity preamble (§20.4.1) with an explicit
/// AcDs-marker width, so the candidate readings can be compared
/// side by side.
///
/// Mirrors `common_entity::read_common_entity_data` field for field.
/// It is kept local to the probe deliberately: the measurement must
/// not move when the library's own reading changes.
fn read_preamble(c: &mut BitCursor<'_>, version: Version, marker_bits: usize) -> Result<Preamble> {
    // EED chain.
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
    // Graphics-preview block.
    if c.read_b()? {
        let bytes = if version.is_r2010_plus() {
            c.read_bll()?
        } else {
            c.read_rl()? as u64
        };
        for _ in 0..bytes {
            let _ = c.read_rc()?;
        }
    }
    let _entmode = c.read_bb()?;
    let _num_reactors = c.read_bl()?;
    if version.is_r2004_plus() {
        let _no_xdictionary = c.read_b()?;
    }
    let mut has_ds_binary_data = false;
    if matches!(version, Version::R2013 | Version::R2018) {
        has_ds_binary_data = c.read_b()?;
        if has_ds_binary_data {
            for _ in 0..marker_bits {
                let _ = c.read_b()?;
            }
        }
    }
    // ENC entity colour (§2.11).
    let colour = c.read_bs_u()?;
    if version.is_r2004_plus() {
        let flags = colour >> 8;
        if flags & 0x20 != 0 {
            let _transparency = c.read_bl()?;
        }
        if flags & 0x40 == 0 && flags & 0x80 != 0 {
            let _rgb = c.read_bl()?;
        }
    }
    let linetype_scale = c.read_bd()?;
    let _ltype_flags = c.read_bb()?;
    let _plotstyle_flags = c.read_bb()?;
    if version.is_r2007_plus() {
        let _material_flags = c.read_bb()?;
        let _shadow_flags = c.read_rc()?;
    }
    if version.is_r2010_plus() {
        let _has_full_visualstyle = c.read_b()?;
        let _has_face_visualstyle = c.read_b()?;
        let _has_edge_visualstyle = c.read_b()?;
    }
    let invisibility = c.read_bs()?;
    let lineweight = if matches!(version, Version::R14) {
        0
    } else {
        c.read_rc()?
    };
    Ok(Preamble {
        has_ds_binary_data,
        colour,
        linetype_scale,
        invisibility,
        lineweight,
    })
}

/// Render a 16-byte revision GUID in canonical UUID form.
fn format_guid(guid: &[u8; 16]) -> String {
    let hex: String = guid.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn dwg_files(path: &PathBuf) -> Vec<PathBuf> {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dwg"))
            .collect();
        files.sort();
        files
    } else {
        vec![path.clone()]
    }
}

fn main() -> ExitCode {
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!("usage: probe_acis_records <dir-or-file.dwg>");
        return ExitCode::FAILURE;
    };
    let files = dwg_files(&PathBuf::from(arg));
    if files.is_empty() {
        eprintln!("no .dwg files found");
        return ExitCode::FAILURE;
    }

    let mut total = 0usize;
    for path in &files {
        let Ok(file) = DwgFile::open(path) else {
            continue;
        };
        let version = file.version();
        let Some(Ok(objects)) = file.all_objects() else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        for object in &objects {
            let kind = ObjectType::from_code(object.type_code);
            if !matches!(
                kind,
                ObjectType::Solid3d | ObjectType::Region | ObjectType::Body
            ) {
                continue;
            }
            total += 1;
            let Some(data_end) = dwg::object::data_end_bit(object, version) else {
                println!("{name} 0x{:X} {} — no boundary", object.handle.value, kind);
                continue;
            };

            // The flag value itself does not depend on the marker
            // width, so read it once with the 0-bit candidate.
            let mut probe = match dwg::object::body_cursor(object, version) {
                Ok(c) => c,
                Err(e) => {
                    println!(
                        "{name} 0x{:X}: body cursor failed: {e}",
                        object.handle.value
                    );
                    continue;
                }
            };
            let flag = read_preamble(&mut probe, version, 0)
                .map(|p| p.has_ds_binary_data)
                .unwrap_or(false);
            println!(
                "{name} 0x{:X} {} ds={flag} data_end={data_end}",
                object.handle.value, kind
            );

            for marker_bits in CANDIDATE_MARKER_BITS {
                let Ok(mut c) = dwg::object::body_cursor(object, version) else {
                    continue;
                };
                let preamble = match read_preamble(&mut c, version, marker_bits) {
                    Ok(p) => p,
                    Err(e) => {
                        println!(
                            "   marker {marker_bits:>2} bits: preamble does not read at all ({e})"
                        );
                        continue;
                    }
                };
                let preamble_end = c.position_bits();
                println!(
                    "   marker {marker_bits:>2} bits: preamble ends {preamble_end:<4} \
                     colour {:#06X} lts {} invis {} lw {:#04X}",
                    preamble.colour,
                    preamble.linetype_scale,
                    preamble.invisibility,
                    preamble.lineweight
                );
                match modeler::decode_record(&mut c, version, preamble.has_ds_binary_data) {
                    Ok(record) => {
                        let at = c.position_bits();
                        let delta = at as isize - data_end as isize;
                        let point = record
                            .tail
                            .point
                            .map(|p| format!("({:.4}, {:.4}, {:.4})", p.x, p.y, p.z))
                            .unwrap_or_else(|| "-".into());
                        let guid = record
                            .tail
                            .revision_guid
                            .as_ref()
                            .map(format_guid)
                            .unwrap_or_else(|| "-".into());
                        println!(
                            "                     delta {delta:<5} isolines {} point {point} \
                             guid {guid}",
                            record.tail.num_isolines
                        );
                    }
                    Err(e) => {
                        println!("                     field list does not close ({e})");
                    }
                }
            }
        }
    }
    println!("\n{total} ACIS record(s) across {} file(s)", files.len());
    ExitCode::SUCCESS
}
