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

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};

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
    let sv = value as i64;
    Ok(if negate { -sv } else { sv })
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
        let mut payload = Vec::new();
        payload.push(0x01); // h_delta = +1
        payload.push(0xE4); // o_delta byte 0 (continues)
        payload.push(0x00); // o_delta byte 1 (terminates, value = 100)
        payload.push(0x01); // h_delta = +1
        payload.push(0x72); // o_delta = -50
        let mut data = Vec::new();
        data.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        data.extend_from_slice(&payload);
        data.extend_from_slice(&[0x00, 0x00]); // CRC placeholder
        data.extend_from_slice(&[0x00, 0x00]); // terminator section
        let map = HandleMap::parse(&data).unwrap();
        assert_eq!(map.entries.len(), 2);
        assert_eq!(map.entries[0], HandleEntry { handle: 1, offset: 100 });
        assert_eq!(map.entries[1], HandleEntry { handle: 2, offset: 50 });
    }
}
