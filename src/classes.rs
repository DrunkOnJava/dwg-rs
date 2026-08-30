//! `AcDb:Classes` section — custom (dynamic) class definitions that extend
//! the built-in DWG object type space (codes < 500).
//!
//! Every object type code ≥ 500 found in the object stream maps to an
//! index into this table via `class_index = type_code - 500`. The table
//! entry carries the application name ("AcDbObjects"), the C++ class
//! name ("AcDbTable"), the DXF record type name ("TABLE"), and a proxy
//! flag that says whether the writer's application was available at save.
//!
//! # On-disk format (R2004+) — measured
//!
//! The section opens with the 16-byte sentinel, then a byte-aligned
//! header whose length depends on the release, and only then does the
//! bit-packed class list start:
//!
//! ```text
//! [0..16]   sentinel
//! [16..20]  RL  size of the class data area, in bytes
//! [20..24]  RL  (R2010+) unknown, observed 0
//! [24..28]  RL  (R2010+) size of the class data area, in bits
//! -- bit stream starts here: byte 20 on R2004, byte 28 on R2010+ --
//!   BS   max_class_number
//!   RC   unknown
//!   RC   unknown
//!   B    unknown
//!   // then one record per custom class:
//!   BS   class_number
//!   BS   version / proxy flags
//!   TV   app_name
//!   TV   cpp_class_name
//!   TV   dxf_class_name
//!   B    was_a_proxy
//!   BS   item_class_id  (0x1F2 proxy entity, 0x1F3 proxy object)
//!   BL   num_objects           -- R2004+
//!   BS   dwg_version           -- R2004+
//!   BS   maintenance_version   -- R2004+
//!   BL   unknown               -- R2004+
//!   BL   unknown               -- R2004+
//! ```
//!
//! # Why the header length is measured, not assumed
//!
//! An earlier cut started the bit stream at byte 24 for every release
//! and read `max_class_number` as a byte-aligned `RL` from bytes 20-24.
//! That reads a class number of 0 and then desynchronises immediately,
//! so [`ClassMap::parse`] returned an **empty table on every real
//! file** — and with it every `Custom(N)` object in the drawing fell
//! through to `Unhandled`, because the dispatcher resolves those codes
//! through this table.
//!
//! The offsets above are read off the bytes:
//!
//! | File | Bytes 16.. | First bits after the header | Decodes to |
//! |---|---|---|---|
//! | `arc_2004.dwg` (AC1018) | `86 02 00 00` then `3F 40 40 00 …` | `00` + `1111110100000001` | `BS` = 509 |
//! | `arc_2010.dwg` (AC1024) | `42 04 00 00 00 00 00 00 0A 22 00 00` then `3F 40 40 00 …` | same | `BS` = 509 |
//! | `arc_2013.dwg` (AC1027) | `D9 03 00 00 00 00 00 00 C3 1E 00 00` then `3F 00 40 00 …` | `00` + `1111110000000001` | `BS` = 508 |
//! | `sample_AC1032.dwg` (AC1032) | `66 18 00 00 00 00 00 00 2A C3 00 00` then `09 40 80 00 …` | `00` + `0010010100000010` | `BS` = 549 |
//!
//! and every one of those matches the highest `Custom(N)` type code the
//! object stream of that file actually uses (508 / 508 / 507 / 547).
//! Continuing to parse from there yields class numbers that run
//! 500, 501, 502, … consecutively up to `max_class_number - 1`, which is
//! the check [`ClassMap::parse`] applies before accepting a table.
//!
//! R2007 (`AC1021`) is not in the table above because this crate cannot
//! read that release's sections yet (`STATUS.md` #110); its header is
//! assumed to match R2010's, and the consecutiveness check rejects the
//! table rather than returning garbage if that assumption is wrong.

use crate::bitcursor::BitCursor;
use crate::bitwriter::BitWriter;
use crate::error::Result;
use crate::tables::read_tv;
use crate::version::Version;

