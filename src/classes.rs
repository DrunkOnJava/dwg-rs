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
//!   BL   max_class_number
//!   B    unknown
//!   // then one record per custom class:
//!   BS   class_number
//!   BS   version / proxy flags
//!   TV   app_name          -- inline pre-R2007, string block from R2007
//!   TV   cpp_class_name    -- inline pre-R2007, string block from R2007
//!   TV   dxf_class_name    -- inline pre-R2007, string block from R2007
//!   B    was_a_proxy
//!   BS   item_class_id  (0x1F2 proxy entity, 0x1F3 proxy object)
//!   BL   num_objects           -- R2004+
//!   BL   dwg_version           -- R2004+
//!   BL   maintenance_version   -- R2004+
//!   BL   unknown               -- R2004+
//!   BL   unknown               -- R2004+
//! ```
//!
//! # The spec states this record two different ways; the bytes pick one
//!
//! The ODA Open Design Specification v5.4.1 describes the class table
//! twice, and the two descriptions disagree on three fields:
//!
//! | Field | §10.2 (R18+) | §5.8 (R2007) |
//! |---|---|---|
//! | `max_class_number` | `BS` + `RC 0x00` + `RC 0x00` | `BL` |
//! | `dwg_version` | `BS` | `BL` |
//! | `maintenance_version` | `BS` | `BL` |
//!
//! §5.8 is the one the bytes agree with, and the two readings are
//! *bit-identical* for most values, which is why the disagreement went
//! unnoticed until `sample_AC1032.dwg`:
//!
//! - `BS` and `BL` share the 2-bit tag alphabet (`01` → one byte,
//!   `10` → zero). They diverge only on tag `00`, where `BS` consumes a
//!   16-bit `RS` and `BL` a 32-bit `RL`.
//! - `BS max_class_number` + two `RC`s consumes 2 + 16 + 16 = 34 bits;
//!   `BL max_class_number` with tag `00` consumes 2 + 32 = 34 bits over
//!   the same four little-endian bytes. Identical whenever the value
//!   exceeds 255, which every real `max_class_number` (≥ 500) does.
//!
//! So the §10.2 reading survives on any file whose class records all
//! carry `dwg_version ≤ 255` and `maintenance_version ≤ 255`. On
//! `sample_AC1032.dwg` four classes — MLEADERSTYLE (508),
//! ACDBDETAILVIEWSTYLE (516), WIPEOUT (520) and MULTILEADER (526) —
//! record `dwg_version = 33`, `maintenance_version = 329`. Reading
//! that `BL` as a `BS` consumes 18 bits instead of 34 and loses 16
//! bits of alignment per occurrence, which is the drift reported in
//! issue #37.
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
//! | File | Bytes 16.. | First bits after the header | `BL` decodes to |
//! |---|---|---|---|
//! | `arc_2004.dwg` (AC1018) | `86 02 00 00` then `3F 40 40 00 …` | `00` + `FD 01 00 00` | 509 |
//! | `arc_2010.dwg` (AC1024) | `42 04 00 00 00 00 00 00 0A 22 00 00` then `3F 40 40 00 …` | same | 509 |
//! | `arc_2013.dwg` (AC1027) | `D9 03 00 00 00 00 00 00 C3 1E 00 00` then `3F 00 40 00 …` | `00` + `FC 01 00 00` | 508 |
//! | `sample_AC1032.dwg` (AC1032) | `66 18 00 00 00 00 00 00 2A C3 00 00` then `09 40 80 00 …` | `00` + `25 02 00 00` | 549 |
//!
//! and every one of those matches the highest `Custom(N)` type code the
//! object stream of that file actually uses (508 / 508 / 507 / 547).
//! Continuing to parse from there yields class numbers that run
//! 500, 501, 502, … consecutively up to `max_class_number - 1`, which is
//! the check [`ClassMap::parse`] applies before accepting a table.
//!
//! # The record count and the string block corroborate each other
//!
//! On R2007+ the three names of every record live in one string block
//! that follows the last record, so a record list that stops one bit
//! early pairs every name with the wrong class. Three independent
//! measurements agree that reading exactly
//! `max_class_number - 500 + 1` records with the field list above
//! lands on the first bit of that block:
//!
//! | File | records | last record ends | string block ends | `AcDb:Classes` string-stream end bit |
//! |---|---|---|---|---|
//! | `arc_2010.dwg` | 10 | 877 | 8665 | 8682 = `size_in_bits` (8714) − 32 |
//! | `arc_2013.dwg` | 9 | 788 | 7826 | 7843 = `size_in_bits` (7875) − 32 |
//! | `sample_AC1032.dwg` | 50 | 4093 | 49897 | 49930 = `size_in_bits` (49962) − 32 |
//!
//! (bit offsets relative to the start of the bit stream.) The
//! right-hand column is the §19.4.1 string-stream trailer: its
//! terminating bit is set in all three files, and its length word
//! reads 7788 / 7038 / 45804 bits — exactly the span between the end
//! of the last record and the end of the last string in each file.
//! `sample_AC1032.dwg` exercises the two-word form
//! (`0xB2EC` has bit 15 set, second word 1 →
//! `(0xB2EC & 0x7FFF) | (1 << 15) = 45804`).
//!
//! The parser does not need the trailer — the record count comes from
//! `max_class_number` and the strings follow the records directly —
//! but it is the strongest available check that the field list above
//! is the real one, so `examples/probe_class_layout.rs` prints it.
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
    /// Number of objects of this class in the drawing (DXF group 91).
    /// Zero on pre-R2004 files, which do not record it.
    pub num_objects: u32,
    /// Release code of the AutoCAD build that last wrote this class
    /// definition (20 = R2000 … 33 = R2018). Zero pre-R2004.
    pub dwg_version: u32,
    /// Maintenance-release counter of that build. Zero pre-R2004, and
    /// the field that overflows a `BS` on R2018 — see the module docs.
    pub maintenance_version: u32,
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
        let bit_start = Self::bit_stream_start(version);
        if bytes.len() < bit_start + 4 {
            return Ok(Self::default());
        }
        let size_bytes = u32::from_le_bytes(
            bytes[16..20]
                .try_into()
                .expect("slice 16..20 is length 4 by the length pre-check"),
        ) as usize;

        let mut c = BitCursor::new(&bytes[bit_start..]);
        // R2004+: `BL max_class_number` then one unknown `B` (§5.8; §10.2
        // spells the same 34 bits as `BS` + `RC 0x00` + `RC 0x00` — see the
        // module docs for why only `BL` generalises). R13-R15 have neither:
        // their first record starts on the first bit of the data area.
        let declared_max = if version.is_r2004_plus() {
            let Ok(v) = c.read_bl_u() else {
                return Ok(Self::default());
            };
            if c.read_b().is_err() {
                return Ok(Self::default());
            }
            Some(v)
        } else {
            None
        };

        // The class data area cannot run past either the declared size
        // or the bytes actually present.
        let max_bits = size_bytes
            .saturating_mul(8)
            .min(bytes.len().saturating_sub(bit_start).saturating_mul(8));

        let classes = if version.is_r2007_plus() {
            let expected = (declared_max.unwrap_or(0) as usize)
                .saturating_sub(FIRST_CUSTOM_CLASS_NUMBER as usize)
                .saturating_add(1)
                .min(MAX_CLASSES);
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
        // R13-R15 record no `max_class_number`; the highest class number
        // the table actually carries is the same quantity.
        let max_class_number = declared_max.unwrap_or(if classes.is_empty() {
            0
        } else {
            FIRST_CUSTOM_CLASS_NUMBER as u32 + classes.len() as u32 - 1
        });
        Ok(Self {
            max_class_number,
            classes: if consecutive { classes } else { Vec::new() },
        })
    }

    /// Byte offset of the first bit of the class list, per release.
    ///
    /// Everything before it is byte-aligned bookkeeping: the 16-byte
    /// sentinel, then one to three `RL`s. The counts are measured, not
    /// assumed — see the module docs for the R2004 / R2010+ evidence and
    /// the two rows below for the other two families.
    ///
    /// | release | bytes 16.. | first bits of the stream | `max_class_number` |
    /// |---|---|---|---|
    /// | `line_R14.dwg` (AC1014) | `21 03 00 00` (size 801) | `3D 00 64 49 …` | *(none recorded)* |
    /// | `line_2000.dwg` (AC1015) | `C0 02 00 00` (size 704) | `3D 00 64 49 …` | *(none recorded)* |
    /// | `line_2007.dwg` (AC1021) | `42 04 00 00 0A 22 00 00` (size 1090, 8714 bits) | `3F 40 40 00 …` | 509 |
    ///
    /// R2007 writes the size-in-bits `RL` immediately after the
    /// size-in-bytes `RL`, where R2010+ inserts a zero `RL` between them —
    /// so its stream starts at byte 24, not 28. Reading it at 28 consumes
    /// the first 32 bits of the class list as a header and the table
    /// desynchronises on the first record.
    ///
    /// R13-R15 have only the one size `RL`, and their first record begins
    /// at byte 20 with no `max_class_number` preamble. On `line_R14.dwg`
    /// the fourteen records that follow run 500..=513 and end on bit 6406
    /// of the declared 6408, and on `line_2000.dwg` the twelve records run
    /// 500..=511 and end on bit 5628 of 5632 — the residue in both cases
    /// is the byte-alignment padding before the CRC.
    fn bit_stream_start(version: Version) -> usize {
        match version {
            Version::R14 | Version::R2000 | Version::R2004 => 20,
            Version::R2007 => 24,
            Version::R2010 | Version::R2013 | Version::R2018 => 28,
        }
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
        // A record that ends past the declared data area was never in the
        // table — the residue after the last real record is the padding
        // that byte-aligns the CRC. Keeping such a record would give the
        // consecutiveness check a class number to reject the whole table
        // over. This matters on R13-R15, whose class list has no record
        // count to stop at (see `ClassMap::bit_stream_start`).
        if c.position_bits() > max_bits {
            break;
        }
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
    let tail = if version.is_r2004_plus() {
        read_r2004_record_tail(c)?
    } else {
        RecordTail::default()
    };
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
        num_objects: tail.num_objects,
        dwg_version: tail.dwg_version,
        maintenance_version: tail.maintenance_version,
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
/// `sample_AC1032.dwg` (R2018) declares 50 records; with
/// `maintenance_version` read as the `BL` §5.8 specifies, all 50
/// decode as 500..=549 and the 150 strings that follow read
/// `ACDBDICTIONARYWDFLT`, `ACDBPLACEHOLDER`, `LAYOUT`,
/// `DICTIONARYVAR`, `TABLESTYLE`, `MATERIAL`, `VISUALSTYLE`, `SCALE`,
/// `MLEADERSTYLE`, … `ACDBPERSSUBENTMANAGER` — the last string ending
/// exactly on the section's string-stream boundary.
///
/// A short read means the string block does not start where this
/// function is about to look, so the names would be paired with the
/// wrong classes; the function then reports nothing rather than a
/// desynchronised list.
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
    let tail = read_r2004_record_tail(c)?;
    Some(ClassDef {
        class_number,
        version: version_flag,
        app_name: String::new(),
        cpp_class_name: String::new(),
        dxf_class_name: String::new(),
        was_a_proxy,
        item_class_id,
        num_objects: tail.num_objects,
        dwg_version: tail.dwg_version,
        maintenance_version: tail.maintenance_version,
    })
}

