//! `build_fixtures` — regenerate the canonical CI corpus under
//! `tests/fixtures/canonical/`.
//!
//! # What fact this proves
//!
//! That the in-tree write path ([`dwg::file_writer::assemble_dwg_bytes`]
//! Stages 1-5, [`dwg::handle_map::write_handle_map`], and the
//! [`dwg::element_encoder::ElementEncoder`] entity encoders) can emit a
//! complete R2004-family DWG byte buffer whose **entities this crate's
//! own reader decodes end-to-end** — section map → `AcDb:Handles` →
//! handle-driven `ObjectWalker` → common-entity preamble → per-entity
//! decoder. Until this example existed, `tests/fixtures/canonical/` was
//! empty and the CI `coverage-smoke` job skipped with a warning,
//! providing zero regression protection (task #94).
//!
//! The files are **self-generated** — no byte of any Autodesk-, ODA-, or
//! third-party-produced DWG is redistributed. They are Apache-2.0, same
//! as the rest of this repository.
//!
//! # How to verify
//!
//! ```bash
//! cargo run --release --example build_fixtures
//! cargo run --release --example coverage_report -- tests/fixtures/canonical
//! ```
//!
//! Regenerating is byte-deterministic: re-running the first command on an
//! unchanged tree produces identical files (`git status` stays clean).
//! `tests/canonical_corpus.rs` pins the exact decode outcome per file.
//!
//! # Scope + honesty note
//!
//! These fixtures exercise **our writer against our reader**. They are
//! NOT proof that AutoCAD, BricsCAD, or LibreCAD accept the bytes — see
//! the acceptance statement in `src/file_writer.rs`. They also cover only
//! the four entity types that have [`ElementEncoder`] implementations
//! (LINE, CIRCLE, ARC, POINT); the corpus should grow as more encoders
//! land.
//!
//! Two container families are emitted. The R2004 family goes through
//! [`assemble_dwg_bytes`] (page map + section info + LZ77). R2000 goes
//! through [`build_flat_fixture`], which writes the §3.2.6 flat
//! section-locator table directly — those releases have no page map, no
//! compression and no section headers, so the "container" is three
//! locator records and a byte layout.
//!
//! R14 is not emitted: its entity records need the §20.4.1 R13/R14
//! preamble and the §20.4.21 `3BD` LINE body, neither of which the
//! [`ElementEncoder`] implementations write. R2007 is not emitted
//! either — the §5.1-§5.4 container is implemented read-only, and a
//! writer for it would have to produce Reed-Solomon codewords and a
//! §5.10-compressed stream this crate has no encoder for.

use dwg::bitwriter::BitWriter;
use dwg::element_encoder::ElementEncoder;
use dwg::entities::arc::Arc;
use dwg::entities::circle::Circle;
use dwg::entities::line::Line;
use dwg::entities::point::Point;
use dwg::entities::{Point3D, Vec3D};
use dwg::file_writer::{WriterScaffold, assemble_dwg_bytes, atomic_write};
use dwg::handle_map::{HandleEntry, HandleMap, write_handle_map};
use dwg::version::Version;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Object type codes (spec §19.4 table). Only the four types with an
/// [`ElementEncoder`] implementation are emitted.
const TYPE_ARC: u16 = 0x11;
const TYPE_CIRCLE: u16 = 0x12;
const TYPE_LINE: u16 = 0x13;
const TYPE_POINT: u16 = 0x1B;

/// R2018 `AcDb:AcDbObjects` streams open with an undocumented RL that
/// the walker skips (spec note: "starts with a RL value of 0x0dca").
const OBJECTS_STREAM_PREFIX: [u8; 4] = [0xCA, 0x0D, 0x00, 0x00];

/// Default extrusion `(0, 0, 1)` — collapses to a single `true` bit in
/// the BE encoding of spec §2.11.
fn default_extrusion() -> Vec3D {
    Vec3D {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    }
}