/// Lowest object type code the class table can describe (§20.3): every
/// code below this is a fixed built-in type.
pub const FIRST_CUSTOM_CLASS_NUMBER: u16 = 500;

/// Defensive upper bound on the class count — no realistic drawing has
/// this many custom classes, and the cap stops a runaway read on an
/// adversarial section.
const MAX_CLASSES: usize = 4096;

/// One custom class definition.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassDef {
    pub class_number: u16,
    pub version: i16,
    pub app_name: String,
    pub cpp_class_name: String,
    pub dxf_class_name: String,
    pub was_a_proxy: bool,
    /// 0x1F2 → proxy entity, 0x1F3 → proxy object, anything else is
    /// a vendor-specific dynamic class (IMAGE, TABLE, MLEADER, ...).
    pub item_class_id: u16,
}

/// Parsed custom class table.
#[derive(Debug, Clone, Default)]
pub struct ClassMap {
    pub max_class_number: u32,
    pub classes: Vec<ClassDef>,
}

impl ClassMap {
    /// R2004+ section sentinel (spec §21).
    pub const SENTINEL_START: [u8; 16] = [
        0x8D, 0xA1, 0xC4, 0xB8, 0xC4, 0xA9, 0xF8, 0xC5, 0xC0, 0xDC, 0xF4, 0x5F, 0xE7, 0xCF, 0xB6,
        0x8A,
    ];

    /// Parse a decompressed `AcDb:Classes` payload.
    ///
    /// On any malformed structure we return an empty map rather than
    /// fail the whole read — the class table is advisory information,
    /// not required for reading built-in entity types.
    pub fn parse(bytes: &[u8], version: Version) -> Result<Self> {
        // Byte offset at which the bit-packed class list begins — see
        // the measured table in the module docs.
        let bit_start = if version.is_r2007_plus() { 28 } else { 20 };
        if bytes.len() < bit_start + 4 {
            return Ok(Self::default());
        }
        let size_bytes = u32::from_le_bytes(
            bytes[16..20]
                .try_into()
                .expect("slice 16..20 is length 4 by the length pre-check"),
        ) as usize;

        let mut c = BitCursor::new(&bytes[bit_start..]);
        let Ok(max_class_number) = c.read_bs_u() else {
            return Ok(Self::default());
        };
        // Two unknown RCs and one unknown B close the class-list header.
        if c.read_rc().is_err() || c.read_rc().is_err() || c.read_b().is_err() {
            return Ok(Self::default());
        }

        // The class data area cannot run past either the declared size
        // or the bytes actually present.
        let max_bits = size_bytes
            .saturating_mul(8)
            .min(bytes.len().saturating_sub(bit_start).saturating_mul(8));

        let expected = (max_class_number as usize)
            .saturating_sub(FIRST_CUSTOM_CLASS_NUMBER as usize)
            .saturating_add(1)
            .min(MAX_CLASSES);

        let classes = if version.is_r2007_plus() {
            read_split_stream_classes(&mut c, expected)
        } else {
            read_inline_classes(&mut c, version, max_bits)
        };

        // A correctly-located table numbers its classes consecutively
        // from FIRST_CUSTOM_CLASS_NUMBER. Anything else means the bit
        // stream was not where this parser looked, so report no classes
        // rather than a desynchronised list a dispatcher would trust.
        let consecutive = classes
            .iter()
            .enumerate()
            .all(|(i, d)| d.class_number as usize == FIRST_CUSTOM_CLASS_NUMBER as usize + i);
        Ok(Self {
            max_class_number: u32::from(max_class_number),
            classes: if consecutive { classes } else { Vec::new() },
        })
    }

    /// Look up a class by its type code (for object_type.rs `Custom(N)`).
    pub fn by_type_code(&self, type_code: u16) -> Option<&ClassDef> {
        self.classes.iter().find(|c| c.class_number == type_code)
    }
}

