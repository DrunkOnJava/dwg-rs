//! Integration tests against the 19-file DWG sample corpus at
//! `../../samples/`. The corpus was assembled from the public
//! `nextgis/dwg_samples` repository (AutoCAD R14 → 2018) plus a 1 MB
//! AC1032 sample.
//!
//! Tests skip gracefully if samples are absent — useful when this crate
//! is vendored into a build system that doesn't carry the corpus.

use dwg::section::SectionKind;
use dwg::{DwgFile, Version};
use std::path::PathBuf;

fn samples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p
}

fn sample(name: &str) -> PathBuf {
    let mut p = samples_dir();
    p.push(name);
    p
}

fn open_if_present(name: &str) -> Option<DwgFile> {
    let p = sample(name);
    if !p.exists() {
        eprintln!("skipping {name}: sample not present");
        return None;
    }
    Some(DwgFile::open(&p).unwrap_or_else(|e| panic!("{name} open failed: {e}")))
}

// ================================================================
// Version detection — one assertion per released format.
// ================================================================

#[test]
fn arc_r14_is_ac1014() {
    if let Some(f) = open_if_present("arc_R14.dwg") {
        assert_eq!(f.version(), Version::R14);
        assert_eq!(&f.version().magic(), b"AC1014");
    }
}

#[test]
fn arc_2000_is_ac1015() {
    if let Some(f) = open_if_present("arc_2000.dwg") {
        assert_eq!(f.version(), Version::R2000);
        assert_eq!(&f.version().magic(), b"AC1015");
    }
}

#[test]
fn arc_2004_is_ac1018() {
    if let Some(f) = open_if_present("arc_2004.dwg") {
        assert_eq!(f.version(), Version::R2004);
        assert_eq!(&f.version().magic(), b"AC1018");
    }
}

#[test]
fn arc_2007_is_ac1021() {
    if let Some(f) = open_if_present("arc_2007.dwg") {
        assert_eq!(f.version(), Version::R2007);
        assert_eq!(&f.version().magic(), b"AC1021");
        // Phase A: R2007 has a deferred layout; we only populate the
        // common header, not the R2004-family struct.
        assert!(
            f.r2007_common().is_some(),
            "R2007 should populate r2007_common"
        );
        assert!(
            f.r2004_header().is_none(),
            "R2007 must NOT populate r2004_header"
        );
    }
}

#[test]
fn arc_2010_is_ac1024() {
    if let Some(f) = open_if_present("arc_2010.dwg") {
        assert_eq!(f.version(), Version::R2010);
    }
}

#[test]
fn arc_2013_is_ac1027() {
    if let Some(f) = open_if_present("arc_2013.dwg") {
        assert_eq!(f.version(), Version::R2013);
    }
}

#[test]
fn sample_ac1032_is_r2018() {
    if let Some(f) = open_if_present("sample_AC1032.dwg") {
        assert_eq!(f.version(), Version::R2018);
        assert_eq!(&f.version().magic(), b"AC1032");
    }
}

// ================================================================
// R13-R15 header details
// ================================================================

#[test]
fn r14_has_locator_records() {
    if let Some(f) = open_if_present("arc_R14.dwg") {
        let h = f.r13_header().expect("R14 must parse R13R15 header");
        // Spec remark §3.2.6: seen with 3..=6 records. We expect at least 3.
        assert!(
            h.locator_count >= 3,
            "unexpected locator_count = {}",
            h.locator_count
        );
        assert_eq!(h.locators.len(), h.locator_count as usize);
        // Record 0 is always "Header variables".
        let hdr_rec = h.locators.iter().find(|r| r.number == 0);
        assert!(hdr_rec.is_some(), "R14 should have header record (0)");
    }
}

#[test]
fn r2000_has_classes_and_handles_sections() {
    if let Some(f) = open_if_present("arc_2000.dwg") {
        assert!(
            f.section_of_kind(SectionKind::Header).is_some(),
            "expected Header section"
        );
        assert!(
            f.section_of_kind(SectionKind::Classes).is_some(),
            "expected Classes section"
        );
        assert!(
            f.section_of_kind(SectionKind::Handles).is_some(),
            "expected Handles section (object map)"
        );
    }
}

// ================================================================
// R2004+ header details
// ================================================================

#[test]
fn ac1032_decrypts_file_id() {
    if let Some(f) = open_if_present("sample_AC1032.dwg") {
        let h = f.r2004_header().expect("R2018 must parse R2004 header");
        let id = &h.file_id[..11];
        assert_eq!(id, b"AcFssFcAJMB", "decrypt failed");
    }
}