/// Write the common entity preamble (spec §19.4.1) — the exact inverse
/// of `dwg::common_entity::read_common_entity_data` for the given
/// version. Emits the "boring" entity: no XDATA, no graphics preview,
/// `ByLayer` mode, no reactors, BYLAYER colour, unit linetype scale,
/// zero invisibility and lineweight.
fn write_common_entity_preamble(w: &mut BitWriter, version: Version) {
    // Extended entity data: a zero-length record terminates the loop.
    w.write_bs_u(0);
    // No graphics preview.
    w.write_b(false);
    // Entity mode: 0b00 = ByLayer.
    w.write_bb(0b00);
    // num_reactors = 0.
    w.write_bl(0);
    // R2004+: no xdictionary handle.
    if version.is_r2004_plus() {
        w.write_b(true);
    }
    // R2013+: no DS binary data chain.
    if matches!(version, Version::R2013 | Version::R2018) {
        w.write_b(false);
    }
    // R13-R2000 carry a `Nolinks` bit before the colour (§20.4.1);
    // R2004+ do not write it separately.
    if version.is_r13_r15() {
        w.write_b(true);
    }
    // CMC colour — raw 0 means BYLAYER with no alpha/RGB/name suffix.
    w.write_bs(0);
    // BD linetype_scale = 1.0.
    w.write_bd(1.0);
    // BB ltype_flags, BB plotstyle_flag.
    w.write_bb(0b00);
    w.write_bb(0b00);
    // R2007+: BB material_flag, RC shadow_flags.
    if version.is_r2007_plus() {
        w.write_bb(0b00);
        w.write_rc(0);
    }
    // R2010+: three visual-style presence bits.
    if version.is_r2010_plus() {
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
    }
    // BS invisibility, RC lineweight.
    w.write_bs(0);
    w.write_rc(0);
}

/// Build one `AcDb:AcDbObjects` record: `MS size` + payload + 2-byte CRC
/// slot, where the payload is the object header (R2010+ handle-stream MC,
/// object type, handle), the common entity preamble, and the entity body.
///
/// Mirrors `dwg::object::ObjectWalker::read_one_at_pos` field-for-field.
/// The 2 trailing bytes are the record CRC-16 slot; the reader skips over
/// it without verifying, so zeros are a faithful placeholder.
fn build_object_record<E: ElementEncoder>(
    type_code: u16,
    handle: u64,
    version: Version,
    entity: &E,
) -> Vec<u8> {
    // The R2000-R2007 `RL` object-data-size-in-bits can only be known
    // once the body is written, and it is a fixed-width field, so the
    // payload is built twice: once with a placeholder to measure, then
    // again with the measured value. Both passes produce the same
    // number of bits.
    let emit = |obj_size_bits: u32| -> BitWriter {
        let mut w = BitWriter::new();
        // R2010+: MC handle-stream-size-in-bits. We emit no handle stream,
        // so the value is 0 — a single 0x00 byte with no continuation bit.
        if version.is_r2010_plus() {
            w.write_rc(0x00);
        }
        // Object type: R2010+ uses a 2-bit dispatch tag (00 = one raw byte
        // follows); earlier R2004-family releases use a plain BS.
        if version.is_r2010_plus() {
            w.write_bb(0b00);
            w.write_rc(type_code as u8);
        } else {
            w.write_bs(type_code as i16);
        }
        // R2000-R2007 (§19.1): object data size in bits, between the
        // object type and the object handle.
        if version.has_object_size_field() {
            w.write_rl(obj_size_bits);
        }
        // Object handle: 4-bit code + 4-bit counter + counter bytes.
        w.write_handle(0, handle);
        write_common_entity_preamble(&mut w, version);
        entity
            .encode(&mut w, version)
            .expect("ElementEncoder impls for LINE/CIRCLE/ARC/POINT are infallible");
        w
    };
    let obj_size_bits = emit(0).position_bits() as u32;
    let payload = emit(obj_size_bits).into_bytes();

    let mut header = BitWriter::new();
    header.write_ms(payload.len() as u64);
    let mut record = header.into_bytes();
    record.extend_from_slice(&payload);
    record.extend_from_slice(&[0x00, 0x00]);
    record
}