/// The three carried values of the five-field record tail R2004 added.
#[derive(Default)]
struct RecordTail {
    num_objects: u32,
    dwg_version: u32,
    maintenance_version: u32,
}

/// `BL num_objects`, `BL dwg_version`, `BL maintenance_version` and two
/// unknown `BL`s — the five fields R2004 added to every class record
/// (§5.8). The two trailing unknowns are 0 on every file measured.
fn read_r2004_record_tail(c: &mut BitCursor<'_>) -> Option<RecordTail> {
    let num_objects = c.read_bl_u().ok()?;
    let dwg_version = c.read_bl_u().ok()?;
    let maintenance_version = c.read_bl_u().ok()?;
    let _unknown1 = c.read_bl().ok()?;
    let _unknown2 = c.read_bl().ok()?;
    Some(RecordTail {
        num_objects,
        dwg_version,
        maintenance_version,
    })
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
// The bit-packed list opens with `BL max_class_number` and one unknown
// `B`, then one record per class. Pre-R2007 a
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

/// Write the five fields R2004 added to every class record — the
/// inverse of [`read_r2004_record_tail`], all five as `BL` per §5.8.
fn write_r2004_record_tail(w: &mut BitWriter, def: &ClassDef) {
    w.write_bl_u(def.num_objects);
    w.write_bl_u(def.dwg_version);
    w.write_bl_u(def.maintenance_version);
    w.write_bl(0); // unknown
    w.write_bl(0); // unknown
}

/// Write an `AcDb:Classes` payload — inverse of [`ClassMap::parse`].
///
/// `writer` is used for internal bit-packing; the returned `Vec<u8>` is
/// the fully-assembled section bytes ready for LZ77 compression by the
/// section writer. The 16-byte sentinel, the byte-aligned size header
/// and the trailing CRC-8 are included; the caller composes this with
/// the R2004+ page framing layer.
///
/// The internal `writer` argument is there purely so the function
/// signature matches the write-path convention used by the element
/// encoders (`trait ElementEncoder`) — the actual class bytes are
/// produced in a fresh [`BitWriter`] and then prefixed / suffixed by
/// the sentinel, size header, and CRC.
///
/// # Not a byte-exact AutoCAD emitter
///
/// This is the inverse of [`ClassMap::parse`], not a reproduction of
/// what AutoCAD writes. Two known differences on R2007+: the
/// `size in bits` header is written as the bit length of the class
/// data alone, where real files carry that length plus 32 (see the
/// module docs), and the §19.4.1 string-stream trailer that real files
/// append after the string block is not emitted. Neither is read back
/// by [`ClassMap::parse`], so the round trip is exact.
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
    inner.write_bl_u(u32::from(class_list_max(classes)));
    inner.write_b(true); // unknown
    if version.is_r2007_plus() {
        for def in &classes.classes {
            inner.write_bs_u(def.class_number);
            inner.write_bs(def.version);
            inner.write_b(def.was_a_proxy);
            inner.write_bs_u(def.item_class_id);
            write_r2004_record_tail(&mut inner, def);
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
                write_r2004_record_tail(&mut inner, def);
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
                    num_objects: 3,
                    dwg_version: 25,
                    maintenance_version: 20,
                },
                ClassDef {
                    class_number: 501,
                    version: 1,
                    app_name: "ObjectDBX Classes".to_string(),
                    cpp_class_name: "AcDbWipeout".to_string(),
                    dxf_class_name: "WIPEOUT".to_string(),
                    was_a_proxy: true,
                    item_class_id: 0x1F2,
                    // The R2018 drift value: > 255, so this field's `BL`
                    // takes the 32-bit tag-`00` form a `BS` misreads.
                    num_objects: 1,
                    dwg_version: 33,
                    maintenance_version: 329,
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

    /// Issue #37 in one fixture: a `maintenance_version` above 255
    /// encodes as a tag-`00` `BL` (2 + 32 bits). Reading it as a `BS`
    /// consumes 2 + 16 and loses sixteen bits of alignment, which on
    /// `sample_AC1032.dwg` desynchronised the class list at record 9
    /// and dropped all 50 classes.
    ///
    /// The fixture puts the oversized field in the *first* of three
    /// records so a `BS` reader cannot reach record 2 at all.
    #[test]
    fn parse_reads_a_maintenance_version_wider_than_a_byte() {
        let map = ClassMap {
            max_class_number: 502,
            classes: (0..3)
                .map(|i| ClassDef {
                    class_number: 500 + i,
                    version: 0,
                    app_name: "ACDB_MLEADERSTYLE_CLASS".to_string(),
                    cpp_class_name: "AcDbMLeaderStyle".to_string(),
                    dxf_class_name: "MLEADERSTYLE".to_string(),
                    was_a_proxy: false,
                    item_class_id: 0x1F3,
                    num_objects: 2,
                    dwg_version: 33,
                    maintenance_version: if i == 0 { 329 } else { 42 },
                })
                .collect(),
        };
        for version in [Version::R2004, Version::R2018] {
            let mut w = BitWriter::new();
            let bytes = write_class_map(&map, &mut w, version).unwrap();
            let parsed = ClassMap::parse(&bytes, version).unwrap();
            assert_eq!(parsed.classes.len(), 3, "{version}");
            assert_eq!(parsed.classes, map.classes, "{version}");
            assert_eq!(parsed.classes[0].maintenance_version, 329, "{version}");
            assert_eq!(parsed.classes[2].class_number, 502, "{version}");
        }
    }

    /// The record tail's three carried values survive the round trip —
    /// `num_objects` is the count this crate cross-checks against the
    /// object stream when validating that the table is located right.
    #[test]
    fn write_class_map_round_trips_the_record_tail_values() {
        let map = sample_map();
        let mut w = BitWriter::new();
        let bytes = write_class_map(&map, &mut w, Version::R2018).unwrap();
        let parsed = ClassMap::parse(&bytes, Version::R2018).unwrap();
        let wipeout = parsed.by_type_code(501).unwrap();
        assert_eq!(wipeout.num_objects, 1);
        assert_eq!(wipeout.dwg_version, 33);
        assert_eq!(wipeout.maintenance_version, 329);
        assert_eq!(parsed.by_type_code(500).unwrap().num_objects, 3);
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
