//! `AcDb:Classes` section — custom (dynamic) class definitions that extend
//! the built-in DWG object type space (codes < 500).
//!
//! Every object type code ≥ 500 found in the object stream maps to an
//! index into this table via `class_index = type_code - 500`. The table
//! entry carries the application name ("AcDbObjects"), the C++ class
//! name ("AcDbTable"), the DXF record type name ("TABLE"), and a proxy
//! flag that says whether the writer's application was available at save.
//!
//! # On-disk format (R2004+)
//!
//! After the standard 16-byte sentinel + 4-byte size header, the class
//! data is a bit-packed sequence:
//!
//! ```text
//!   RL   max_class_number  (always 0x1F3 on files with no custom classes)
//!   B    ? (unknown flag)
//!   // then repeated until size reached:
//!   BS   class_number
//!   BS   version / proxy_flag (R2007+ splits these)
//!   TV   app_name
//!   TV   cpp_class_name
//!   TV   dxf_class_name
//!   B    was_a_proxy
//!   BS   is_an_entity (0x1F2 for proxy entity, 0x1F3 for proxy object)
//! ```
//!
//! Phase D-3 parses the full entry list; Phase E will resolve `Custom(u16)`
//! object types against this table when walking the object stream.

use crate::bitcursor::BitCursor;
use crate::bitwriter::BitWriter;
use crate::error::Result;
use crate::tables::read_tv;
use crate::version::Version;

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
        // The first 16 bytes are a sentinel we can validate but don't need.
        if bytes.len() < 20 {
            return Ok(Self::default());
        }
        // Skip 16-byte sentinel + 4-byte size-in-bits header.
        // The length pre-check above guarantees bytes[16..20] is 4 bytes.
        let size_in_bits = u32::from_le_bytes(
            bytes[16..20]
                .try_into()
                .expect("slice 16..20 is length 4 by length pre-check"),
        ) as usize;
        let max_class_number_pos = 20;
        if max_class_number_pos + 4 > bytes.len() {
            return Ok(Self::default());
        }
        let max_class_number = u32::from_le_bytes(
            bytes[max_class_number_pos..max_class_number_pos + 4]
                .try_into()
                .expect("4-byte slice guaranteed by length pre-check above"),
        );
        // Bit-level parsing starts at byte 24.
        let bit_start = 24;
        if bit_start >= bytes.len() {
            return Ok(Self::default());
        }
        let mut c = BitCursor::new(&bytes[bit_start..]);
        // Skip unknown B flag (present in R2007+).
        if version.is_r2007_plus() {
            let _ = c.read_b();
        }
        let mut classes = Vec::new();
        // Read entries until we exhaust the declared bit-count or hit EOF.
        let max_bits = size_in_bits.saturating_sub(8 * 4);
        while c.position_bits() < max_bits && c.remaining_bits() >= 64 {
            let class_number = match c.read_bs_u() {
                Ok(v) => v,
                Err(_) => break,
            };
            let version_flag = c.read_bs().unwrap_or(0);
            let app_name = read_tv(&mut c, version).unwrap_or_default();
            let cpp_class_name = read_tv(&mut c, version).unwrap_or_default();
            let dxf_class_name = read_tv(&mut c, version).unwrap_or_default();
            let was_a_proxy = c.read_b().unwrap_or(false);
            let item_class_id = c.read_bs_u().unwrap_or(0);
            // Stop if we've obviously overrun.
            if app_name.is_empty() && cpp_class_name.is_empty() && dxf_class_name.is_empty() {
                break;
            }
            classes.push(ClassDef {
                class_number,
                version: version_flag,
                app_name,
                cpp_class_name,
                dxf_class_name,
                was_a_proxy,
                item_class_id,
            });
            if classes.len() > 4096 {
                // Defensive upper bound — no realistic drawing has this
                // many custom classes; bail to prevent runaway reads.
                break;
            }
        }
        Ok(Self {
            max_class_number,
            classes,
        })
    }

    /// Look up a class by its type code (for object_type.rs `Custom(N)`).
    pub fn by_type_code(&self, type_code: u16) -> Option<&ClassDef> {
        self.classes.iter().find(|c| c.class_number == type_code)
    }
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
// Produced stream:
//
// ```text
// [0..16]   16 ASCII bytes         — ClassMap::SENTINEL_START
// [16..20]  4 bytes little-endian  — total bit-count of classes payload
// [20..24]  4 bytes little-endian  — max_class_number
// [24..]    bit-packed sequence of ClassDef entries
// ```
//
// Each `ClassDef` is written as:
//
// ```text
// BS  class_number
// BS  version / proxy_flag
// TV  app_name
// TV  cpp_class_name
// TV  dxf_class_name
// B   was_a_proxy
// BS  item_class_id
// ```
//
// On R2007+ a leading B is emitted before the first entry (decoder reads
// and discards it). A trailing CRC-8 (§2.14.1) is appended over the
// entire byte-aligned classes payload — seed 0xC0C1 per the reader's
// implicit convention when the section is R2004+.
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
    // R2007+ files have a leading B flag the parser discards.
    if version.is_r2007_plus() {
        inner.write_b(false);
    }
    for def in &classes.classes {
        inner.write_bs_u(def.class_number);
        inner.write_bs(def.version);
        write_tv(&mut inner, &def.app_name, version);
        write_tv(&mut inner, &def.cpp_class_name, version);
        write_tv(&mut inner, &def.dxf_class_name, version);
        inner.write_b(def.was_a_proxy);
        inner.write_bs_u(def.item_class_id);
    }
    let bits_written = inner.position_bits();
    let classes_bytes = inner.into_bytes();

    // Assemble: sentinel(16) + size_in_bits(4 LE) + max_class_number(4 LE)
    //         + classes payload + CRC-8(2 LE).
    let mut out = Vec::with_capacity(26 + classes_bytes.len());
    out.extend_from_slice(&ClassMap::SENTINEL_START);
    out.extend_from_slice(&(bits_written as u32).to_le_bytes());
    out.extend_from_slice(&classes.max_class_number.to_le_bytes());
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
        // sentinel(16) + size(4) + max(4) + CRC(2) = 26 bytes.
        assert_eq!(bytes.len(), 26);
        assert_eq!(&bytes[..16], &ClassMap::SENTINEL_START);
        // max_class_number at [20..24]
        let max = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
        assert_eq!(max, 0x1F3);
    }

    #[test]
    fn write_class_map_r2007_prefix_bit_is_emitted() {
        let map = ClassMap {
            max_class_number: 0x1F3,
            classes: Vec::new(),
        };
        let mut w = BitWriter::new();
        let r2004 = write_class_map(&map, &mut w, Version::R2004).unwrap();
        let mut w2 = BitWriter::new();
        let r2018 = write_class_map(&map, &mut w2, Version::R2018).unwrap();
        // R2018 payload includes 1 leading bit → same byte count after
        // alignment but a distinct size-in-bits header.
        let bits_r2004 = u32::from_le_bytes(r2004[16..20].try_into().unwrap());
        let bits_r2018 = u32::from_le_bytes(r2018[16..20].try_into().unwrap());
        assert_eq!(bits_r2004, 0);
        assert_eq!(bits_r2018, 1);
    }

    #[test]
    fn write_class_map_single_entry_is_readable_by_parse() {
        // Build a map with one definition, write it, then parse the bytes
        // back and check the parsed table shape matches what we wrote.
        let def = ClassDef {
            class_number: 500,
            version: 0,
            app_name: "AcDbObjects".to_string(),
            cpp_class_name: "AcDbTable".to_string(),
            dxf_class_name: "TABLE".to_string(),
            was_a_proxy: false,
            item_class_id: 0x1F2,
        };
        let map = ClassMap {
            max_class_number: 500,
            classes: vec![def.clone()],
        };
        let mut w = BitWriter::new();
        let bytes = write_class_map(&map, &mut w, Version::R2004).unwrap();

        let parsed = ClassMap::parse(&bytes, Version::R2004).unwrap();
        assert_eq!(parsed.max_class_number, 500);
        assert_eq!(parsed.classes.len(), 1);
        let read = &parsed.classes[0];
        assert_eq!(read.class_number, def.class_number);
        assert_eq!(read.app_name, def.app_name);
        assert_eq!(read.cpp_class_name, def.cpp_class_name);
        assert_eq!(read.dxf_class_name, def.dxf_class_name);
        assert_eq!(read.was_a_proxy, def.was_a_proxy);
        assert_eq!(read.item_class_id, def.item_class_id);
    }

    #[test]
    fn write_class_map_utf16_roundtrip_on_r2018() {
        let def = ClassDef {
            class_number: 501,
            version: 1,
            app_name: "AcDbObjects".to_string(),
            cpp_class_name: "AcDbWipeout".to_string(),
            dxf_class_name: "WIPEOUT".to_string(),
            was_a_proxy: false,
            item_class_id: 0x1F2,
        };
        let map = ClassMap {
            max_class_number: 501,
            classes: vec![def.clone()],
        };
        let mut w = BitWriter::new();
        let bytes = write_class_map(&map, &mut w, Version::R2018).unwrap();
        let parsed = ClassMap::parse(&bytes, Version::R2018).unwrap();
        assert_eq!(parsed.classes.len(), 1);
        assert_eq!(parsed.classes[0].dxf_class_name, "WIPEOUT");
        assert_eq!(parsed.classes[0].cpp_class_name, "AcDbWipeout");
    }
}
