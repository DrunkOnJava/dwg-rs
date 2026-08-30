//! End-to-end tests for the three container layouts that had no object
//! walk before: R14 / R2000 (the §3.2.6 flat locator table) and R2007
//! (the §5.1-§5.4 page/section map).
//!
//! Every fixture here is **byte-built**: a `BitWriter` or a literal byte
//! array, assembled by the test itself. Nothing is read from the corpus,
//! so the tests run in any checkout. The corpus-gated assertions that
//! pin the same behaviour against real AutoCAD output live at the bottom
//! and skip when `samples/` is absent.

use dwg::bitwriter::BitWriter;
use dwg::entities::DecodedEntity;
use dwg::handle_map::{HandleEntry, HandleMap, write_handle_map};
use dwg::{DwgFile, ObjectType, Version};
use std::path::PathBuf;

// ================================================================
// Bit-level helpers
// ================================================================

/// Append every bit a [`BitWriter`] holds onto another one.
fn append_bits(dst: &mut BitWriter, src: &BitWriter) {
    let bits = src.position_bits();
    let bytes = src.clone().into_bytes();
    for i in 0..bits {
        dst.write_b((bytes[i / 8] >> (7 - (i % 8))) & 1 != 0);
    }
}

/// The LINE body §20.4.21 prescribes for R13/R14: two plain `3BD`s,
/// then the thickness and extrusion defaults.
fn write_r13_r14_line_body(w: &mut BitWriter, start: (f64, f64, f64), end: (f64, f64, f64)) {
    for v in [start.0, start.1, start.2, end.0, end.1, end.2] {
        w.write_bd(v);
    }
    w.write_b(true); // BT thickness: default 0.0
    w.write_b(true); // BE extrusion: default (0, 0, 1)
}

/// The LINE body §20.4.21 prescribes for R2000+: a `Z's are zero` flag,
/// then `RD` / `DD` pairs per axis.
fn write_r2000_line_body(w: &mut BitWriter, start: (f64, f64, f64), end: (f64, f64, f64)) {
    w.write_b(true); // Z's are zero
    w.write_rd(start.0);
    w.write_dd(start.0, end.0);
    w.write_rd(start.1);
    w.write_dd(start.1, end.1);
    w.write_b(true); // BT thickness default
    w.write_b(true); // BE extrusion default
}

/// The common entity preamble from the entity-mode bits on, for the
/// release band the flags describe (§20.4.1).
fn write_common_entity_tail(w: &mut BitWriter, version: Version) {
    w.write_bb(0b10); // entmode: in block
    w.write_bl(0); // num_reactors
    if matches!(version, Version::R14) {
        w.write_b(true); // Isbylayerlt (R13-R14 only)
    }
    w.write_b(true); // Nolinks
    w.write_bs(0); // CMC colour — BS index only up to R2000 (§2.11)
    w.write_bd(1.0); // linetype scale
    if !matches!(version, Version::R14) {
        w.write_bb(0b00); // ltype flags (R2000+)
        w.write_bb(0b00); // plotstyle flags (R2000+)
    }
    w.write_bs(0); // invisibility
    if !matches!(version, Version::R14) {
        w.write_rc(0); // lineweight (R2000+)
    }
}

/// Build one complete object record — leading `MS` byte count, payload,
/// two CRC bytes — for a LINE entity on `version`.
///
/// The `RL` object-data-size is computed, not guessed: the fields after
/// it are laid out first so their bit length is known, then the whole
/// record is written with the real value in place. That is the same
/// quantity the reader checks the decoded field list against, so a
/// record built here is only decodable if the writer and the reader
/// agree on the layout.
fn build_line_record(
    version: Version,
    handle: u64,
    start: (f64, f64, f64),
    end: (f64, f64, f64),
) -> Vec<u8> {
    // Everything from the entity mode bits to the end of the LINE body.
    let mut tail = BitWriter::new();
    write_common_entity_tail(&mut tail, version);
    if matches!(version, Version::R14) {
        write_r13_r14_line_body(&mut tail, start, end);
    } else {
        write_r2000_line_body(&mut tail, start, end);
    }

    // Pass 1: measure where the tail starts.
    let mut probe = BitWriter::new();
    write_object_prefix(&mut probe, version, handle, 0);
    let data_end = probe.position_bits() + tail.position_bits();

    // Pass 2: write it for real.
    let mut payload = BitWriter::new();
    write_object_prefix(&mut payload, version, handle, data_end as u32);
    append_bits(&mut payload, &tail);
    // The handle stream would follow; a byte of padding stands in for it
    // so the record has somewhere for its owner reference to live.
    payload.write_rc(0);
    let payload_bytes = payload.into_bytes();

    let mut record = Vec::new();
    // MS byte count: one 15-bit module is enough for these records.
    record.extend_from_slice(&(payload_bytes.len() as u16).to_le_bytes());
    record.extend_from_slice(&payload_bytes);
    record.extend_from_slice(&[0x00, 0x00]); // CRC — the walker does not verify it
    record
}

