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
//! Two properties of the delta run are easy to get wrong and were both
//! wrong here until the `sample_AC1032.dwg` audit in #43/#44:
//!
//! 1. **The handle delta is an UNSIGNED modular char; only the offset
//!    delta is signed.** Handle numbers inside one handle section only
//!    grow, so the encoder has no sign to spend — every one of the
//!    terminating byte's low 7 bits is magnitude. Decoding the handle
//!    delta as a *signed* MC steals bit 0x40 as a negation flag, which
//!    silently halves any single-byte delta ≥ 0x40 and mis-splits the
//!    multi-byte form. Byte offsets, by contrast, do jump backward
//!    (objects get deleted and their space reused), so the offset delta
//!    keeps the signed encoding.
//! 2. **Both accumulators restart at zero on every handle section.**
//!    The first pair of a section carries the absolute handle and the
//!    absolute offset, not a delta from the previous section's last
//!    entry. Carrying the running totals across the section boundary
//!    throws every entry after the first section into a fabricated
//!    address space.
//!
//! Both facts are verified against the file itself: each record in
//! `AcDb:AcDbObjects` repeats its own handle, so "does the record at
//! the offset this map produced carry the handle this map produced?"
//! is a self-checking oracle. On `sample_AC1032.dwg` it agrees for
//! 842 of 842 entries under these rules and for 272 of 842 without
//! them.
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
            // Walk the MC-delta pairs. Both running totals restart at
            // zero for every handle section — the section's first pair
            // is absolute (see the module docs).
            let mut last_handle: u64 = 0;
            let mut last_offset: i64 = 0;
            let mut cur = BitCursor::new(payload);
            while cur.remaining_bits() >= 8 {
                let h_delta = match read_unsigned_mc(&mut cur) {
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
                    handle: last_handle,
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

/// Write an UNSIGNED modular char — 7 magnitude bits per byte, 0x80 on
/// every non-terminal byte. Inverse of [`read_unsigned_mc`].
fn write_unsigned_mc(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let limb = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(limb);
            return;
        }
        out.push(0x80 | limb);
    }
}

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

    let mut idx = 0;
    while idx < map.entries.len() {
        // Accumulate one section's pairs until adding the next pair would
        // exceed MAX_HANDLE_SECTION_BYTES - 2 (leaving room for the CRC).
        // Both baselines start at zero: the reader restarts its
        // accumulators on every section, so the first pair is absolute.
        let mut pairs = Vec::with_capacity(32);
        let mut sec_last_handle: u64 = 0;
        let mut sec_last_offset: i64 = 0;
        while idx < map.entries.len() {
            let e = map.entries[idx];
            // The handle delta is unsigned on the wire, so a handle that
            // moves backward cannot be encoded inside the current
            // section. Close the section: the next one restarts from
            // zero, where any absolute handle is representable.
            let Some(h_delta) = e.handle.checked_sub(sec_last_handle) else {
                break;
            };
            let o_delta = (e.offset as i64).wrapping_sub(sec_last_offset);
            let mut pair_bytes = Vec::with_capacity(4);
            write_unsigned_mc(&mut pair_bytes, h_delta);
            write_signed_mc(&mut pair_bytes, o_delta);
            // 2 bytes reserved for the trailing CRC.
            if pairs.len() + pair_bytes.len() + 2 > MAX_HANDLE_SECTION_BYTES {
                break;
            }
            pairs.extend_from_slice(&pair_bytes);
            sec_last_handle = e.handle;
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
    }
    // Terminator: size = 0.
    out.extend_from_slice(&0u16.to_be_bytes());
    Ok(out)
}

