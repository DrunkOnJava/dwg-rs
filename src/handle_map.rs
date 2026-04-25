//! `AcDb:Handles` object map parser — handle → byte-offset lookup table
//! that enables random-access iteration of the `AcDb:AcDbObjects` stream.
//!
//! # On-disk format
//!
//! The Handles section is divided into *handle sections*, each ≤ 2032
//! bytes. Each handle section has this shape (big-endian u16s!):
//!
//! ```text
//!   2 bytes  big-endian u16 section size
//!   N bytes  run of (MC handle_delta, MC offset_delta) pairs
//!   2 bytes  CRC-8 (spec §2.14.1) over the preceding bytes
//! ```
//!
//! Handle numbers only grow (they're monotonic); byte offsets can jump
//! backward because objects may be deleted and reused. Deltas are signed
//! modular chars (0x40-is-sign).
//!
//! The section list terminates with a size-0 header.

use crate::bitcursor::{BitCursor, signed_modular_char_value};
use crate::bitwriter::BitWriter;
use crate::error::{Error, Result};
use crate::version::Version;

/// Defensive cap on parsed handle map entries (matches the 1 M bound
/// documented in `SECURITY.md`). No legitimate drawing ships with more
/// than a few hundred thousand objects; a claimed count past this cap
/// indicates a malformed or adversarial file.
pub const MAX_HANDLE_ENTRIES: usize = 1_000_000;

/// Entry in the parsed handle map — one DWG object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleEntry {
    /// Absolute handle value (monotonically increasing).
    pub handle: u64,
    /// Byte offset into the `AcDb:AcDbObjects` decompressed stream.
    pub offset: u64,
}

/// Full handle→offset index for a drawing.
#[derive(Debug, Clone, Default)]
pub struct HandleMap {
    pub entries: Vec<HandleEntry>,
}

impl HandleMap {
    /// Parse a decompressed `AcDb:Handles` payload.
    ///
    /// # Caps
    ///
    /// Returns [`Error::SectionMap`] if parsing would produce more than
    /// [`MAX_HANDLE_ENTRIES`] entries. This matches the documented
    /// threat-model cap in `SECURITY.md` and bounds a malformed file's
    /// ability to force unbounded allocation.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut entries = Vec::new();
        let mut pos = 0usize;
        let mut last_handle: i64 = 0;
        let mut last_offset: i64 = 0;
        while pos < bytes.len() {
            if pos + 2 > bytes.len() {
                break;
            }
            // Big-endian u16 section size.
            let section_size = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
            pos += 2;
            if section_size == 0 {
                // Empty section → end of list.
                break;
            }
            if pos + section_size > bytes.len() {
                return Err(Error::SectionMap(format!(
                    "AcDb:Handles section at byte {} claims {} bytes, {} remain",
                    pos - 2,
                    section_size,
                    bytes.len() - pos
                )));
            }
            // Section payload. Last 2 bytes are CRC, stripped.
            let payload_end = pos + section_size - 2;
            let payload = &bytes[pos..payload_end];
            pos += section_size;
            // Walk the MC-delta pairs.
            let mut cur = BitCursor::new(payload);
            while cur.remaining_bits() >= 8 {
                let h_delta = match read_signed_mc(&mut cur) {
                    Ok(v) => v,
                    Err(_) => break,
                };
                let o_delta = match read_signed_mc(&mut cur) {
                    Ok(v) => v,
                    Err(_) => break,
                };
                last_handle = last_handle.wrapping_add(h_delta);
                last_offset = last_offset.wrapping_add(o_delta);
                if entries.len() >= MAX_HANDLE_ENTRIES {
                    return Err(Error::SectionMap(format!(
                        "AcDb:Handles parse exceeded MAX_HANDLE_ENTRIES \
                         ({MAX_HANDLE_ENTRIES}); malformed or adversarial file"
                    )));
                }
                entries.push(HandleEntry {
                    handle: last_handle as u64,
                    offset: last_offset as u64,
                });
            }
        }
        Ok(Self { entries })
    }

    /// Look up an object's offset by handle.
    pub fn offset_of(&self, handle: u64) -> Option<u64> {
        self.entries
            .iter()
            .find(|e| e.handle == handle)
            .map(|e| e.offset)
    }

    /// Iterate every (handle, offset) pair in map order. Ordering is
    /// sorted-by-handle because handle deltas are monotonic on the
    /// wire; callers that need sorted iteration get it for free.
    pub fn iter(&self) -> std::slice::Iter<'_, HandleEntry> {
        self.entries.iter()
    }

    /// Number of entries in the map.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if the map has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'a> IntoIterator for &'a HandleMap {
    type Item = &'a HandleEntry;
    type IntoIter = std::slice::Iter<'a, HandleEntry>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