/// Write the object header §20.4.1 puts before the entity-mode bits.
///
/// R2000+ carry the `RL` object-data-size between the type code and the
/// handle; R13/R14 carry it after the graphics-present flag instead.
fn write_object_prefix(w: &mut BitWriter, version: Version, handle: u64, data_end_bits: u32) {
    w.write_bs(0x13); // LINE
    if !matches!(version, Version::R14) {
        w.write_rl(data_end_bits);
    }
    w.write_handle(0, handle);
    w.write_bs_u(0); // EED size 0 — no extended data
    w.write_b(false); // no graphics
    if matches!(version, Version::R14) {
        w.write_rl(data_end_bits);
    }
}

/// Assemble a whole R13-R15 file around one object record (§3.1, §3.2.6).
///
/// The object goes at `OBJECT_AT`, the `AcDb:Handles` object map after
/// it, and the five section locators point at both. Handle-map offsets
/// are absolute file offsets on these releases, which is the property
/// the walk depends on.
fn build_flat_file(version: Version, record: &[u8], handle: u64) -> Vec<u8> {
    const OBJECT_AT: usize = 0x100;
    let mut map = BitWriter::new();
    let handles = HandleMap {
        entries: vec![HandleEntry {
            handle,
            offset: OBJECT_AT as u64,
        }],
    };
    let map_bytes = write_handle_map(&handles, &mut map, version).expect("handle map");
    let map_at = OBJECT_AT + record.len() + 0x10;

    let mut file = vec![0u8; map_at + map_bytes.len()];
    file[..6].copy_from_slice(&version.magic());
    file[0x13..0x15].copy_from_slice(&30u16.to_le_bytes()); // codepage
    file[0x15..0x19].copy_from_slice(&3u32.to_le_bytes()); // locator count
    let locators: [(u8, u32, u32); 3] = [
        (0, 0x40, 0),                               // AcDb:Header — empty
        (1, 0x40, 0),                               // AcDb:Classes — empty
        (2, map_at as u32, map_bytes.len() as u32), // AcDb:Handles
    ];
    for (i, (number, seeker, size)) in locators.iter().enumerate() {
        let at = 0x19 + i * 9;
        file[at] = *number;
        file[at + 1..at + 5].copy_from_slice(&seeker.to_le_bytes());
        file[at + 5..at + 9].copy_from_slice(&size.to_le_bytes());
    }
    file[OBJECT_AT..OBJECT_AT + record.len()].copy_from_slice(record);
    file[map_at..].copy_from_slice(&map_bytes);
    file
}

// ================================================================
// R13-R15 flat locator layout
// ================================================================

