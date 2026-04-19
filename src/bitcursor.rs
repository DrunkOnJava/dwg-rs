//! Bit-level cursor over a byte slice, implementing the DWG primitive types
//! from ODA Open Design Specification v5.4.1 §2 ("BIT CODES AND DATA
//! DEFINITIONS").
//!
//! All integer types read little-endian at the byte level, but bit-streams
//! are consumed most-significant-bit first from each byte — the convention
//! the spec uses throughout.
//!
//! # Coverage
//!
//! | Spec | Method                 | Semantics |
//! |------|------------------------|-----------|
//! | §2.0 | [`BitCursor::read_b`]      | single bit → bool |
//! | §2.0 | [`BitCursor::read_bb`]     | two-bit code 0..3 |
//! | §2.1 | [`BitCursor::read_3b`]     | 1-3 bits, variable-length 0..7 |
//! | §2.2 | [`BitCursor::read_bs`]     | bitshort: 00=16-bit / 01=8-bit / 10=0 / 11=256 |
//! | §2.3 | [`BitCursor::read_bl`]     | bitlong: 00=32-bit / 01=8-bit / 10=0 / 11=reserved |
//! | §2.4 | [`BitCursor::read_bll`]    | bitlonglong: 3-bit length + that many LE bytes |
//! | §2.5 | [`BitCursor::read_bd`]     | bitdouble: 00=f64 / 01=1.0 / 10=0.0 / 11=reserved |
//! | §2   | [`BitCursor::read_rc`]     | raw u8 (byte-aligned) |
//! | §2   | [`BitCursor::read_rs`]     | raw u16 LE |
//! | §2   | [`BitCursor::read_rl`]     | raw u32 LE |
//! | §2   | [`BitCursor::read_rd`]     | raw f64 LE |
//! | §2.6 | [`BitCursor::read_mc`]     | modular char, signed (0x40 negation flag) |
//! | §2.7 | [`BitCursor::read_ms`]     | modular short, unsigned |
//! | §2.13| [`BitCursor::read_handle`] | handle reference (CODE.COUNTER.bytes) |
//!
//! Individual bit reads advance within a byte; `align_to_byte()` drops the
//! remaining bits in the current byte (used between CRC-aligned objects, per
//! spec §2.14: "They always appear on byte boundaries. Thus there may be
//! extra unused bits at the end of an object.").

use crate::error::{Error, Result};

/// Bit-level cursor over a byte slice.
///
/// Tracks a byte index and a within-byte bit index. Methods read primitive
/// types per the ODA spec and advance the cursor. All errors are
/// `Error::BitsExhausted` unless specifically noted.
#[derive(Debug, Clone)]
pub struct BitCursor<'a> {
    bytes: &'a [u8],
    byte_pos: usize,
    bit_pos: u8, // 0..7, MSB-first (0 is the top bit of the current byte)
}