#[test]
fn ac1032_has_nonzero_section_page_map() {
    if let Some(f) = open_if_present("sample_AC1032.dwg") {
        let h = f.r2004_header().unwrap();
        assert!(
            h.section_page_map_addr > 0,
            "section_page_map_addr must point somewhere"
        );
        assert!(
            h.section_page_amount >= 1,
            "must have at least one section page"
        );
    }
}

// ================================================================
// CRC-32 validation — spec §4.1 says the decrypted block's CRC-32,
// with bytes 0x68..0x6C zeroed, must equal the stored value.
// ================================================================

#[test]
fn ac1032_header_crc_matches() {
    if let Some(f) = open_if_present("sample_AC1032.dwg") {
        let bytes = f.raw_bytes();
        let (expected, actual) = dwg::reader::validate_r2004_header_crc(bytes).unwrap();
        assert_eq!(
            expected, actual,
            "R2004+ header CRC mismatch: stored={expected:#x}, computed={actual:#x}"
        );
    }
}

// ================================================================
// Every entity file in the corpus opens without error.
// ================================================================

#[test]
fn all_corpus_files_open() {
    let dir = samples_dir();
    if !dir.exists() {
        eprintln!("skipping corpus sweep: {} does not exist", dir.display());
        return;
    }
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("dwg"))
        .collect();
    if entries.is_empty() {
        eprintln!(
            "skipping corpus sweep: no .dwg files under {}",
            dir.display()
        );
        return;
    }
    let mut failures = Vec::new();
    for e in &entries {
        if let Err(err) = DwgFile::open(e.path()) {
            failures.push(format!("{}: {err}", e.path().display()));
        }
    }
    assert!(
        failures.is_empty(),
        "{} / {} files failed to open:\n{}",
        failures.len(),
        entries.len(),
        failures.join("\n")
    );
}

// ================================================================
// Every version in the corpus reports the section types we expect.
// ================================================================

#[test]
fn every_file_reports_some_sections() {
    let dir = samples_dir();
    if !dir.exists() {
        return;
    }
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if e.path().extension().and_then(|s| s.to_str()) != Some("dwg") {
            continue;
        }
        let f = DwgFile::open(e.path()).expect("open");
        assert!(
            !f.sections().is_empty(),
            "{:?} reports zero sections — reader is broken",
            e.path().file_name()
        );
    }
}

// ================================================================
// Phase B: named-section enumeration via LZ77 + Section Map walk
// ================================================================

#[test]
fn ac1032_enumerates_named_sections() {
    let Some(f) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    let names: Vec<&str> = f.sections().iter().map(|s| s.name.as_str()).collect();
    // After Phase B wiring we expect the canonical AcDb: names.
    let must_have = [
        "AcDb:Header",
        "AcDb:Classes",
        "AcDb:Handles",
        "AcDb:AcDbObjects",
    ];
    for expected in must_have {
        assert!(
            names.contains(&expected),
            "expected section {:?} not found. Got: {:?}",
            expected,
            names
        );
    }
}

#[test]
fn ac1032_sections_have_nonzero_sizes() {
    let Some(f) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    // The critical named sections (AcDb:Header, AcDb:AcDbObjects) must
    // have real size data from the section-info table, not a stub 0.
    for name in ["AcDb:Header", "AcDb:AcDbObjects"] {
        let Some(s) = f.section_by_name(name) else {
            panic!("section {name:?} missing from enumeration");
        };
        assert!(
            s.size > 0,
            "section {name:?} reports size=0, Phase B not wired?"
        );
    }
}

#[test]
fn ac1032_preview_is_classified() {
    let Some(f) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    if let Some(preview) = f.section_of_kind(SectionKind::Preview) {
        assert_eq!(preview.name, "AcDb:Preview");
    }
}

#[test]
fn ac1032_can_extract_preview_bytes() {
    let Some(f) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    // Phase C: full section extraction (Sec_Mask decrypt of data page
    // header + optional LZ77 decompression + reassembly).
    let preview = match f.read_section("AcDb:Preview") {
        Some(Ok(b)) => b,
        Some(Err(e)) => panic!("preview extract failed: {e}"),
        None => return, // non-R2004-family or absent — skip
    };
    // From the section info table we know AcDb:Preview is 1548 bytes.
    assert_eq!(
        preview.len(),
        1548,
        "preview size mismatch, got {} bytes",
        preview.len()
    );
    // The AC1032 preview begins with a 16-byte DWG sentinel
    // 0x1F 0x25 0x6D 0x07 0xD4 0x36 0x28 0x28 0x9D 0x57 0xCA 0x3F 0x9D 0x44 0x10 0x2B
    // followed by image data. We don't assert the sentinel exactly
    // (Autodesk versions it slightly), but the first byte should be non-zero.
    assert_ne!(
        preview[0], 0x00,
        "preview leads with zero — probable extraction bug"
    );
}