/// Read an UNSIGNED modular char — every byte contributes 7 magnitude
/// bits and the 0x80 bit is the only flag (spec §2.6). Used for the
/// handle delta, which cannot be negative inside one handle section.
fn read_unsigned_mc(r: &mut BitCursor<'_>) -> Result<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let b = r.read_rc()? as u64;
        value |= (b & 0x7F) << shift;
        shift += 7;
        if (b & 0x80) == 0 {
            return Ok(value);
        }
        if shift >= 64 {
            return Ok(value);
        }
    }
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

    #[test]
    fn handle_delta_is_unsigned_so_bit_0x40_is_magnitude_not_sign() {
        // One pair: handle delta byte 0x56, offset delta byte 0x04.
        //
        // Read as an UNSIGNED modular char, 0x56 is 86. Read as a
        // SIGNED one it would be -(0x56 & 0x3F) = -22 — the decode bug
        // this test pins. 86 is the correct value: handle numbers only
        // grow inside a handle section, so the encoder spends all seven
        // low bits on magnitude.
        let payload: Vec<u8> = vec![0x56, 0x04];
        let mut data = Vec::new();
        data.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        data.extend_from_slice(&payload);
        data.extend_from_slice(&[0x00, 0x00]); // CRC placeholder
        data.extend_from_slice(&[0x00, 0x00]); // terminator
        let map = HandleMap::parse(&data).unwrap();
        assert_eq!(map.entries.len(), 1);
        assert_eq!(map.entries[0].handle, 86);
        assert_eq!(map.entries[0].offset, 4);
    }

    #[test]
    fn multi_byte_handle_delta_carries_seven_bits_in_its_terminator() {
        // 0xC1 0x02 = continuation limb 0x41 (65) + terminator 0x02.
        // Unsigned: 65 | (2 << 7) = 321. The signed reading would mask
        // the terminator to six bits — same value here — but would also
        // treat a terminator with 0x40 set as a sign; the companion
        // pair below (0x81 0x41) proves the difference: unsigned gives
        // 1 | (0x41 << 7) = 8321, signed would give -1.
        let payload: Vec<u8> = vec![0xC1, 0x02, 0x01, 0x81, 0x41, 0x01];
        let mut data = Vec::new();
        data.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        data.extend_from_slice(&payload);
        data.extend_from_slice(&[0x00, 0x00]);
        data.extend_from_slice(&[0x00, 0x00]);
        let map = HandleMap::parse(&data).unwrap();
        assert_eq!(map.entries.len(), 2);
        assert_eq!(map.entries[0].handle, 321);
        assert_eq!(map.entries[1].handle, 321 + 8321);
    }

    #[test]
    fn accumulators_restart_on_every_handle_section() {
        // Section 1: (handle 5, offset 100). Section 2 opens with the
        // pair (7, 40) — which is the ABSOLUTE handle 7 at ABSOLUTE
        // offset 40, not handle 12 at offset 140. Handle 7 < 12 also
        // makes the two readings distinguishable in one assertion.
        let mut data = Vec::new();
        let s1: Vec<u8> = vec![0x05, 0xE4, 0x00]; // h=+5, o=+100 (two-byte MC)
        data.extend_from_slice(&((s1.len() + 2) as u16).to_be_bytes());
        data.extend_from_slice(&s1);
        data.extend_from_slice(&[0x00, 0x00]);
        let s2: Vec<u8> = vec![0x07, 0x28]; // h=7, o=40
        data.extend_from_slice(&((s2.len() + 2) as u16).to_be_bytes());
        data.extend_from_slice(&s2);
        data.extend_from_slice(&[0x00, 0x00]);
        data.extend_from_slice(&[0x00, 0x00]); // terminator
        let map = HandleMap::parse(&data).unwrap();
        assert_eq!(
            map.entries,
            vec![
                HandleEntry {
                    handle: 5,
                    offset: 100
                },
                HandleEntry {
                    handle: 7,
                    offset: 40
                },
            ]
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
    fn write_handle_map_roundtrips_deltas_that_set_bit_0x40() {
        // Handle deltas of 64..=127 are exactly the range the signed-MC
        // misreading corrupted; make the writer/reader pair prove them.
        let entries: Vec<HandleEntry> = (0u64..64)
            .map(|i| HandleEntry {
                handle: 1 + i * 86,
                offset: 4 + i * 70,
            })
            .collect();
        let map = HandleMap {
            entries: entries.clone(),
        };
        let mut w = BitWriter::new();
        let bytes = write_handle_map(&map, &mut w, Version::R2018).unwrap();
        assert_eq!(HandleMap::parse(&bytes).unwrap().entries, entries);
    }

    #[test]
    fn write_handle_map_splits_a_section_when_the_handle_moves_backward() {
        // The handle delta is unsigned on the wire, so a backward jump
        // has to open a new section (whose accumulators restart at 0).
        let entries = vec![
            HandleEntry {
                handle: 500,
                offset: 10,
            },
            HandleEntry {
                handle: 900,
                offset: 20,
            },
            HandleEntry {
                handle: 7,
                offset: 30,
            },
            HandleEntry {
                handle: 9,
                offset: 40,
            },
        ];
        let map = HandleMap {
            entries: entries.clone(),
        };
        let mut w = BitWriter::new();
        let bytes = write_handle_map(&map, &mut w, Version::R2018).unwrap();
        // Two data sections plus the 2-byte terminator.
        let mut sections = 0;
        let mut pos = 0usize;
        while pos + 2 <= bytes.len() {
            let size = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
            if size == 0 {
                break;
            }
            sections += 1;
            pos += 2 + size;
        }
        assert_eq!(sections, 2);
        assert_eq!(HandleMap::parse(&bytes).unwrap().entries, entries);
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