impl<'a> BitCursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    /// Total bits available in the underlying slice (regardless of position).
    pub fn total_bits(&self) -> usize {
        self.bytes.len() * 8
    }

    /// Bits remaining from the current position to the end of the slice.
    pub fn remaining_bits(&self) -> usize {
        self.total_bits()
            .saturating_sub(self.byte_pos * 8 + self.bit_pos as usize)
    }

    /// Current bit position (0 == start of file/slice).
    pub fn position_bits(&self) -> usize {
        self.byte_pos * 8 + self.bit_pos as usize
    }

    /// Drop any bits remaining in the current byte — advance to the next
    /// byte boundary. Required before reading aligned CRC values (spec §2.14).
    pub fn align_to_byte(&mut self) {
        if self.bit_pos != 0 {
            self.byte_pos += 1;
            self.bit_pos = 0;
        }
    }

    fn need(&self, bits: usize) -> Result<()> {
        if self.remaining_bits() < bits {
            Err(Error::BitsExhausted {
                wanted: bits,
                remaining: self.remaining_bits(),
            })
        } else {
            Ok(())
        }
    }

    /// Read N bits (N ≤ 64) as a big-endian-within-bitstream u64.
    ///
    /// Conceptually: take the next N bits starting at the current cursor
    /// position, interpret them as a binary number (MSB first), and return.
    fn read_bits(&mut self, n: usize) -> Result<u64> {
        debug_assert!(n <= 64);
        self.need(n)?;
        let mut out: u64 = 0;
        let mut taken = 0;
        while taken < n {
            let byte = self.bytes[self.byte_pos];
            let avail = 8 - self.bit_pos as usize;
            let take = (n - taken).min(avail);
            // Extract `take` bits starting at self.bit_pos (MSB-first).
            let shift_right = avail - take;
            let mask = if take == 8 {
                0xFFu8
            } else {
                ((1u16 << take) - 1) as u8
            };
            let chunk = ((byte >> shift_right) & mask) as u64;
            out = (out << take) | chunk;

            self.bit_pos += take as u8;
            if self.bit_pos == 8 {
                self.byte_pos += 1;
                self.bit_pos = 0;
            }
            taken += take;
        }
        Ok(out)
    }

    // ================================================================
    // §2.0 — B (1 bit)
    // ================================================================

    pub fn read_b(&mut self) -> Result<bool> {
        Ok(self.read_bits(1)? != 0)
    }

    // ================================================================
    // §2.0 — BB (2 bits, used heavily as dispatch tag)
    // ================================================================

    pub fn read_bb(&mut self) -> Result<u8> {
        Ok(self.read_bits(2)? as u8)
    }

    // ================================================================
    // §2.1 — 3B (1 to 3 bits; used from R24 onward for entmode-like fields)
    //
    // "Keep reading bits until a zero bit is encountered or until the 3rd
    //  bit is read, whatever comes first. Each time a bit is read, shift
    //  the previously read bits to the left. Result is 0-7."
    // ================================================================

    pub fn read_3b(&mut self) -> Result<u8> {
        let mut result: u8 = 0;
        for _ in 0..3 {
            let bit = self.read_bits(1)? as u8;
            result = (result << 1) | bit;
            if bit == 0 {
                break;
            }
        }
        Ok(result)
    }

    // ================================================================
    // §2.2 — BS (bitshort)
    //   00 → 16-bit LE short follows
    //   01 → 8-bit unsigned char follows
    //   10 → 0
    //   11 → 256
    // ================================================================

    pub fn read_bs(&mut self) -> Result<i16> {
        match self.read_bb()? {
            0b00 => {
                // 16-bit LE short.
                // Read two raw bytes LSB first, per spec.
                let lsb = self.read_bits(8)? as u16;
                let msb = self.read_bits(8)? as u16;
                Ok(((msb << 8) | lsb) as i16)
            }
            0b01 => Ok(self.read_bits(8)? as i16),
            0b10 => Ok(0),
            0b11 => Ok(256),
            _ => unreachable!(),
        }
    }

    /// Unsigned variant — useful when the context guarantees non-negative
    /// (e.g., section counts).
    pub fn read_bs_u(&mut self) -> Result<u16> {
        Ok(self.read_bs()? as u16)
    }

    // ================================================================
    // §2.3 — BL (bitlong)
    //   00 → 32-bit LE long follows
    //   01 → 8-bit unsigned char follows
    //   10 → 0
    //   11 → not used (reserved)
    // ================================================================

    pub fn read_bl(&mut self) -> Result<i32> {
        match self.read_bb()? {
            0b00 => {
                let b0 = self.read_bits(8)? as u32;
                let b1 = self.read_bits(8)? as u32;
                let b2 = self.read_bits(8)? as u32;
                let b3 = self.read_bits(8)? as u32;
                Ok(((b3 << 24) | (b2 << 16) | (b1 << 8) | b0) as i32)
            }
            0b01 => Ok(self.read_bits(8)? as i32),
            0b10 => Ok(0),
            0b11 => Err(Error::ReservedBitPattern {
                code_type: "BL",
                pattern: "11",
            }),
            _ => unreachable!(),
        }
    }

    pub fn read_bl_u(&mut self) -> Result<u32> {
        Ok(self.read_bl()? as u32)
    }

    // ================================================================
    // §2.4 — BLL (bitlonglong), R24+
    //
    // "The first 1-3 bits indicate the length l (see paragraph 2.1). Then
    //  l bytes follow, which represent the number (least significant byte
    //  first)."
    // ================================================================

    pub fn read_bll(&mut self) -> Result<u64> {
        let len = self.read_3b()? as usize;
        let mut out: u64 = 0;
        for i in 0..len {
            let b = self.read_bits(8)?;
            out |= b << (i * 8);
        }
        Ok(out)
    }

    // ================================================================
    // §2.5 — BD (bitdouble)
    //   00 → 8-byte IEEE double follows
    //   01 → 1.0
    //   10 → 0.0
    //   11 → reserved
    // ================================================================

    pub fn read_bd(&mut self) -> Result<f64> {
        match self.read_bb()? {
            0b00 => {
                let mut bs = [0u8; 8];
                for b in &mut bs {
                    *b = self.read_bits(8)? as u8;
                }
                Ok(f64::from_le_bytes(bs))
            }
            0b01 => Ok(1.0),
            0b10 => Ok(0.0),
            0b11 => Err(Error::ReservedBitPattern {
                code_type: "BD",
                pattern: "11",
            }),
            _ => unreachable!(),
        }
    }

    // ================================================================
    // Raw (byte-aligned) types. These still honor bit_pos, but are not
    // "compressed" — they always consume the stated number of bytes.
    // ================================================================

    pub fn read_rc(&mut self) -> Result<u8> {
        Ok(self.read_bits(8)? as u8)
    }

    pub fn read_rs(&mut self) -> Result<i16> {
        let lsb = self.read_bits(8)? as u16;
        let msb = self.read_bits(8)? as u16;
        Ok(((msb << 8) | lsb) as i16)
    }

    pub fn read_rl(&mut self) -> Result<u32> {
        let b0 = self.read_bits(8)? as u32;
        let b1 = self.read_bits(8)? as u32;
        let b2 = self.read_bits(8)? as u32;
        let b3 = self.read_bits(8)? as u32;
        Ok((b3 << 24) | (b2 << 16) | (b1 << 8) | b0)
    }

    pub fn read_rd(&mut self) -> Result<f64> {
        let mut bs = [0u8; 8];
        for b in &mut bs {
            *b = self.read_bits(8)? as u8;
        }
        Ok(f64::from_le_bytes(bs))
    }

    // ================================================================
    // §2.6 — Modular chars (MC), signed
    //
    // Byte stream; high bit = continuation flag.  Bytes are consumed in
    // LSB-first order. The terminating byte's 0x40 bit indicates negation.
    // ================================================================

    pub fn read_mc(&mut self) -> Result<i64> {
        let mut value: u64 = 0;
        let mut shift: u32 = 0;
        let mut negate = false;
        loop {
            let b = self.read_rc()? as u64;
            let cont = (b & 0x80) != 0;
            // The final byte (cont==false) uses 0x40 as the negation flag;
            // continuation bytes contribute 7 data bits each.
            let data = if cont { b & 0x7F } else { b & 0x3F };
            value |= data << shift;
            shift += if cont { 7 } else { 6 };
            if !cont {
                negate = (b & 0x40) != 0;
                break;
            }
            if shift >= 64 {
                break; // defensive: don't overflow
            }
        }
        let sv = value as i64;
        Ok(if negate { -sv } else { sv })
    }

    // ================================================================
    // §2.7 — Modular shorts (MS), unsigned
    //
    // Same as MC but 2-byte base module. Used for section sizes.
    // ================================================================

    pub fn read_ms(&mut self) -> Result<u64> {
        let mut value: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            // Two bytes per "module" (LE).
            let lo = self.read_rc()? as u64;
            let hi = self.read_rc()? as u64;
            let module = (hi << 8) | lo;
            let cont = (module & 0x8000) != 0;
            let data = module & 0x7FFF;
            value |= data << shift;
            shift += 15;
            if !cont || shift >= 64 {
                break;
            }
        }
        Ok(value)
    }

    // ================================================================
    // §2.13 — Handle references
    //
    // |CODE(4 bits)|COUNTER(4 bits)|HANDLE_BYTES * COUNTER|
    // ================================================================

    pub fn read_handle(&mut self) -> Result<Handle> {
        let code = self.read_bits(4)? as u8;
        let counter = self.read_bits(4)? as u8;
        let mut value: u64 = 0;
        for _ in 0..counter {
            value = (value << 8) | self.read_rc()? as u64;
        }
        Ok(Handle {
            code,
            counter,
            value,
        })
    }
}

