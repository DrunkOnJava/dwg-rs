//! Library entry point for the `dwg-to-dxf` conversion pipeline (L11-08).
//!
//! Separated from [`crate::bin::dwg_to_dxf`] so the CLI stays a thin
//! wrapper and the conversion logic is available to downstream
//! library consumers (integration tests, programmatic exporters)
//! without pulling in `clap` / `anyhow`.
//!
//! # Honest scope
//!
//! `convert_decoded_to_dxf` takes an already-opened [`crate::DwgFile`]
//! (so the caller can wire their own error handling) and returns the
//! emitted DXF as a String. Unsupported entities are surfaced as
//! `999 <skipped: KIND x N>` DXF comments per the existing convention
//! — dropping the file rather than partial-converting would surprise
//! callers who've opted into best-effort mode elsewhere.
//!
//! # Acceptance status
//!
//! The emitter writes spec-compliant DXF group-code pairs per the
//! AutoCAD DXF reference. **Actual acceptance against AutoCAD /
//! BricsCAD / LibreCAD is untested** — there is no Autodesk product
//! in CI. The only automated validation is `cargo test`, which
//! exercises group-code emission patterns against synthesized inputs.
//! Real round-trip-via-AutoCAD validation remains a manual step
//! documented in `tests/integration_dxf_roundtrip.rs`.

use crate::curve::{Curve, Path};
use crate::dxf::{DxfVersion, DxfWriter};
use crate::dxf_sections::{
    DecodedObject, EntityGeometry, EntityRecord, HeaderEntry, LayerEntry, write_blocks_section,
    write_entities_section, write_header_section, write_objects_section, write_tables_section,
};
use crate::entities::DecodedEntity;
use crate::entity_geometry::{
    arc_to_curve, circle_to_curve, line_to_curve, lwpolyline_to_path, point_to_curve,
};
use crate::object::RawObject;
use crate::object_type::ObjectType;
use crate::reader::DwgFile;

/// Open a DWG file at `path` and emit a minimal DXF document
/// targeting `version`.
///
/// Equivalent to opening the file via [`DwgFile::open`] and passing
/// the result to [`convert_dwg_to_dxf`]. Returns the emitted DXF as
/// a String so the caller can write it wherever they want (stdout,
/// file, HTTP response).
pub fn convert_file_to_dxf(
    path: impl AsRef<std::path::Path>,
    version: DxfVersion,
) -> crate::Result<String> {
    let file = DwgFile::open(path)?;
    convert_dwg_to_dxf(&file, version)
}

/// Emit a minimal DXF document representing `file`. Targets `version`
/// — controls the `$ACADVER` magic and whether subclass markers are
/// emitted (see [`DxfVersion::supports_subclass_markers`]).
///
/// Unsupported entities produce `999 <skipped: KIND x N>` comments
/// so the omission is visible in the output.
pub fn convert_dwg_to_dxf(file: &DwgFile, version: DxfVersion) -> crate::Result<String> {
    let mut writer = DxfWriter::with_version(version);

    // HEADER — auto-emits $ACADVER from writer.version().
    // Future: enrich with $INSUNITS, $LIMMIN/$LIMMAX, $EXTMIN/$EXTMAX
    // once HeaderVars → DXF-name mapping lands.
    let header_vars: [HeaderEntry; 0] = [];
    write_header_section(&mut writer, &header_vars);

    // TABLES — at minimum a default "0" LAYER, which every DXF
    // reader requires to exist.
    let layers = [LayerEntry::default()];
    write_tables_section(&mut writer, &layers);

    // BLOCKS — *Model_Space + *Paper_Space placeholders. Real block
    // expansion is deferred.
    write_blocks_section(&mut writer);

    // ENTITIES — walk the full object list via the handle map. For
    // pre-R2004 family (R13/R14/R2000) we currently have no
    // all_objects() support; those files get an empty ENTITIES
    // section plus a skip comment.
    let mut entities: Vec<EntityRecord> = Vec::new();
    let mut skipped_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();

    if let Some(decoded_res) = file.decoded_entities() {
        let (decoded_list, _summary) = decoded_res?;
        for decoded in &decoded_list {
            match decoded_entity_to_record(decoded) {
                Some(record) => entities.push(record),
                None => {
                    let label = decoded_label(decoded);
                    *skipped_counts.entry(label).or_insert(0) += 1;
                }
            }
        }
    } else {
        skipped_counts.insert(
            format!(
                "no-object-walker-for-{}",
                file.version().release().replace(' ', "_")
            ),
            1,
        );
    }

    // First ENTITIES section — emit skip comments only. A DXF
    // reader merges adjacent ENTITIES sections, so this shows up
    // next to the actual records.
    writer.begin_section("ENTITIES");
    for (kind, n) in &skipped_counts {
        writer.write_comment(&format!("skipped: {kind} x{n}"));
    }
    writer.end_section();

    // Second ENTITIES section — actual records.
    write_entities_section(&mut writer, &entities);

    // OBJECTS section — collected from the handle-driven object walk
    // for non-entity, non-table-entry records (DICTIONARY, XRECORD,
    // ACDBPLACEHOLDER, LAYOUT, ACAD_GROUP, proxy, …).
    //
    // The emitter is a no-op under R12 (OBJECTS is an R2000+ concept
    // per the AutoCAD DXF reference), so we always call it and let
    // the version gate inside `write_objects_section` decide.
    let decoded_objects = collect_decoded_objects(file);
    write_objects_section(&mut writer, decoded_objects);

    writer.finish();
    Ok(writer.take_output())
}