/// The fixture's entity set — one of each encodable type, with values
/// chosen so a mis-aligned decode produces visibly wrong numbers rather
/// than plausible zeros.
fn fixture_entities() -> (Line, Line, Circle, Arc, Point) {
    let line_2d = Line {
        start: Point3D {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        end: Point3D {
            x: 100.0,
            y: 50.0,
            z: 0.0,
        },
        thickness: 0.0,
        extrusion: default_extrusion(),
        is_2d: true,
    };
    let line_3d = Line {
        start: Point3D {
            x: 1.0,
            y: 3.0,
            z: 5.0,
        },
        end: Point3D {
            x: 3.0,
            y: 7.0,
            z: 11.0,
        },
        thickness: 2.5,
        extrusion: Vec3D {
            x: 0.5,
            y: 0.25,
            z: 0.75,
        },
        is_2d: false,
    };
    let circle = Circle {
        center: Point3D {
            x: 10.0,
            y: 20.0,
            z: 0.0,
        },
        radius: 5.0,
        thickness: 0.0,
        extrusion: default_extrusion(),
    };
    let arc = Arc {
        center: Point3D {
            x: -4.0,
            y: 8.0,
            z: 0.0,
        },
        radius: 12.5,
        thickness: 0.0,
        extrusion: default_extrusion(),
        start_angle: 0.0,
        end_angle: std::f64::consts::FRAC_PI_2,
    };
    let point = Point {
        position: Point3D {
            x: 1.25,
            y: 2.5,
            z: 3.75,
        },
        thickness: 0.0,
        extrusion: default_extrusion(),
        x_axis_angle: 0.0,
    };
    (line_2d, line_3d, circle, arc, point)
}

/// The `(handle, record)` list both container assemblers share.
fn build_object_records(version: Version) -> Vec<(u64, Vec<u8>)> {
    let (line_2d, line_3d, circle, arc, point) = fixture_entities();
    vec![
        (
            0x20,
            build_object_record(TYPE_LINE, 0x20, version, &line_2d),
        ),
        (
            0x21,
            build_object_record(TYPE_LINE, 0x21, version, &line_3d),
        ),
        (
            0x22,
            build_object_record(TYPE_CIRCLE, 0x22, version, &circle),
        ),
        (0x23, build_object_record(TYPE_ARC, 0x23, version, &arc)),
        (0x24, build_object_record(TYPE_POINT, 0x24, version, &point)),
    ]
}

/// Build the R2004-family `AcDb:AcDbObjects` section: the `0x0dca`
/// prefix, the records back to back, and a handle map whose offsets are
/// **section-relative** — which is what distinguishes it from the flat
/// layout, where they are absolute file offsets.
fn build_object_stream(version: Version) -> (Vec<u8>, HandleMap) {
    let records = build_object_records(version);
    let mut stream = OBJECTS_STREAM_PREFIX.to_vec();
    let mut entries = Vec::with_capacity(records.len());
    for (handle, record) in &records {
        entries.push(HandleEntry {
            handle: *handle,
            offset: stream.len() as u64,
        });
        stream.extend_from_slice(record);
    }
    (stream, HandleMap { entries })
}

/// Assemble one canonical fixture for `version`. Returns the complete
/// DWG byte buffer.
fn build_fixture(version: Version) -> Result<Vec<u8>, String> {
    let (objects, handles) = build_object_stream(version);
    let handle_bytes = write_handle_map(&handles, &mut BitWriter::new(), version)
        .map_err(|e| format!("write_handle_map: {e}"))?;

    // A small placeholder `AcDb:Header`. The reader does not need it to
    // walk entities, but every real drawing has one and its presence
    // keeps the section-info table representative. 64 bytes clears the
    // LZ77 encoder's 1..=3 unencodable-length gap.
    let header_payload = vec![0x00u8; 64];

    let mut scaffold = WriterScaffold::new(version);
    scaffold.add_section("AcDb:Header", header_payload);
    scaffold.add_section("AcDb:Handles", handle_bytes);
    scaffold.add_section("AcDb:AcDbObjects", objects);

    let built = scaffold
        .build_sections()
        .map_err(|e| format!("build_sections: {e}"))?;
    assemble_dwg_bytes(&built, version).map_err(|e| format!("assemble_dwg_bytes: {e}"))
}

/// Assemble a flat R13-R15 fixture (§3.1, §3.2.6).
///
/// These releases have no section *map*: the file header carries a
/// short list of `(record number, absolute seeker, size)` locators, the
/// object records sit loose in the file, and the object map addresses
/// them by **absolute file offset**. So the fixture is laid out
/// directly rather than assembled by [`WriterScaffold`].
///
/// ```text
/// 0x00  magic, codepage, locator count, three locator records
/// 0x100 AcDb:Header placeholder      -- locator 0
/// 0x140 object records, back to back -- addressed by the object map
/// ...   AcDb:Handles object map      -- locator 2
/// ```
fn build_flat_fixture(version: Version) -> Result<Vec<u8>, String> {
    /// Where the placeholder header block starts.
    const HEADER_AT: usize = 0x100;
    /// Size of that placeholder.
    const HEADER_LEN: usize = 64;
    /// Where the first object record starts.
    const OBJECTS_AT: usize = HEADER_AT + HEADER_LEN;

    let records = build_object_records(version);
    let mut objects = Vec::new();
    let mut entries = Vec::with_capacity(records.len());
    for (handle, record) in &records {
        entries.push(HandleEntry {
            handle: *handle,
            offset: (OBJECTS_AT + objects.len()) as u64,
        });
        objects.extend_from_slice(record);
    }
    let handles = HandleMap { entries };
    let handle_bytes = write_handle_map(&handles, &mut BitWriter::new(), version)
        .map_err(|e| format!("write_handle_map: {e}"))?;

    let map_at = OBJECTS_AT + objects.len();
    let mut file = vec![0u8; map_at + handle_bytes.len()];
    file[..6].copy_from_slice(&version.magic());
    // Byte 0x0C is "0x00, 0x01, or 0x03" per §3.2.1; real files write 1.
    file[0x0C] = 0x01;
    file[0x13..0x15].copy_from_slice(&30u16.to_le_bytes()); // DWGCODEPAGE
    file[0x15..0x19].copy_from_slice(&3u32.to_le_bytes());
    let locators: [(u8, u32, u32); 3] = [
        (0, HEADER_AT as u32, HEADER_LEN as u32),
        (1, 0, 0),
        (2, map_at as u32, handle_bytes.len() as u32),
    ];
    for (i, (number, seeker, size)) in locators.iter().enumerate() {
        let at = 0x19 + i * 9;
        file[at] = *number;
        file[at + 1..at + 5].copy_from_slice(&seeker.to_le_bytes());
        file[at + 5..at + 9].copy_from_slice(&size.to_le_bytes());
    }
    file[OBJECTS_AT..map_at].copy_from_slice(&objects);
    file[map_at..].copy_from_slice(&handle_bytes);
    Ok(file)
}

/// Canonical corpus directory, resolved relative to the crate root so
/// the example works from any current directory.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/canonical")
}

fn main() -> ExitCode {
    let out_dir = corpus_dir();
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("build_fixtures: cannot create {}: {e}", out_dir.display());
        return ExitCode::FAILURE;
    }

    // R2000 uses the §3.2.6 flat locator table; the rest go through
    // the R2004-family page-map assembler.
    let versions = [
        Version::R2000,
        Version::R2004,
        Version::R2010,
        Version::R2013,
        Version::R2018,
    ];

    for version in versions {
        let built = if version.is_r13_r15() {
            build_flat_fixture(version)
        } else {
            build_fixture(version)
        };
        let bytes = match built {
            Ok(b) => b,
            Err(e) => {
                eprintln!("build_fixtures: {version} failed: {e}");
                return ExitCode::FAILURE;
            }
        };
        let name = format!("synthetic_{}.dwg", version.release().to_lowercase());
        let path = out_dir.join(&name);
        if let Err(e) = atomic_write(&path, &bytes) {
            eprintln!("build_fixtures: writing {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
        println!("wrote {:<28} {:>6} bytes  ({version})", name, bytes.len());
    }

    println!();
    println!("Verify with:");
    println!("  cargo run --release --example coverage_report -- tests/fixtures/canonical");
    ExitCode::SUCCESS
}