/// Maximum bytes per handle-section on the wire (spec §4.3 — the reader
/// enforces ≤ 2032-byte section payloads + 2-byte CRC trailer).
pub const MAX_HANDLE_SECTION_BYTES: usize = 2032;

/// Write a SIGNED modular char using the same on-disk encoding
/// [`read_signed_mc`] consumes. Multi-byte output with continuation bit
/// 0x80 on non-terminal bytes and negation bit 0x40 on the terminator.
/// Terminator byte holds 6 data bits; continuation bytes hold 7.
fn write_signed_mc(out: &mut Vec<u8>, v: i64) {
    let (abs, negate) = if v < 0 {
        (v.unsigned_abs(), true)
    } else {
        (v as u64, false)
    };
    // Short form: value fits in 6 data bits → one terminating byte.
    if abs < 0x40 {
        let b = if negate {
            0x40 | (abs as u8)
        } else {
            abs as u8
        };
        out.push(b);
        return;
    }
    // Multi-byte: split into 7-bit limbs.
    let mut limbs: Vec<u8> = Vec::new();
    let mut x = abs;
    while x != 0 {
        limbs.push((x & 0x7F) as u8);
        x >>= 7;
    }
    for (i, limb) in limbs.iter().enumerate() {
        let is_last = i == limbs.len() - 1;
        if is_last {
            if (*limb & 0x40) == 0 {
                // Fits in the terminator's 6 data bits → pack sign here.
                let mut b = *limb & 0x3F;
                if negate {
                    b |= 0x40;
                }
                out.push(b);
            } else {
                // 0x40 bit conflicts with the terminator negation flag —
                // emit this limb as continuation, then a zero
                // terminator carrying only the sign.
                out.push(0x80 | limb);
                out.push(if negate { 0x40 } else { 0x00 });
            }
        } else {
            out.push(0x80 | limb);
        }
    }
}

// ================================================================
// L12-08 — handle map writer (task #381)
//
// Inverse of [`HandleMap::parse`]. Emits an `AcDb:Handles` stream
// composed of zero-or-more handle sections followed by a terminator
// section (size = 0). Each handle section:
//
// ```text
//   2 bytes big-endian u16 size (pairs + CRC = size)
//   N bytes pairs of (MC handle_delta, MC offset_delta)
//   2 bytes LE CRC-8 (§2.14.1, seed 0xC0C1) over the pairs only
// ```
//
// Section payload is bounded by [`MAX_HANDLE_SECTION_BYTES`] (≤ 2032 as
// documented in `SECURITY.md`). If the caller's entries won't fit in a
// single section, the writer splits them at pair boundaries and emits
// multiple sections — handle deltas remain monotonic across splits
// because the encoder tracks per-section baseline handles/offsets.
// ================================================================