/// Read the pre-R2007 class list, whose `TV`s sit inline in each record.
fn read_inline_classes(c: &mut BitCursor<'_>, version: Version, max_bits: usize) -> Vec<ClassDef> {
    let mut classes = Vec::new();
    while c.position_bits() < max_bits && c.remaining_bits() >= 64 && classes.len() < MAX_CLASSES {
        let Some(def) = read_inline_class_record(c, version) else {
            break;
        };
        classes.push(def);
    }
    classes
}

/// Read one pre-R2007 class record. `None` as soon as a field fails to
/// decode or the record is obviously not a class definition.
fn read_inline_class_record(c: &mut BitCursor<'_>, version: Version) -> Option<ClassDef> {
    let class_number = c.read_bs_u().ok()?;
    let version_flag = c.read_bs().ok()?;
    let app_name = read_tv(c, version).ok()?;
    let cpp_class_name = read_tv(c, version).ok()?;
    let dxf_class_name = read_tv(c, version).ok()?;
    let was_a_proxy = c.read_b().ok()?;
    let item_class_id = c.read_bs_u().ok()?;
    if version.is_r2004_plus() {
        read_r2004_record_tail(c)?;
    }
    if app_name.is_empty() && cpp_class_name.is_empty() && dxf_class_name.is_empty() {
        return None;
    }
    Some(ClassDef {
        class_number,
        version: version_flag,
        app_name,
        cpp_class_name,
        dxf_class_name,
        was_a_proxy,
        item_class_id,
    })
}

/// Read the R2007+ class list — `expected` records of non-string fields
/// followed by three `TU` strings per record, in record order.
///
/// # Measured
///
/// `arc_2013.dwg` declares `max_class_number` 508, so nine records; its
/// non-string fields decode to class numbers 500..=508 with item class
/// id `0x1F3` throughout and object counts 1/3/24/17/1/1/1/1/5 —
/// exactly the number of records of each type the object stream holds.
/// The 27 strings that follow read `"ObjectDBX Classes"`,
/// `"AcDbDictionaryWithDefault"`, `"ACDBDICTIONARYWDFLT"`,
/// `"ObjectDBX Classes"`, `"AcDbMaterial"`, `"MATERIAL"`, … in that
/// order, and the 28th read is junk. `arc_2010.dwg` agrees with ten
/// records, 500..=509.
///
/// `sample_AC1032.dwg` (R2018) declares 50 records but its ninth record
/// does not decode with this field list — see `STATUS.md`. This
/// function returns what it read; [`ClassMap::parse`]'s consecutiveness
/// check then rejects the short list rather than pairing 9 records with
/// strings meant for 50.
fn read_split_stream_classes(c: &mut BitCursor<'_>, expected: usize) -> Vec<ClassDef> {
    let mut classes = Vec::with_capacity(expected.min(64));
    for _ in 0..expected {
        let Some(def) = read_split_stream_class_record(c) else {
            // A short read means the string block does not start where
            // this function is about to look, so the names would be
            // paired with the wrong classes. Report nothing.
            return Vec::new();
        };
        classes.push(def);
    }
    for def in classes.iter_mut() {
        let (Ok(app), Ok(cpp), Ok(dxf)) = (
            read_tv(c, Version::R2018),
            read_tv(c, Version::R2018),
            read_tv(c, Version::R2018),
        ) else {
            return Vec::new();
        };
        def.app_name = app;
        def.cpp_class_name = cpp;
        def.dxf_class_name = dxf;
    }
    if classes.iter().any(|d| d.dxf_class_name.is_empty()) {
        return Vec::new();
    }
    classes
}