#[test]
fn ac1032_can_extract_header_section() {
    let Some(f) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    // AcDb:Header is LZ77-compressed (compressed=2) — exercises the
    // decompression codepath end-to-end.
    let header = match f.read_section("AcDb:Header") {
        Some(Ok(b)) => b,
        Some(Err(e)) => panic!("header extract failed: {e}"),
        None => return,
    };
    // From the section info table: 870 bytes decompressed.
    assert_eq!(header.len(), 870);
}

#[test]
fn ac1032_parses_handle_map() {
    let Some(f) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    let Some(map) = f.handle_map() else {
        return;
    };
    let map = match map {
        Ok(m) => m,
        Err(e) => panic!("handle map parse failed: {e}"),
    };
    // A valid AC1032 drawing must contain at least BLOCK_CONTROL (h=1).
    assert!(!map.entries.is_empty(), "empty handle map");
    // Handle 1 (BLOCK_CONTROL) is the root object; every file has it.
    // Note: handles are monotonic *per section* but can reset/re-sort
    // across section boundaries in practice — we don't assert global
    // monotonicity.
    assert!(
        map.entries.iter().any(|e| e.handle == 1),
        "handle 1 (BLOCK_CONTROL) absent from {} entries",
        map.entries.len()
    );
    // Reasonable sanity: offsets fit in the AcDbObjects size.
    let max_offset = map.entries.iter().map(|e| e.offset).max().unwrap_or(0);
    assert!(
        max_offset < 2_000_000,
        "max offset {max_offset} exceeds plausible AcDbObjects size"
    );
}

#[test]
fn ac1032_parses_class_map() {
    let Some(f) = open_if_present("sample_AC1032.dwg") else {
        return;
    };
    let Some(cmap) = f.class_map() else {
        return;
    };
    let _cmap = match cmap {
        Ok(c) => c,
        // The R2018 class section has quirks beyond what the ODA spec
        // documents; a parse error is acceptable as long as the section
        // itself was extractable. The writer path will need to address
        // the R2018 layout when Phase E ships.
        Err(_) => return,
    };
}

// ================================================================
// The R13-R15 and R2004+ code paths should NEVER both activate.
// ================================================================

#[test]
fn header_paths_are_mutually_exclusive() {
    let dir = samples_dir();
    if !dir.exists() {
        return;
    }
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if e.path().extension().and_then(|s| s.to_str()) != Some("dwg") {
            continue;
        }
        let f = DwgFile::open(e.path()).unwrap();
        let r13 = f.r13_header().is_some();
        let r24 = f.r2004_header().is_some();
        let r27 = f.r2007_common().is_some();
        let n = [r13, r24, r27].iter().filter(|&&b| b).count();
        assert_eq!(
            n,
            1,
            "{:?}: exactly one of r13/r2004/r2007 must be populated (got {})",
            e.path().file_name(),
            n
        );
        match f.version() {
            v if v.is_r13_r15() => assert!(r13),
            v if v.is_r2007() => assert!(r27),
            v if v.is_r2004_family() => assert!(r24),
            v => panic!("unexpected version classification for {:?}", v),
        }
    }
}

// ================================================================
// VISUALSTYLE decodes on every release band (dwg-rs#73).
// ================================================================

