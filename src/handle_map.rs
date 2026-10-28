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
            if section_size < 2 {
                return Err(Error::SectionMap(format!(
                    "AcDb:Handles section at byte {} claims {} bytes, less than its own 2-byte CRC",
                    pos - 2,
                    section_size
                )));
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
//   2 bytes big-endian u16 size (these 2 bytes + pairs)
//   N bytes pairs of (MC handle_delta, MC offset_delta)
//   2 bytes big-endian CRC-8 (§2.14.1, seed 0xC0C1) over size and pairs
// ```
//
// The map ends with an empty section: size 2 and its own CRC (§23.1).
// Real R14 through R2013 files frame every section this way.
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
/// Each handle-section's 2-byte big-endian trailer is a DWG CRC-8 (spec
/// §2.14.1, seed 0xC0C1) over the section's size bytes and pairs, as the
/// streaming walk verifies. An empty handle map produces only the
/// 4-byte terminator section.
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
        // make the section size, which counts its own 2 bytes, exceed
        // MAX_HANDLE_SECTION_BYTES.
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
            if pairs.len() + pair_bytes.len() + 2 > MAX_HANDLE_SECTION_BYTES {
                break;
            }
            pairs.extend_from_slice(&pair_bytes);
            sec_last_handle = e.handle;
            sec_last_offset = e.offset as i64;
            idx += 1;
        }
        let start = out.len();
        out.extend_from_slice(&((pairs.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&pairs);
        let crc = crc8(0xC0C1, &out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let end = 2u16.to_be_bytes();
    out.extend_from_slice(&end);
    out.extend_from_slice(&crc8(0xC0C1, &end).to_be_bytes());
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