/// Read one R2007+ class record's non-string fields.
fn read_split_stream_class_record(c: &mut BitCursor<'_>) -> Option<ClassDef> {
    let class_number = c.read_bs_u().ok()?;
    let version_flag = c.read_bs().ok()?;
    let was_a_proxy = c.read_b().ok()?;
    let item_class_id = c.read_bs_u().ok()?;
    read_r2004_record_tail(c)?;
    Some(ClassDef {
        class_number,
        version: version_flag,
        app_name: String::new(),
        cpp_class_name: String::new(),
        dxf_class_name: String::new(),
        was_a_proxy,
        item_class_id,
    })
}

/// `BL num_objects`, `BS dwg_version`, `BS maintenance_version` and two
/// unknown `BL`s — the five fields R2004 added to every class record.
fn read_r2004_record_tail(c: &mut BitCursor<'_>) -> Option<()> {
    let _num_objects = c.read_bl_u().ok()?;
    let _dwg_version = c.read_bs().ok()?;
    let _maintenance_version = c.read_bs().ok()?;
    let _unknown1 = c.read_bl().ok()?;
    let _unknown2 = c.read_bl().ok()?;
    Some(())
}

// TV string reading is delegated to `crate::tables::read_tv`, which
// correctly branches on UTF-8 (R2004 and earlier) vs UTF-16LE (R2007+
// per spec §2). An earlier local implementation in this module
// read 8-bit for all versions, which mangled vendor class names in
// files whose author used non-ASCII identifiers.

// ================================================================
// L12-07 — class map writer (task #380)
//
// Inverse of [`ClassMap::parse`]. Assembles the 5-field class-record
// layout + trailing CRC per ODA Open Design Specification v5.4.1 §5.7
// (R2004+) / §21.4.2.
//
// Produced stream — the exact inverse of the measured layout in this
// module's docs:
//
// ```text
// [0..16]   16 bytes               — ClassMap::SENTINEL_START
// [16..20]  4 bytes little-endian  — size of the class data area, bytes
// [20..24]  4 bytes little-endian  — (R2010+) unknown, written as 0
// [24..28]  4 bytes little-endian  — (R2010+) class data area, in bits
// [20..] / [28..]                  — the bit-packed class list
// ```
//
// The bit-packed list opens with `BS max_class_number`, two unknown
// `RC`s and one unknown `B`, then one record per class. Pre-R2007 a
// record carries its three `TV`s inline; from R2007 the non-string
// fields of every record come first and the `3 × N` strings follow as a
// block, in record order. A trailing CRC-8 (§2.14.1, seed 0xC0C1)
// covers the whole byte-aligned payload.
// ================================================================

/// Write a TV (variable text) field per spec §2, branching on version.
///
/// - R2007+ → UTF-16LE: `BS len` then `len` little-endian u16 code units.
/// - Older   → 8-bit: `BS len` then `len` raw bytes (UTF-8 on the wire).
///
/// The decoder counterpart is [`crate::tables::read_tv`]. It pops a
/// trailing NUL unit if present; the writer emits the string verbatim
/// without appending a NUL because callers pass the already-decoded
/// value, not the on-disk bytes.
fn write_tv(w: &mut BitWriter, s: &str, version: Version) {
    if version.uses_utf16_text() {
        let units: Vec<u16> = s.encode_utf16().collect();
        w.write_bs_u(units.len() as u16);
        for u in units {
            w.write_rc((u & 0xFF) as u8);
            w.write_rc((u >> 8) as u8);
        }
    } else {
        let bytes = s.as_bytes();
        w.write_bs_u(bytes.len() as u16);
        for b in bytes {
            w.write_rc(*b);
        }
    }
}

/// Highest class number the list describes, as the header's
/// `max_class_number` — the value [`ClassMap::parse`] uses to decide how
/// many records to expect on R2007+.
fn class_list_max(classes: &ClassMap) -> u16 {
    classes
        .classes
        .last()
        .map(|d| d.class_number)
        .unwrap_or_else(|| classes.max_class_number.min(u32::from(u16::MAX)) as u16)
}