#[test]
fn r14_flat_file_walks_its_object_map_and_decodes_a_line() {
    let record = build_line_record(Version::R14, 0x83, (50.0, 50.0, 0.0), (100.0, 100.0, 0.0));
    let bytes = build_flat_file(Version::R14, &record, 0x83);
    let file = DwgFile::from_bytes(bytes).expect("R14 file opens");
    assert_eq!(file.version(), Version::R14);

    // The locator list names its records the way the R2004+ section map
    // does, so the same accessors work across every release.
    assert!(file.section_by_name("AcDb:Handles").is_some());
    let map = file.handle_map().expect("handles present").expect("parses");
    assert_eq!(map.len(), 1);
    assert_eq!(map.entries[0].handle, 0x83);
    assert_eq!(map.entries[0].offset, 0x100);

    let raws = file.all_objects().expect("walkable").expect("walks");
    assert_eq!(raws.len(), 1);
    assert_eq!(raws[0].kind, ObjectType::Line);
    // R13/R14 keep the object size out of the prologue, so the walker
    // records none — the entity path recovers it from the preamble.
    assert_eq!(raws[0].obj_size_bits, None);

    let (decoded, summary) = file
        .decoded_entities()
        .expect("dispatchable")
        .expect("dispatches");
    assert_eq!(summary.errored, 0, "R14 LINE must not error");
    match &decoded[0] {
        DecodedEntity::Line(l) => {
            assert_eq!((l.start.x, l.start.y, l.start.z), (50.0, 50.0, 0.0));
            assert_eq!((l.end.x, l.end.y, l.end.z), (100.0, 100.0, 0.0));
        }
        other => panic!("expected a LINE, got {other:?}"),
    }
}

#[test]
fn r2000_flat_file_walks_its_object_map_and_decodes_a_line() {
    let record = build_line_record(Version::R2000, 0x83, (1.0, 2.0, 0.0), (4.0, 8.0, 0.0));
    let bytes = build_flat_file(Version::R2000, &record, 0x83);
    let file = DwgFile::from_bytes(bytes).expect("R2000 file opens");
    assert_eq!(file.version(), Version::R2000);

    let raws = file.all_objects().expect("walkable").expect("walks");
    assert_eq!(raws.len(), 1);
    // R2000 states the boundary in the object prologue, so the walker
    // has it before any decoder runs.
    assert!(raws[0].obj_size_bits.is_some());

    let (decoded, summary) = file
        .decoded_entities()
        .expect("dispatchable")
        .expect("dispatches");
    assert_eq!(summary.errored, 0, "R2000 LINE must not error");
    match &decoded[0] {
        DecodedEntity::Line(l) => {
            assert_eq!((l.start.x, l.start.y), (1.0, 2.0));
            assert_eq!((l.end.x, l.end.y), (4.0, 8.0));
        }
        other => panic!("expected a LINE, got {other:?}"),
    }
}

#[test]
fn r13_r15_object_stream_is_the_whole_file() {
    // §3.1 gives these releases no object *section*: the records sit
    // loose between the class definitions and the object map, and the
    // map addresses them by absolute file offset. `object_stream` has to
    // hand back the file itself for those offsets to mean anything.
    let record = build_line_record(Version::R14, 0x83, (0.0, 0.0, 0.0), (1.0, 1.0, 0.0));
    let bytes = build_flat_file(Version::R14, &record, 0x83);
    let len = bytes.len();
    let file = DwgFile::from_bytes(bytes).expect("opens");
    let stream = file.object_stream().expect("present").expect("readable");
    assert_eq!(stream.len(), len);
    assert_eq!(&stream[..6], b"AC1014");
}

#[test]
fn r13_r15_locators_carry_the_canonical_section_names() {
    let record = build_line_record(Version::R14, 1, (0.0, 0.0, 0.0), (1.0, 1.0, 0.0));
    let bytes = build_flat_file(Version::R14, &record, 1);
    let file = DwgFile::from_bytes(bytes).expect("opens");
    let names: Vec<&str> = file.sections().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["AcDb:Header", "AcDb:Classes", "AcDb:Handles"]);
    // An absent name is `None`, not an error.
    assert!(file.read_section("AcDb:Nonexistent").is_none());
}

// ================================================================
// R2007 (§5.1-§5.4)
// ================================================================