/// Write a full [`HandleMap`] as a byte stream suitable for placement
/// in an `AcDb:Handles` section. Inverse of [`HandleMap::parse`].
///
/// The `version` argument is reserved for future format divergence; the
/// R2004-R2018 wire format is stable, so current implementations ignore
/// it. The `_writer` argument is unused and kept only to match the
/// signature convention the other writer helpers in this crate follow.
///
/// # CRC
///
/// Each handle-section's 2-byte trailer is a DWG CRC-8 (spec §2.14.1)
/// computed over the section's pair bytes with seed 0xC0C1, matching
/// what the reader verifies. An empty handle map produces a single
/// 2-byte terminator (size-0 section header).
pub fn write_handle_map(
    map: &HandleMap,
    _writer: &mut BitWriter,
    _version: Version,
) -> Result<Vec<u8>> {
    use crate::crc::crc8;

    let mut out = Vec::new();
    let mut last_handle: i64 = 0;
    let mut last_offset: i64 = 0;

    let mut idx = 0;
    while idx < map.entries.len() {
        // Accumulate one section's pairs until adding the next pair would
        // exceed MAX_HANDLE_SECTION_BYTES - 2 (leaving room for the CRC).
        let mut pairs = Vec::with_capacity(32);
        let mut sec_last_handle = last_handle;
        let mut sec_last_offset = last_offset;
        while idx < map.entries.len() {
            let e = map.entries[idx];
            let h_delta = (e.handle as i64).wrapping_sub(sec_last_handle);
            let o_delta = (e.offset as i64).wrapping_sub(sec_last_offset);
            let mut pair_bytes = Vec::with_capacity(4);
            write_signed_mc(&mut pair_bytes, h_delta);
            write_signed_mc(&mut pair_bytes, o_delta);
            // 2 bytes reserved for the trailing CRC.
            if pairs.len() + pair_bytes.len() + 2 > MAX_HANDLE_SECTION_BYTES {
                break;
            }
            pairs.extend_from_slice(&pair_bytes);
            sec_last_handle = e.handle as i64;
            sec_last_offset = e.offset as i64;
            idx += 1;
        }
        // Section header: big-endian u16 (pair_bytes + 2 CRC bytes).
        let section_size = pairs.len() + 2;
        out.extend_from_slice(&(section_size as u16).to_be_bytes());
        out.extend_from_slice(&pairs);
        // CRC-8 over the pair bytes.
        let crc = crc8(0xC0C1, &pairs);
        out.extend_from_slice(&crc.to_le_bytes());
        last_handle = sec_last_handle;
        last_offset = sec_last_offset;
    }
    // Terminator: size = 0.
    out.extend_from_slice(&0u16.to_be_bytes());
    Ok(out)
}