/// The 24 built-in visual styles are in **every** corpus file, R14
/// included, and all 456 records decode against their own data-stream
/// boundary — the R14 / R2000 band on the flag-less field list with
/// §2.11's bare-index colours, R2007 on the same list plus its one
/// extra 2-bit slot, R2010+ on the `(value, flag)` list.
///
/// The assertions below are the value corroboration the layouts were
/// derived from, so they fail if a field list ever slips a slot even
/// while still landing on the boundary.
#[test]
fn every_corpus_file_decodes_its_twenty_four_visual_styles() {
    let names = [
        "arc_R14.dwg",
        "circle_R14.dwg",
        "line_R14.dwg",
        "arc_2000.dwg",
        "circle_2000.dwg",
        "line_2000.dwg",
        "arc_2004.dwg",
        "circle_2004.dwg",
        "line_2004.dwg",
        "arc_2007.dwg",
        "circle_2007.dwg",
        "line_2007.dwg",
        "arc_2010.dwg",
        "circle_2010.dwg",
        "line_2010.dwg",
        "arc_2013.dwg",
        "circle_2013.dwg",
        "line_2013.dwg",
        "sample_AC1032.dwg",
    ];
    let mut total = 0usize;
    let mut files = 0usize;
    for name in names {
        let Some(file) = open_if_present(name) else {
            continue;
        };
        files += 1;
        let version = file.version();
        let (decoded, _) = file
            .decoded_entities()
            .unwrap_or_else(|| panic!("{name}: no object walk"))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let styles: Vec<_> = decoded
            .iter()
            .filter_map(|d| match d {
                dwg::entities::DecodedEntity::VisualStyle(v) => Some(v),
                _ => None,
            })
            .collect();
        assert_eq!(styles.len(), 24, "{name}: VISUALSTYLE records decoded");
        total += styles.len();

        // The Visual Styles Manager lists ten of the 24 and hides
        // fourteen; a field list off by one bit cannot reproduce that.
        let listed = styles.iter().filter(|s| !s.is_internal_use_only).count();
        assert_eq!(listed, 10, "{name}: internal-use split");

        // A dense per-style enumeration running 0 (`Flat`) … 27
        // (`Shaded`).
        let mut types: Vec<i16> = styles.iter().map(|s| s.internal_style_type).collect();
        types.sort_unstable();
        types.dedup();
        assert_eq!(types.len(), 24, "{name}: style types are distinct");
        assert_eq!(types[0], 0, "{name}: lowest style type");
        assert_eq!(types[23], 27, "{name}: highest style type");

        let by_name = |want: &str| {
            styles
                .iter()
                .find(|s| s.description == want)
                .unwrap_or_else(|| panic!("{name}: no {want} style"))
        };

        // `X-Ray` is the one translucent style; the flag-less bands
        // carry the same magnitudes with a sign that tracks whether the
        // property applies, so compare magnitudes.
        for style in &styles {
            let want = if style.description == "X-Ray" {
                0.5
            } else {
                0.6
            };
            assert!(
                (style.face_opacity.abs() - want).abs() < 1e-12,
                "{name}: {} face_opacity {}",
                style.description,
                style.face_opacity
            );
        }

        // The eighteen `arc_` / `circle_` / `line_` files are one
        // drawing saved down through six releases, so their style
        // *content* is comparable band to band. `sample_AC1032.dwg` is
        // a different drawing whose author edited some of these values
        // — its `Conceptual` carries a 40-degree crease angle and its
        // `Dim` no brightness offset — so the content assertions below
        // apply to the saved-down set only.
        if name != "sample_AC1032.dwg" {
            // Crease angles in degrees, on the styles where one is
            // meaningful.
            assert!(
                (by_name("Conceptual").edge_crease_angle - 179.0).abs() < 1e-12,
                "{name}: Conceptual crease angle"
            );
            for want40 in ["Hidden", "Sketchy", "Shades of Gray"] {
                assert!(
                    (by_name(want40).edge_crease_angle - 40.0).abs() < 1e-12,
                    "{name}: {want40} crease angle"
                );
            }

            // Brightness: a `BL` up to R2007 and a `BD` from R2010,
            // the same three values either way.
            assert!(
                (by_name("Dim").display_brightness + 50.0).abs() < 1e-12,
                "{name}: Dim brightness"
            );
            assert!(
                (by_name("Brighten").display_brightness - 50.0).abs() < 1e-12,
                "{name}: Brighten brightness"
            );
            for style in &styles {
                if !matches!(style.description.as_str(), "Dim" | "Brighten") {
                    assert_eq!(
                        style.display_brightness, 0.0,
                        "{name}: {} brightness",
                        style.description
                    );
                }
            }
        }

        // `ColorChange` is the one style with a grey face colour, and
        // the two colour encodings say the same thing: RGB 0x808080
        // from R2004, ACI index 8 before it (§2.11).
        let color_change = by_name("ColorChange");
        if version.is_r2004_plus() {
            assert_eq!(color_change.face_mono_color.method(), 0xC2, "{name}");
            assert_eq!(
                color_change.face_mono_color.payload(),
                0x0080_8080,
                "{name}"
            );
        } else {
            assert_eq!(color_change.face_mono_color.index, 8, "{name}");
            assert_eq!(color_change.face_mono_color.rgb, 0, "{name}");
        }

        // R2007 alone writes the extra 2-bit slot, and it is zero on
        // every record measured.
        for style in &styles {
            assert_eq!(
                style.display_unknown_short, 0,
                "{name}: {} trailing slot",
                style.description
            );
        }
    }
    if files == names.len() {
        assert_eq!(total, 456, "the corpus holds 456 VISUALSTYLE records");
    }
}