#[test]
fn r2007_locates_its_string_stream_from_the_object_prologue() {
    // §19.1 puts the string stream between an object's data fields and
    // its handle stream, and the trailer that sizes it sits at the end
    // of the data area. R2010+ find that end from the leading `MC`;
    // R2007 has none, and §19.1's `RL` object-data-size names the same
    // bit. Build a record that says so.
    let name = "Continuous";
    let mut strings = BitWriter::new();
    strings.write_bs_u(name.encode_utf16().count() as u16);
    for unit in name.encode_utf16() {
        strings.write_rc((unit & 0xFF) as u8);
        strings.write_rc((unit >> 8) as u8);
    }
    let string_bits = strings.position_bits();

    // Pass 1: measure the prologue so the `RL` can be computed.
    let mut probe = BitWriter::new();
    probe.write_bs(0x39); // LTYPE
    probe.write_rl(0);
    probe.write_handle(0, 0x5D);
    let prologue_bits = probe.position_bits();
    let body_bits = 7usize; // stand-in data fields
    let data_end = prologue_bits + body_bits + string_bits + 17;

    let mut w = BitWriter::new();
    w.write_bs(0x39);
    w.write_rl(data_end as u32);
    w.write_handle(0, 0x5D);
    for _ in 0..body_bits {
        w.write_b(false);
    }
    append_bits(&mut w, &strings);
    w.write_rs(string_bits as i16);
    w.write_b(true); // strings present
    assert_eq!(w.position_bits(), data_end);
    // Handle-stream filler, byte-aligned.
    while w.position_bits() % 8 != 0 {
        w.write_b(false);
    }
    w.write_rc(0);
    let payload = w.into_bytes();

    let section_end =
        dwg::string_stream::data_section_end(&payload, Version::R2007).expect("R2007 boundary");
    assert_eq!(section_end, data_end);
    let stream = dwg::string_stream::locate(&payload, Version::R2007).expect("string stream");
    assert_eq!(stream.start_bit, prologue_bits + body_bits);
    assert_eq!(
        dwg::string_stream::data_field_end(&payload, Version::R2007),
        Some(stream.start_bit)
    );
    let mut reader = dwg::string_stream::StringReader::new(&payload, stream).expect("reader");
    assert_eq!(reader.read_tv().expect("tv"), name);
}

#[test]
fn r2007_system_page_size_matches_the_spec_pseudo_code() {
    // §5.3.1 `GetSystemPageSize`: the Reed-Solomon encoding of the
    // 8-aligned data must fit the page twice, with a 0x400 floor.
    assert_eq!(dwg::r2007::system_page_size(0), 0x400);
    assert_eq!(dwg::r2007::system_page_size(304), 0x400);
    assert_eq!(dwg::r2007::system_page_size(2132), 0x1200);
}

#[test]
fn r2007_page_map_pairs_accumulate_into_offsets() {
    // §5.2 prints the loop verbatim: read `(Int64 size, Int64 id)`
    // pairs, give each page the running offset, then advance it.
    let mut data = Vec::new();
    for (size, id) in [(0x400i64, 22i64), (0x400, 23), (0xA0, 3)] {
        data.extend_from_slice(&size.to_le_bytes());
        data.extend_from_slice(&id.to_le_bytes());
    }
    let map = dwg::r2007::PageMap::parse(&data).expect("page map");
    assert_eq!(map.len(), 3);
    assert_eq!(map.file_offset_of(22), Some(dwg::r2007::PAGE_BASE));
    assert_eq!(map.file_offset_of(23), Some(dwg::r2007::PAGE_BASE + 0x400));
    assert_eq!(map.file_offset_of(3), Some(dwg::r2007::PAGE_BASE + 0x800));
}

// ================================================================
// Corpus-gated: the same behaviour against real AutoCAD output
// ================================================================

fn sample(name: &str) -> Option<PathBuf> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p.push(name);
    if p.exists() { Some(p) } else { None }
}