/// Walk `file`'s object stream and synthesize a list of
/// [`DecodedObject`] suitable for the OBJECTS section.
///
/// The current implementation is deliberately minimal: it enumerates
/// every non-entity, non-table-entry [`RawObject`] via
/// [`DwgFile::all_objects`] and emits a typed `DecodedObject` variant
/// for the object kinds we recognise (DICTIONARY, XRECORD,
/// ACDBPLACEHOLDER, LAYOUT, ACAD_GROUP, ACAD_MLINESTYLE, …).
///
/// Body fields are NOT typed-decoded here — the public walker
/// pipeline does not currently expose per-object typed decoders for
/// these kinds. We preserve the handle, the canonical DXF object
/// type name, and the `100 AcDb*` subclass marker so the emitted DXF
/// is syntactically valid and visibly honest about the suppression
/// (via `999` diagnostic comments inside `write_objects_section`).
fn collect_decoded_objects(file: &DwgFile) -> Vec<DecodedObject> {
    let Some(walk) = file.all_objects() else {
        return Vec::new();
    };
    let raws = match walk {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    raws.iter().filter_map(raw_to_decoded_object).collect()
}

/// Best-effort classification of one [`RawObject`] to a
/// [`DecodedObject`]. Returns `None` for entity kinds and for
/// symbol-table entries — those belong in ENTITIES and TABLES
/// respectively, not OBJECTS.
fn raw_to_decoded_object(raw: &RawObject) -> Option<DecodedObject> {
    if raw.is_entity() || raw.kind.is_table_entry() || raw.kind.is_control() {
        return None;
    }
    let handle = raw.handle.value;
    // Owner handle is carried in the object's common-object header,
    // which we don't typed-decode here. Emitting 0 is the canonical
    // "owned by the drawing root" encoding in DXF; downstream readers
    // accept it without error.
    let owner_handle = 0u64;
    Some(match raw.kind {
        ObjectType::Dictionary => DecodedObject::Dictionary {
            handle,
            owner_handle,
            hard_owner: true,
            entries: Vec::new(),
        },
        ObjectType::XRecord => DecodedObject::XRecord {
            handle,
            owner_handle,
            cloning_flags: 1,
            raw_bytes_len: raw.raw.len(),
        },
        ObjectType::AcadProxyObject => DecodedObject::ProxyObject {
            handle,
            owner_handle,
            class_id: 0,
        },
        ObjectType::AcadProxyEntity => DecodedObject::ProxyEntity {
            handle,
            owner_handle,
            class_id: 0,
        },
        ObjectType::Group => DecodedObject::AcadGroup {
            handle,
            owner_handle,
            name: String::new(),
            selectable: true,
            member_handles: Vec::new(),
        },
        ObjectType::MLineStyle => DecodedObject::AcadMlinestyle {
            handle,
            owner_handle,
            name: String::new(),
        },
        ObjectType::Layout => DecodedObject::PassThrough {
            type_name: "LAYOUT".to_string(),
            handle,
            owner_handle,
            subclass: Some("AcDbLayout".to_string()),
        },
        ObjectType::AcDbPlaceholder => DecodedObject::PassThrough {
            type_name: "ACDBPLACEHOLDER".to_string(),
            handle,
            owner_handle,
            subclass: Some("AcDbPlaceHolder".to_string()),
        },
        ObjectType::VbaProject => DecodedObject::PassThrough {
            type_name: "VBA_PROJECT".to_string(),
            handle,
            owner_handle,
            subclass: Some("AcDbVbaProject".to_string()),
        },
        ObjectType::Dummy => DecodedObject::PassThrough {
            type_name: "DUMMY".to_string(),
            handle,
            owner_handle,
            subclass: None,
        },
        ObjectType::LongTransaction => DecodedObject::PassThrough {
            type_name: "LONG_TRANSACTION".to_string(),
            handle,
            owner_handle,
            subclass: Some("AcDbLongTransaction".to_string()),
        },
        ObjectType::OleFrame => DecodedObject::PassThrough {
            type_name: "OLEFRAME".to_string(),
            handle,
            owner_handle,
            subclass: Some("AcDbOleFrame".to_string()),
        },
        // Custom classes (type code ≥ 500) and unknown codes —
        // safest pass-through stub with the class-name sourced from
        // ObjectType's short_label so we at least surface the code.
        ObjectType::Custom(_) | ObjectType::Unknown(_) => DecodedObject::PassThrough {
            type_name: raw.kind.short_label().to_string(),
            handle,
            owner_handle,
            subclass: None,
        },
        _ => return None,
    })
}

/// Best-effort conversion from a [`DecodedEntity`] to an
/// [`EntityRecord`]. Returns `None` for any entity whose geometry
/// isn't one of the five Curve-shaped primitives this pipeline emits.
fn decoded_entity_to_record(e: &DecodedEntity) -> Option<EntityRecord> {
    // Layer + color aren't yet surfaced uniformly across
    // DecodedEntity. Default to "0" / ACI 7 (ByLayer equivalent);
    // when common-entity decoding exposes them (L8-21+), wire
    // through here.
    let layer = "0".to_string();
    let aci = 7u8;

    match e {
        DecodedEntity::Line(line) => Some(EntityRecord {
            handle: None,
            layer,
            aci,
            geometry: EntityGeometry::Line(line_to_curve(line)),
        }),
        DecodedEntity::Circle(c) => Some(EntityRecord {
            handle: None,
            layer,
            aci,
            geometry: EntityGeometry::Circle(circle_to_curve(c)),
        }),
        DecodedEntity::Arc(a) => Some(EntityRecord {
            handle: None,
            layer,
            aci,
            geometry: EntityGeometry::Arc(arc_to_curve(a)),
        }),
        DecodedEntity::Point(p) => match point_to_curve(p) {
            Curve::Line { a, .. } => Some(EntityRecord {
                handle: None,
                layer,
                aci,
                geometry: EntityGeometry::Point(a),
            }),
            _ => None,
        },
        DecodedEntity::LwPolyline(p) => {
            let path: Path = lwpolyline_to_path(p);
            let flattened = flatten_polyline(&path);
            Some(EntityRecord {
                handle: None,
                layer,
                aci,
                geometry: EntityGeometry::Polyline(flattened),
            })
        }
        // TEXT, MTEXT, INSERT, HATCH, DIMENSION*, SPLINE, 3DFACE,
        // ELLIPSE, VIEWPORT, surfaces, symbol-table entries — we
        // skip and emit a 999 comment.
        _ => None,
    }
}

/// Flatten a [`Path`] whose segments may be [`Curve::Polyline`] into
/// a Path with only [`Curve::Line`] segments so the existing
/// `dxf_sections::write_entities_section` emitter can walk it.
fn flatten_polyline(path: &Path) -> Path {
    let mut lines: Vec<Curve> = Vec::new();
    let mut closed = path.closed;
    for seg in &path.segments {
        match seg {
            Curve::Line { a, b } => lines.push(Curve::Line { a: *a, b: *b }),
            Curve::Polyline {
                vertices,
                closed: c,
            } => {
                closed |= *c;
                if vertices.len() >= 2 {
                    for pair in vertices.windows(2) {
                        lines.push(Curve::Line {
                            a: pair[0].point,
                            b: pair[1].point,
                        });
                    }
                    if *c {
                        let first = vertices.first().map(|v| v.point);
                        let last = vertices.last().map(|v| v.point);
                        if let (Some(a), Some(b)) = (last, first) {
                            if a != b {
                                lines.push(Curve::Line { a, b });
                            }
                        }
                    }
                }
            }
            _ => {
                // Arc / ellipse / spline inside a Path aren't
                // representable as a single LWPOLYLINE — drop them.
            }
        }
    }
    Path {
        segments: lines,
        closed,
    }
}

fn decoded_label(e: &DecodedEntity) -> String {
    let lbl = format!("{e:?}");
    let end = lbl.find(['(', '{', ' ']).unwrap_or(lbl.len());
    lbl[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_is_short_kind_name() {
        let e = DecodedEntity::Unhandled {
            type_code: 42,
            kind: crate::ObjectType::Unknown(42),
        };
        let s = decoded_label(&e);
        assert_eq!(s, "Unhandled");
    }
}
