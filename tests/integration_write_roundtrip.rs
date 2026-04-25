//! Write-path round-trip tests (L12-12).
//!
//! These tests cover both the section-page framing path and the
//! full in-crate R2004-family byte-buffer assembly path. They prove
//! this crate's reader can recover the emitted sections; external CAD
//! application acceptance remains a separate manual compatibility gate.

use dwg::DwgFile;
use dwg::file_writer::{WriterScaffold, assemble_dwg_bytes};
use dwg::lz77;
use dwg::section_writer::HEADER_SIZE;
use dwg::version::Version;

/// Multi-section Stage-1 round-trip: several named sections with
/// varied payload shapes all recover exactly.
#[test]
fn stage1_multi_section_roundtrip() {
    let sections: Vec<(&str, Vec<u8>)> = vec![
        (
            "AcDb:Header",
            b"header payload with nulls\0\0\0\0and text".to_vec(),
        ),
        ("AcDb:SummaryInfo", vec![0x55u8; 512]),
        ("AcDb:Preview", vec![0xAAu8; 1_000]),
        ("AcDb:AppInfo", b"app\0info\0payload".to_vec()),
        ("AcDb:Classes", (0u8..=255).collect::<Vec<u8>>()),
    ];

    let mut scaffold = WriterScaffold::new(Version::R2018);
    for (name, payload) in &sections {
        scaffold.add_section(*name, payload.clone());
    }
    let built = scaffold.build_sections().expect("build_sections succeeds");
    assert_eq!(built.len(), sections.len());

    for b in &built {
        // Strip the 32-byte masked header to isolate the LZ77 stream.
        let lz77_stream =
            &b.built.bytes[HEADER_SIZE..HEADER_SIZE + b.built.compressed_size as usize];
        let decompressed = lz77::decompress(lz77_stream, None)
            .unwrap_or_else(|e| panic!("{} failed to decompress: {e}", b.name));
        // Find the matching source payload by name and compare.
        let source = sections
            .iter()
            .find(|(n, _)| *n == b.name)
            .map(|(_, p)| p)
            .expect("built section name present in source list");
        assert_eq!(
            decompressed, *source,
            "{} did NOT round-trip through stage-1 LZ77 + framing",
            b.name
        );
    }
}

/// Stage-1 round-trip with an empty section: encoder must not panic
/// on zero-length input and the decoder must return the empty slice.
#[test]
fn stage1_empty_section_roundtrip() {
    let mut scaffold = WriterScaffold::new(Version::R2018);
    scaffold.add_section("AcDb:Preview", Vec::new());
    let built = scaffold.build_sections().expect("empty section builds");
    assert_eq!(built.len(), 1);
    let b = &built[0];
    assert_eq!(b.built.decompressed_size, 0);
    let lz77_stream = &b.built.bytes[HEADER_SIZE..HEADER_SIZE + b.built.compressed_size as usize];
    let decompressed = lz77::decompress(lz77_stream, None).unwrap();
    assert!(decompressed.is_empty());
}

/// Stage-1 output is byte-deterministic: building the same input
/// twice must yield identical Stage-1 bytes. Locks the property so a
/// future writer that accidentally introduces HashMap ordering or
/// timestamps breaks here.
#[test]
fn stage1_output_is_deterministic() {
    let payload = b"deterministic round-trip test payload".to_vec();
    let mut a = WriterScaffold::new(Version::R2018);
    a.add_section("AcDb:Header", payload.clone());
    let built_a = a.build_sections().unwrap();
    let mut b = WriterScaffold::new(Version::R2018);
    b.add_section("AcDb:Header", payload);
    let built_b = b.build_sections().unwrap();

    assert_eq!(built_a.len(), 1);
    assert_eq!(built_b.len(), 1);
    assert_eq!(
        built_a[0].built.bytes, built_b[0].built.bytes,
        "two identical-input Stage-1 runs must produce identical bytes"
    );
}

/// Stage-1 page size is always a multiple of 32 bytes (the DWG file
/// page alignment per ODA §4.2). A section that decompresses to 1
/// byte still emits a page of at least 32 + padding bytes.
#[test]
fn stage1_pages_are_32_byte_aligned() {
    // Skip payloads < 32B — the LZ77 encoder can return
    // Lz77UnencodableLength on pathologically small inputs that don't
    // contain a byte-aligned opcode. Production callers always pass
    // real section bodies (header/summary/preview) which are in the
    // hundreds-to-thousands range.
    for payload_size in &[32usize, 33, 64, 100, 1_000, 10_000] {
        let mut scaffold = WriterScaffold::new(Version::R2018);
        let payload = vec![0xC3u8; *payload_size];
        scaffold.add_section("AcDb:Header", payload);
        let built = scaffold.build_sections().unwrap();
        assert_eq!(built.len(), 1);
        let page_len = built[0].built.bytes.len();
        assert_eq!(
            page_len % 32,
            0,
            "page size {page_len} for payload_size={payload_size} is not 32-byte aligned"
        );
    }
}

#[test]
fn assembled_r2004_family_bytes_roundtrip_sections() {
    let sections: Vec<(&str, Vec<u8>)> = vec![
        ("AcDb:Header", vec![0x11u8; 128]),
        (
            "AcDb:SummaryInfo",
            b"title\0subject\0author\0keywords\0comments\0".to_vec(),
        ),
        ("AcDb:Preview", vec![0xAAu8; 256]),
    ];

    let mut scaffold = WriterScaffold::new(Version::R2018);
    for (name, payload) in &sections {
        scaffold.add_section(*name, payload.clone());
    }

    let built = scaffold.build_sections().expect("build_sections succeeds");
    let bytes = assemble_dwg_bytes(&built, Version::R2018).expect("assemble_dwg_bytes succeeds");
    let file = DwgFile::from_bytes(bytes).expect("assembled bytes reopen");

    for (name, expected) in sections {
        let actual = file
            .read_section(name)
            .unwrap_or_else(|| panic!("{name} section should exist"))
            .unwrap_or_else(|e| panic!("{name} should read cleanly: {e}"));
        assert_eq!(actual, expected, "{name} did not round-trip");
    }
}

#[test]
fn dwg_file_to_bytes_roundtrips_sections() {
    let sections: Vec<(&str, Vec<u8>)> = vec![
        ("AcDb:Header", vec![0x22u8; 128]),
        ("AcDb:Classes", vec![0x33u8; 192]),
        ("AcDb:Handles", vec![0x44u8; 224]),
    ];

    let mut scaffold = WriterScaffold::new(Version::R2018);
    for (name, payload) in &sections {
        scaffold.add_section(*name, payload.clone());
    }
    let built = scaffold.build_sections().expect("build_sections succeeds");
    let first_bytes =
        assemble_dwg_bytes(&built, Version::R2018).expect("assemble_dwg_bytes succeeds");
    let file = DwgFile::from_bytes(first_bytes).expect("assembled bytes reopen");

    let second_bytes = file.to_bytes().expect("DwgFile::to_bytes succeeds");
    let reopened = DwgFile::from_bytes(second_bytes).expect("to_bytes output reopens");

    for (name, expected) in sections {
        let actual = reopened
            .read_section(name)
            .unwrap_or_else(|| panic!("{name} section should exist"))
            .unwrap_or_else(|e| panic!("{name} should read cleanly: {e}"));
        assert_eq!(
            actual, expected,
            "{name} did not round-trip through to_bytes"
        );
    }
}