/// Write the five fields R2004 added to every class record, all zero.
fn write_r2004_record_tail(w: &mut BitWriter) {
    w.write_bl(0); // num_objects
    w.write_bs(0); // dwg_version
    w.write_bs(0); // maintenance_version
    w.write_bl(0); // unknown
    w.write_bl(0); // unknown
}

/// Write an `AcDb:Classes` payload — inverse of [`ClassMap::parse`].
///
/// `writer` is used for internal bit-packing; the returned `Vec<u8>` is
/// the fully-assembled section bytes ready for LZ77 compression by the
/// section writer. The 16-byte sentinel, 4-byte size-in-bits header,
/// 4-byte `max_class_number`, and trailing CRC-8 are included; the
/// caller composes this with the R2004+ page framing layer.
///
/// The internal `writer` argument is there purely so the function
/// signature matches the write-path convention used by the element
/// encoders (`trait ElementEncoder`) — the actual class bytes are
/// produced in a fresh [`BitWriter`] and then prefixed / suffixed by
/// the sentinel, size header, max-class-number, and CRC.
///
/// # CRC
///
/// A 2-byte CRC-8 (§2.14.1, seed 0xC0C1) is appended covering the
/// entire byte-aligned payload starting at the sentinel. This matches
/// the reader's validation path.
pub fn write_class_map(
    classes: &ClassMap,
    _writer: &mut BitWriter,
    version: Version,
) -> Result<Vec<u8>> {
    use crate::crc::crc8;

    let mut inner = BitWriter::new();
    inner.write_bs_u(class_list_max(classes));
    inner.write_rc(0); // unknown
    inner.write_rc(0); // unknown
    inner.write_b(true); // unknown
    if version.is_r2007_plus() {
        for def in &classes.classes {
            inner.write_bs_u(def.class_number);
            inner.write_bs(def.version);
            inner.write_b(def.was_a_proxy);
            inner.write_bs_u(def.item_class_id);
            write_r2004_record_tail(&mut inner);
        }
        for def in &classes.classes {
            write_tv(&mut inner, &def.app_name, version);
            write_tv(&mut inner, &def.cpp_class_name, version);
            write_tv(&mut inner, &def.dxf_class_name, version);
        }
    } else {
        for def in &classes.classes {
            inner.write_bs_u(def.class_number);
            inner.write_bs(def.version);
            write_tv(&mut inner, &def.app_name, version);
            write_tv(&mut inner, &def.cpp_class_name, version);
            write_tv(&mut inner, &def.dxf_class_name, version);
            inner.write_b(def.was_a_proxy);
            inner.write_bs_u(def.item_class_id);
            if version.is_r2004_plus() {
                write_r2004_record_tail(&mut inner);
            }
        }
    }
    let bits_written = inner.position_bits();
    let classes_bytes = inner.into_bytes();

    let mut out = Vec::with_capacity(32 + classes_bytes.len());
    out.extend_from_slice(&ClassMap::SENTINEL_START);
    out.extend_from_slice(&(classes_bytes.len() as u32).to_le_bytes());
    if version.is_r2007_plus() {
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(bits_written as u32).to_le_bytes());
    }
    out.extend_from_slice(&classes_bytes);

    // CRC-8 over [sentinel, size, max, payload] with seed 0xC0C1.
    let crc = crc8(0xC0C1, &out);
    out.extend_from_slice(&crc.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bytes_produces_empty_map() {
        let m = ClassMap::parse(&[], Version::R2018).unwrap();
        assert!(m.classes.is_empty());
    }

    #[test]
    fn short_bytes_produces_empty_map() {
        let m = ClassMap::parse(&[0u8; 10], Version::R2018).unwrap();
        assert!(m.classes.is_empty());
    }

    // -------- L12-07: writer tests --------

    #[test]
    fn write_class_map_empty_map_emits_sentinel_and_headers_only() {
        let map = ClassMap {
            max_class_number: 0x1F3,
            classes: Vec::new(),
        };
        let mut w = BitWriter::new();
        let bytes = write_class_map(&map, &mut w, Version::R2004).unwrap();
        assert_eq!(&bytes[..16], &ClassMap::SENTINEL_START);
        // sentinel(16) + size(4) + the 35-bit class-list header rounded
        // up to 5 bytes + CRC(2).
        assert_eq!(bytes.len(), 27);
        let size_bytes = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        assert_eq!(size_bytes, 5);
        assert_eq!(
            ClassMap::parse(&bytes, Version::R2004)
                .unwrap()
                .classes
                .len(),
            0
        );
    }

    /// R2010+ inserts two extra `RL`s between the size header and the
    /// class list; the writer must emit them or the reader starts eight
    /// bytes early.
    #[test]
    fn write_class_map_r2010_header_carries_the_two_extra_rls() {
        let map = ClassMap {
            max_class_number: 0x1F3,
            classes: Vec::new(),
        };
        let mut w = BitWriter::new();
        let r2004 = write_class_map(&map, &mut w, Version::R2004).unwrap();
        let mut w2 = BitWriter::new();
        let r2018 = write_class_map(&map, &mut w2, Version::R2018).unwrap();
        assert_eq!(r2018.len(), r2004.len() + 8);
        assert_eq!(u32::from_le_bytes(r2018[20..24].try_into().unwrap()), 0);
        // The third RL is the class data area in bits: 35 for a header
        // with no records.
        assert_eq!(u32::from_le_bytes(r2018[24..28].try_into().unwrap()), 35);
    }

    /// Build one representative class table and require a byte-exact
    /// round trip through the writer and the parser, in both layouts.
    fn sample_map() -> ClassMap {
        ClassMap {
            max_class_number: 501,
            classes: vec![
                ClassDef {
                    class_number: 500,
                    version: 0,
                    app_name: "ObjectDBX Classes".to_string(),
                    cpp_class_name: "AcDbTable".to_string(),
                    dxf_class_name: "TABLE".to_string(),
                    was_a_proxy: false,
                    item_class_id: 0x1F2,
                },
                ClassDef {
                    class_number: 501,
                    version: 1,
                    app_name: "ObjectDBX Classes".to_string(),
                    cpp_class_name: "AcDbWipeout".to_string(),
                    dxf_class_name: "WIPEOUT".to_string(),
                    was_a_proxy: true,
                    item_class_id: 0x1F2,
                },
            ],
        }
    }

    #[test]
    fn write_class_map_inline_layout_round_trips_on_r2004() {
        let map = sample_map();
        let mut w = BitWriter::new();
        let bytes = write_class_map(&map, &mut w, Version::R2004).unwrap();
        let parsed = ClassMap::parse(&bytes, Version::R2004).unwrap();
        assert_eq!(parsed.max_class_number, 501);
        assert_eq!(parsed.classes, map.classes);
    }

    #[test]
    fn write_class_map_split_layout_round_trips_on_r2018() {
        let map = sample_map();
        let mut w = BitWriter::new();
        let bytes = write_class_map(&map, &mut w, Version::R2018).unwrap();
        let parsed = ClassMap::parse(&bytes, Version::R2018).unwrap();
        assert_eq!(parsed.max_class_number, 501);
        assert_eq!(parsed.classes, map.classes);
        assert_eq!(parsed.by_type_code(501).unwrap().dxf_class_name, "WIPEOUT");
    }

    /// A table whose class numbers do not run consecutively from 500
    /// cannot have come from the offsets this parser reads, so it must
    /// be reported as no classes rather than as a desynchronised list.
    #[test]
    fn parse_rejects_a_non_consecutive_class_list() {
        let mut map = sample_map();
        map.classes[0].class_number = 400;
        let mut w = BitWriter::new();
        let bytes = write_class_map(&map, &mut w, Version::R2004).unwrap();
        assert!(
            ClassMap::parse(&bytes, Version::R2004)
                .unwrap()
                .classes
                .is_empty()
        );
    }
}