/// The one LINE of `line_R14.dwg` / `line_2000.dwg` / `line_2007.dwg`
/// is the same authored geometry the R2013 regression test already
/// pins: `(50, 50, 0) -> (100, 100, 0)`.
#[test]
fn corpus_line_geometry_agrees_across_the_three_new_bands() {
    for name in ["line_R14.dwg", "line_2000.dwg", "line_2007.dwg"] {
        let Some(path) = sample(name) else {
            eprintln!("skipping {name}: sample not present");
            continue;
        };
        let file = DwgFile::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (decoded, summary) = file
            .decoded_entities()
            .unwrap_or_else(|| panic!("{name}: no object walk"))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(summary.errored, 0, "{name} must decode without errors");
        let line = decoded
            .iter()
            .find_map(|d| match d {
                DecodedEntity::Line(l) => Some(l),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name}: no LINE decoded"));
        assert_eq!((line.start.x, line.start.y), (50.0, 50.0), "{name} start");
        assert_eq!((line.end.x, line.end.y), (100.0, 100.0), "{name} end");
    }
}

/// Every record of every corpus file in the three new bands resolves to
/// a handle whose own field agrees with the `AcDb:Handles` map, and
/// every handle-map entry yields a record.
#[test]
fn corpus_new_bands_walk_with_no_skips_and_no_handle_mismatches() {
    for name in [
        "line_R14.dwg",
        "arc_R14.dwg",
        "circle_R14.dwg",
        "line_2000.dwg",
        "arc_2000.dwg",
        "circle_2000.dwg",
        "line_2007.dwg",
        "arc_2007.dwg",
        "circle_2007.dwg",
    ] {
        let Some(path) = sample(name) else {
            eprintln!("skipping {name}: sample not present");
            continue;
        };
        let file = DwgFile::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (raws, walk) = file
            .all_objects_lossy()
            .unwrap_or_else(|| panic!("{name}: no object walk"))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!raws.is_empty(), "{name}: walked zero records");
        assert!(
            walk.skipped.is_empty(),
            "{name}: {} handle-map entries yielded no record",
            walk.skipped.len()
        );
        assert!(
            walk.handle_mismatches.is_empty(),
            "{name}: {} records disagree with the handle map",
            walk.handle_mismatches.len()
        );
    }
}

/// R2007's section map must name the sections the §5.2 table lists, and
/// the container must report a `Full` section map rather than the
/// pre-#110 deferred placeholder.
#[test]
fn corpus_r2007_container_names_the_spec_sections() {
    let Some(path) = sample("line_2007.dwg") else {
        eprintln!("skipping: line_2007.dwg not present");
        return;
    };
    let file = DwgFile::open(&path).expect("opens");
    assert_eq!(
        file.section_map_status(),
        &dwg::SectionMapStatus::Full,
        "R2007 section map should be authoritative"
    );
    let container = file.r2007_container().expect("container parsed");
    // §5.2 tabulates a hash code per section name; a decoded map that
    // matches them is a map that came out of the Reed-Solomon and LZ
    // layers intact.
    for (name, hash) in [
        ("AcDb:AcDbObjects", 0x674c_05a9u64),
        ("AcDb:Handles", 0x3f6e_0450),
        ("AcDb:Classes", 0x3f54_045f),
        ("AcDb:Header", 0x32b8_03d9),
        ("AcDb:SummaryInfo", 0x717a_060f),
    ] {
        let desc = container
            .section_map
            .by_name(name)
            .unwrap_or_else(|| panic!("{name} missing from the section map"));
        assert_eq!(desc.hash_code & 0xFFFF_FFFF, hash, "{name} hash code");
    }
    // The file header records the file's own size — the single cheapest
    // check that the §5.10 decompression produced the right bytes.
    assert_eq!(container.header.file_size, file.file_size());
    assert_eq!(container.header.header_size, 0x70);

    let objects = file
        .read_section("AcDb:AcDbObjects")
        .expect("present")
        .expect("readable");
    assert_eq!(&objects[..4], &[0xCA, 0x0D, 0x00, 0x00]);
    let classes = file
        .read_section("AcDb:Classes")
        .expect("present")
        .expect("readable");
    assert_eq!(&classes[..16], &dwg::ClassMap::SENTINEL_START);
}

/// The pre-2004 `AcDb:Classes` layout: no `max_class_number` preamble,
/// records straight from byte 20, numbered consecutively from 500.
#[test]
fn corpus_pre_2004_class_table_numbers_consecutively_from_500() {
    for (name, expect_last) in [("line_R14.dwg", 513u32), ("line_2000.dwg", 511)] {
        let Some(path) = sample(name) else {
            eprintln!("skipping {name}: sample not present");
            continue;
        };
        let file = DwgFile::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let classes = file
            .class_map()
            .unwrap_or_else(|| panic!("{name}: no class section"))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!classes.classes.is_empty(), "{name}: empty class table");
        for (i, c) in classes.classes.iter().enumerate() {
            assert_eq!(c.class_number as usize, 500 + i, "{name} class {i}");
        }
        assert_eq!(
            classes.max_class_number, expect_last,
            "{name} highest class"
        );
        assert_eq!(classes.classes[0].dxf_class_name, "ACDBDICTIONARYWDFLT");
    }
}