/// A DWG handle reference: 4-bit code + 4-bit byte-length counter + up to
/// 15 bytes of absolute-handle-or-offset payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle {
    pub code: u8,
    pub counter: u8,
    pub value: u64,
}

impl Handle {
    /// Softly-owned (relation code 2) — owner doesn't strictly need the
    /// target; target can exist on its own.
    pub fn is_soft_owner(&self) -> bool {
        self.code == 2
    }

    /// Hard-owned (code 3) — target cannot exist without its owner.
    pub fn is_hard_owner(&self) -> bool {
        self.code == 3
    }

    /// Absolute reference codes (2..=5) that are NOT relative offsets.
    pub fn is_absolute(&self) -> bool {
        (2..=5).contains(&self.code)
    }

    /// Relative offset code set (6, 8, A, C) per spec §2.13.
    pub fn is_offset(&self) -> bool {
        matches!(self.code, 0x6 | 0x8 | 0xA | 0xC)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: pack a sequence of `(value, bit_count)` pairs MSB-first into a
    /// byte vector. Pads the final byte with zeros. Used to construct bit
    /// streams for spec example tests without pen-and-paper arithmetic.
    fn pack_bits(fields: &[(u64, u32)]) -> Vec<u8> {
        let mut acc: u128 = 0;
        let mut nbits: u32 = 0;
        for &(v, k) in fields {
            assert!(k <= 64);
            assert!(nbits + k <= 128, "test helper overflow");
            acc = (acc << k) | (v as u128);
            nbits += k;
        }
        // pad to byte boundary
        let pad = (8 - (nbits % 8)) % 8;
        acc <<= pad as u128;
        let total = nbits + pad;
        let mut out = Vec::with_capacity((total / 8) as usize);
        let mut remaining = total;
        while remaining > 0 {
            remaining -= 8;
            out.push(((acc >> remaining) & 0xFF) as u8);
        }
        out
    }

    // Spec §2.2 — BITSHORT worked example:
    //   Stream: 00 00000001 00000001 10 11 01 00001111 10
    //   5 shorts: 257, 0, 256, 15, 0
    #[test]
    fn spec_2_2_bitshort_example() {
        let stream = pack_bits(&[
            (0b00, 2),        // tag 00 → 16-bit LE
            (0b0000_0001, 8), // LSB
            (0b0000_0001, 8), // MSB → 257
            (0b10, 2),        // → 0
            (0b11, 2),        // → 256
            (0b01, 2),        // tag 01 → 8-bit
            (0b0000_1111, 8), // = 15
            (0b10, 2),        // → 0
        ]);
        let mut c = BitCursor::new(&stream);
        assert_eq!(c.read_bs().unwrap(), 257);
        assert_eq!(c.read_bs().unwrap(), 0);
        assert_eq!(c.read_bs().unwrap(), 256);
        assert_eq!(c.read_bs().unwrap(), 15);
        assert_eq!(c.read_bs().unwrap(), 0);
    }

    // Spec §2.3 — BITLONG worked example:
    //   Stream: 00 00000001 00000001 00000000 00000000 10 01 00001111 10
    //   4 longs: 257, 0, 15, 0
    #[test]
    fn spec_2_3_bitlong_example() {
        let stream = pack_bits(&[
            (0b00, 2),        // tag 00 → 32-bit LE
            (0x01, 8),        // byte 0 (LSB)
            (0x01, 8),        // byte 1
            (0x00, 8),        // byte 2
            (0x00, 8),        // byte 3 (MSB) → 257
            (0b10, 2),        // → 0
            (0b01, 2),        // tag 01 → 8-bit
            (0b0000_1111, 8), // = 15
            (0b10, 2),        // → 0
        ]);
        let mut c = BitCursor::new(&stream);
        assert_eq!(c.read_bl().unwrap(), 257);
        assert_eq!(c.read_bl().unwrap(), 0);
        assert_eq!(c.read_bl().unwrap(), 15);
        assert_eq!(c.read_bl().unwrap(), 0);
    }

    // Spec §2.5 — BITDOUBLE special cases + one full double.
    #[test]
    fn spec_2_5_bitdouble_specials() {
        // Prefix 01 10 00 then 8 LE bytes of 2.0.
        let bytes_2p0 = 2.0f64.to_le_bytes();
        let mut fields: Vec<(u64, u32)> = vec![
            (0b01, 2), // → 1.0
            (0b10, 2), // → 0.0
            (0b00, 2), // → read 8 raw LE bytes
        ];
        for b in bytes_2p0 {
            fields.push((b as u64, 8));
        }
        let stream = pack_bits(&fields);
        let mut c = BitCursor::new(&stream);
        assert_eq!(c.read_bd().unwrap(), 1.0);
        assert_eq!(c.read_bd().unwrap(), 0.0);
        assert_eq!(c.read_bd().unwrap(), 2.0);
    }

    // §2.0 — single bits and 2-bit codes
    #[test]
    fn single_bits() {
        let bytes = [0b1010_0000];
        let mut c = BitCursor::new(&bytes);
        assert!(c.read_b().unwrap());
        assert!(!c.read_b().unwrap());
        assert!(c.read_b().unwrap());
        assert!(!c.read_b().unwrap());
    }

    #[test]
    fn two_bit_codes() {
        // 11 01 10 00 → 3, 1, 2, 0
        let bytes = [0b1101_1000];
        let mut c = BitCursor::new(&bytes);
        assert_eq!(c.read_bb().unwrap(), 3);
        assert_eq!(c.read_bb().unwrap(), 1);
        assert_eq!(c.read_bb().unwrap(), 2);
        assert_eq!(c.read_bb().unwrap(), 0);
    }

    // §2.1 — 3B
    #[test]
    fn read_3b_stops_at_zero() {
        // "0..." → 0
        let bytes = [0b0000_0000];
        assert_eq!(BitCursor::new(&bytes).read_3b().unwrap(), 0);
        // "10..." → 10 → 2
        let bytes = [0b1000_0000];
        assert_eq!(BitCursor::new(&bytes).read_3b().unwrap(), 0b10);
        // "111..." → 7
        let bytes = [0b1110_0000];
        assert_eq!(BitCursor::new(&bytes).read_3b().unwrap(), 0b111);
    }

    // Byte alignment between objects, per §2.14.
    #[test]
    fn align_to_byte_drops_partial() {
        let bytes = [0b1010_0000, 0xAB];
        let mut c = BitCursor::new(&bytes);
        // Consume 3 bits; cursor at bit 3.
        let _ = c.read_bits(3).unwrap();
        c.align_to_byte();
        assert_eq!(c.read_rc().unwrap(), 0xAB);
    }

    #[test]
    fn remaining_bits_tracks_position() {
        let bytes = [0xFF, 0xFF];
        let mut c = BitCursor::new(&bytes);
        assert_eq!(c.remaining_bits(), 16);
        let _ = c.read_bits(5).unwrap();
        assert_eq!(c.remaining_bits(), 11);
    }

    #[test]
    fn exhaustion_returns_error() {
        let bytes = [0xFF];
        let mut c = BitCursor::new(&bytes);
        let _ = c.read_bits(8).unwrap();
        assert!(matches!(c.read_b(), Err(Error::BitsExhausted { .. })));
    }

    // §2.13 — Handle reference
    //
    // Spec example: LAYER 0 has handle 0x0F stored as 5.1.0F
    //   → CODE=5 COUNTER=1 HANDLE=0x0F
    //   → bitstream: 0101 0001 00001111
    #[test]
    fn handle_layer_0_example() {
        let bytes = [0b0101_0001, 0b0000_1111];
        let mut c = BitCursor::new(&bytes);
        let h = c.read_handle().unwrap();
        assert_eq!(h.code, 5);
        assert_eq!(h.counter, 1);
        assert_eq!(h.value, 0x0F);
        assert!(h.is_absolute());
        assert!(!h.is_offset());
    }

    #[test]
    fn raw_types_are_byte_aligned_le() {
        let bytes = [0x01, 0x00, 0x02, 0x00, 0x00, 0x00];
        let mut c = BitCursor::new(&bytes);
        assert_eq!(c.read_rs().unwrap(), 1);
        assert_eq!(c.read_rl().unwrap(), 2);
    }
}
