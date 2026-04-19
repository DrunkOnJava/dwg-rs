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
use crate::error::{Error, Result};
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
        let size_in_bits = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
        let max_class_number_pos = 20;
        if max_class_number_pos + 4 > bytes.len() {
            return Ok(Self::default());
        }
        let max_class_number =
            u32::from_le_bytes(bytes[max_class_number_pos..max_class_number_pos + 4].try_into().unwrap());
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

/// Read a variable text (TV) — R2004 and earlier use 8-bit strings,
/// R2007+ uses UTF-16LE strings (spec §2). For simplicity we read as
/// 8-bit for all versions here; R2007+ strings will often read as valid
/// ASCII when the content is ASCII-only (app names, C++ class names are
/// always ASCII in practice).
fn read_tv(c: &mut BitCursor<'_>, _version: Version) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        out.push(c.read_rc()?);
    }
    // Strip trailing NUL if any.
    if out.last() == Some(&0) {
        out.pop();
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
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
}