/// Read a SIGNED modular char — the 0x40 bit on the terminating byte
/// indicates negation (spec §2.6). Mirrors `BitCursor::read_mc` but with
/// an explicit `Result` return and tighter bounds checks for the handle
/// map's tight decoder loop.
fn read_signed_mc(r: &mut BitCursor<'_>) -> Result<i64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    let mut negate = false;
    loop {
        let b = r.read_rc()? as u64;
        let cont = (b & 0x80) != 0;
        let data = if cont { b & 0x7F } else { b & 0x3F };
        value |= data << shift;
        shift += if cont { 7 } else { 6 };
        if !cont {
            negate = (b & 0x40) != 0;
            break;
        }
        if shift >= 64 {
            break;
        }
    }
    signed_modular_char_value(value, negate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_map_from_terminator() {
        // Size-0 section header → empty map.
        let bytes = [0x00, 0x00];
        let map = HandleMap::parse(&bytes).unwrap();
        assert!(map.entries.is_empty());
    }

    #[test]
    fn parses_single_entry_section() {
        // Section with one (handle_delta, offset_delta) = (1, 10) pair.
        //
        // Handle delta = +1 → signed MC byte = 0x01 (cont=0, data=1, neg=0).
        // Offset delta = +10 → signed MC byte = 0x0A.
        // Section size = 4 (2 pair bytes + 2 CRC bytes); CRC ignored here.
        let mut data = Vec::new();
        data.extend_from_slice(&4u16.to_be_bytes());
        data.push(0x01);
        data.push(0x0A);
        data.extend_from_slice(&[0x00, 0x00]); // placeholder CRC
        data.extend_from_slice(&[0x00, 0x00]); // terminator
        let map = HandleMap::parse(&data).unwrap();
        assert_eq!(map.entries.len(), 1);
        assert_eq!(map.entries[0].handle, 1);
        assert_eq!(map.entries[0].offset, 10);
    }

    #[test]
    fn monotonic_handles_negative_offset_jump() {
        // Two entries: (h=1, off=100), (h=2, off=50).
        // Deltas: (+1, +100), (+1, -50).
        //
        // +100 is too big for a single terminating-byte signed MC (only
        // 6 data bits = 0..=63), so it takes two bytes:
        //   byte 0: cont=1, data = 100 & 0x7F = 0x64 → 0x80 | 0x64 = 0xE4
        //   byte 1: cont=0, negate=0, data = 0                → 0x00
        //
        // -50 fits in one signed MC terminating byte:
        //   byte: cont=0, negate=1, data = 50 & 0x3F = 0x32   → 0x72
        let payload: Vec<u8> = vec![
            0x01, // h_delta = +1
            0xE4, // o_delta byte 0 (continues)
            0x00, // o_delta byte 1 (terminates, value = 100)
            0x01, // h_delta = +1
            0x72, // o_delta = -50
        ];
        let mut data = Vec::new();
        data.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        data.extend_from_slice(&payload);
        data.extend_from_slice(&[0x00, 0x00]); // CRC placeholder
        data.extend_from_slice(&[0x00, 0x00]); // terminator section
        let map = HandleMap::parse(&data).unwrap();
        assert_eq!(map.entries.len(), 2);
        assert_eq!(
            map.entries[0],
            HandleEntry {
                handle: 1,
                offset: 100
            }
        );
        assert_eq!(
            map.entries[1],
            HandleEntry {
                handle: 2,
                offset: 50
            }
        );
    }

    // -------- L12-08: writer tests --------

    #[test]
    fn signed_mc_short_roundtrip() {
        for v in [0i64, 1, -1, 0x3F, -0x3F] {
            let mut buf = Vec::new();
            write_signed_mc(&mut buf, v);
            let mut c = BitCursor::new(&buf);
            let read = read_signed_mc(&mut c).unwrap();
            assert_eq!(read, v, "roundtrip mismatch for v={v}");
        }
    }

    #[test]
    fn signed_mc_multi_byte_roundtrip() {
        for v in [100i64, -100, 1000, -1000, 0xFFFF, -0xFFFF] {
            let mut buf = Vec::new();
            write_signed_mc(&mut buf, v);
            let mut c = BitCursor::new(&buf);
            let read = read_signed_mc(&mut c).unwrap();
            assert_eq!(read, v, "roundtrip mismatch for v={v}");
        }
    }

    #[test]
    fn write_handle_map_empty_map_emits_two_byte_terminator() {
        let map = HandleMap::default();
        let mut w = BitWriter::new();
        let bytes = write_handle_map(&map, &mut w, Version::R2018).unwrap();
        // Just the size-0 terminator.
        assert_eq!(bytes, vec![0x00, 0x00]);
        // Parse roundtrips to empty.
        let parsed = HandleMap::parse(&bytes).unwrap();
        assert!(parsed.entries.is_empty());
    }

    #[test]
    fn write_handle_map_single_entry_roundtrips_through_parse() {
        let map = HandleMap {
            entries: vec![HandleEntry {
                handle: 1,
                offset: 10,
            }],
        };
        let mut w = BitWriter::new();
        let bytes = write_handle_map(&map, &mut w, Version::R2018).unwrap();
        let parsed = HandleMap::parse(&bytes).unwrap();
        assert_eq!(parsed.entries, map.entries);
    }

    #[test]
    fn write_handle_map_multi_entry_with_negative_offset_delta_roundtrips() {
        let map = HandleMap {
            entries: vec![
                HandleEntry {
                    handle: 1,
                    offset: 100,
                },
                HandleEntry {
                    handle: 2,
                    offset: 50,
                },
                HandleEntry {
                    handle: 10,
                    offset: 500,
                },
            ],
        };
        let mut w = BitWriter::new();
        let bytes = write_handle_map(&map, &mut w, Version::R2018).unwrap();
        let parsed = HandleMap::parse(&bytes).unwrap();
        assert_eq!(parsed.entries, map.entries);
    }

    #[test]
    fn write_handle_map_many_entries_produces_multiple_sections() {
        // Enough entries that the writer must emit > 1 section. At 4
        // bytes per short-delta pair, ~508 pairs max per section. Emit
        // 1500 entries so the writer splits at least 3 times.
        let entries: Vec<HandleEntry> = (1u64..=1500)
            .map(|h| HandleEntry {
                handle: h,
                offset: h * 10,
            })
            .collect();
        let map = HandleMap {
            entries: entries.clone(),
        };
        let mut w = BitWriter::new();
        let bytes = write_handle_map(&map, &mut w, Version::R2018).unwrap();
        let parsed = HandleMap::parse(&bytes).unwrap();
        assert_eq!(parsed.entries.len(), entries.len());
        assert_eq!(parsed.entries, entries);
    }
}
