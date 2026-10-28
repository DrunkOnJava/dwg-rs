//! Bit-level *writer* for DWG primitive types (spec §2) — mirror of
//! [`crate::bitcursor::BitCursor`] for the encode path.
//!
//! # Model
//!
//! Appends bits MSB-first into an internal `Vec<u8>`. Once done, call
//! `into_bytes()` to consume the writer and get the padded buffer.
//! Padding zeros are appended to round out the final byte, matching the
//! "pad to byte boundary" convention DWG uses at the end of objects.
//!
//! # Coverage
//!
//! Primitives: B, BB, 3B, BS, BL, BLL, BD, DD, RC, RS, RL, RD, MC, MS, H,
//! TV (8-bit variant). All emit bit-for-bit-reversible streams against
//! the corresponding `BitCursor::read_*` — the bit-cursor + bit-writer
//! pair is the foundational round-trip invariant for Phase H write
//! support.

use crate::error::{Error, Result};

/// A MSB-first bit buffer.
#[derive(Debug, Default, Clone)]
pub struct BitWriter {
    bytes: Vec<u8>,
    // Position within the last byte where the NEXT write will land.
    // 0 means "no partial byte in progress — next write pushes a new byte";
    // 1..=7 means "1..=7 bits already used in the last (already-pushed) byte".
    bit_pos: u8,
}

impl BitWriter {
    /// Creates an empty writer positioned at bit 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Total bits written so far.
    ///
    /// At a whole-byte boundary (no partial byte in progress),
    /// `bit_pos == 0` and the count is `bytes.len() * 8`. Mid-byte,
    /// the last element of `bytes` already exists and `bit_pos` is
    /// the 1..=7 bits-used offset into it.
    pub fn position_bits(&self) -> usize {
        if self.bytes.is_empty() {
            0
        } else if self.bit_pos == 0 {
            self.bytes.len() * 8
        } else {
            (self.bytes.len() - 1) * 8 + self.bit_pos as usize
        }
    }

    /// Pad to the next byte boundary with zeros (matches spec §2.14 "B*").
    pub fn align_to_byte(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
        }
    }

    /// Finalize and return the encoded bytes. Any pending bits are padded
    /// to a byte boundary.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// View the current contents (testing helper).
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    fn write_bits(&mut self, value: u64, n: usize) {
        debug_assert!(n <= 64);
        for i in (0..n).rev() {
            let bit = ((value >> i) & 1) as u8;
            if self.bit_pos == 0 {
                self.bytes.push(0);
            }
            let byte_idx = self.bytes.len() - 1;
            let shift = 7 - self.bit_pos;
            self.bytes[byte_idx] |= bit << shift;
            self.bit_pos += 1;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
            }
        }
    }

    /// Writes a `B` (single bit).
    pub fn write_b(&mut self, v: bool) {
        self.write_bits(if v { 1 } else { 0 }, 1);
    }

    /// Writes a `BB` (2-bit code) from the low two bits of `v`.
    pub fn write_bb(&mut self, v: u8) {
        debug_assert!(v <= 3);
        self.write_bits(v as u64, 2);
    }

    /// 3B per spec §2.1 (as measured by [`crate::bitcursor::BitCursor::read_3b`]).
    ///
    /// Three raw bits, MSB first, so every value `0..=7` is
    /// representable in a fixed width. Returns [`Error::Invalid3B`] for
    /// anything larger — this is the preferred fallible API.
    /// [`write_3b`](Self::write_3b) is kept as a convenience wrapper
    /// that `.expect`s the result for call sites that have statically
    /// validated the input.
    pub fn try_write_3b(&mut self, v: u8) -> Result<()> {
        if v > 7 {
            return Err(Error::Invalid3B { value: v });
        }
        self.write_bits(v as u64, 3);
        Ok(())
    }

    /// Convenience wrapper around [`try_write_3b`](Self::try_write_3b)
    /// that panics on invalid input. Prefer the fallible form at any
    /// call site where the value is not statically in `0..=7`.
    ///
    /// # Panics
    ///
    /// Panics with [`Error::Invalid3B`] formatting when `v > 7`.
    pub fn write_3b(&mut self, v: u8) {
        self.try_write_3b(v).expect("invalid 3B value");
    }

    /// BS (bitshort) — 00=LE short, 01=u8, 10=0, 11=256. Choose the
    /// shortest encoding that fits.
    pub fn write_bs(&mut self, v: i16) {
        match v {
            0 => self.write_bb(0b10),
            256 => self.write_bb(0b11),
            x if (0..256).contains(&(x as i32)) => {
                self.write_bb(0b01);
                self.write_bits(x as u64 & 0xFF, 8);
            }
            x => {
                self.write_bb(0b00);
                let w = x as u16;
                self.write_bits((w & 0xFF) as u64, 8);
                self.write_bits((w >> 8) as u64, 8);
            }
        }
    }

    /// Writes a `BS` from an unsigned value; bit-identical to writing it reinterpreted as `i16`.
    pub fn write_bs_u(&mut self, v: u16) {
        self.write_bs(v as i16);
    }

    /// BL — 00=LE long, 01=u8, 10=0, 11=reserved.
    pub fn write_bl(&mut self, v: i32) {
        match v {
            0 => self.write_bb(0b10),
            x if (0..256).contains(&x) => {
                self.write_bb(0b01);
                self.write_bits(x as u64 & 0xFF, 8);
            }
            x => {
                self.write_bb(0b00);
                let w = x as u32;
                for i in 0..4 {
                    self.write_bits(((w >> (i * 8)) & 0xFF) as u64, 8);
                }
            }
        }
    }

    /// Writes a `BL` from an unsigned value; bit-identical to writing it reinterpreted as `i32`.
    pub fn write_bl_u(&mut self, v: u32) {
        self.write_bl(v as i32);
    }

    /// BLL (R24+) per spec §2.4: a 3B byte count followed by that many
    /// little-endian data bytes.
    ///
    /// The count is three raw bits, so it spans `0..=7` and the writer
    /// emits the exact number of bytes the value needs:
    ///
    /// | range of `v`                     | length | total bits (incl. prefix) |
    /// |----------------------------------|--------|---------------------------|
    /// | `0`                              | 0      | 3                         |
    /// | `1..=0xFF`                       | 1      | 11                        |
    /// | `0x100..=0xFFFF`                 | 2      | 19                        |
    /// | …                                | …      | …                         |
    /// | `0x1_0000_0000_0000..=(1<<56)-1` | 7      | 59                        |
    ///
    /// Values requiring more than 56 bits (`v >= 1 << 56`) return
    /// [`Error::BllOverflow`] — the encoding cannot represent them.
    pub fn write_bll(&mut self, v: u64) -> Result<()> {
        let byte_count = if v == 0 {
            0
        } else {
            ((64 - v.leading_zeros()) as usize).div_ceil(8)
        };
        let len: u8 = match byte_count {
            0..=7 => byte_count as u8,
            _ => return Err(Error::BllOverflow { value: v }),
        };
        self.try_write_3b(len)?;
        for i in 0..len as usize {
            self.write_bits((v >> (i * 8)) & 0xFF, 8);
        }
        Ok(())
    }

    /// BD — 00=IEEE 754 double, 01=1.0, 10=0.0, 11=reserved.
    pub fn write_bd(&mut self, v: f64) {
        if v == 1.0 {
            self.write_bb(0b01);
        } else if v == 0.0 {
            self.write_bb(0b10);
        } else {
            self.write_bb(0b00);
            let bs = v.to_le_bytes();
            for b in bs {
                self.write_bits(b as u64, 8);
            }
        }
    }

    /// DD — bitdouble with default. Emits the exact-default form when possible,
    /// otherwise falls back to the full raw double form.
    pub fn write_dd(&mut self, default: f64, v: f64) {
        if v.to_bits() == default.to_bits() {
            self.write_bb(0b00);
        } else {
            self.write_bb(0b11);
            self.write_rd(v);
        }
    }

    /// Writes an `RC` (raw 8-bit char).
    pub fn write_rc(&mut self, v: u8) {
        self.write_bits(v as u64, 8);
    }

    /// Writes an `RS` (raw little-endian 16-bit short).
    pub fn write_rs(&mut self, v: i16) {
        let w = v as u16;
        self.write_bits((w & 0xFF) as u64, 8);
        self.write_bits((w >> 8) as u64, 8);
    }

    /// Writes an `RL` (raw little-endian 32-bit long).
    pub fn write_rl(&mut self, v: u32) {
        for i in 0..4 {
            self.write_bits(((v >> (i * 8)) & 0xFF) as u64, 8);
        }
    }

    /// Writes an `RD` (raw little-endian IEEE-754 double).
    pub fn write_rd(&mut self, v: f64) {
        for b in v.to_le_bytes() {
            self.write_bits(b as u64, 8);
        }
    }

    /// MC (modular char, signed) per spec §2.6.
    ///
    /// Uses [`i64::unsigned_abs`] so that `v == i64::MIN` (whose
    /// two's-complement negation is unrepresentable as an `i64`) is
    /// still encoded correctly instead of overflowing.
    pub fn write_mc(&mut self, v: i64) -> Result<()> {
        let (abs, negate) = if v < 0 {
            (v.unsigned_abs(), true)
        } else {
            (v as u64, false)
        };
        // Emit 7 bits per continuation byte; final byte carries 6 data bits +
        // negate flag (bit 6) + cont=0.
        if abs < 0x40 {
            let b = if negate {
                0x40 | (abs as u8)
            } else {
                abs as u8
            };
            self.write_rc(b);
            return Ok(());
        }
        // Multi-byte: split into 7-bit limbs.
        let mut limbs = Vec::new();
        let mut x = abs;
        while x != 0 {
            limbs.push((x & 0x7F) as u8);
            x >>= 7;
        }
        for (i, limb) in limbs.iter().enumerate() {
            let is_last = i == limbs.len() - 1;
            if is_last {
                // Terminating byte — only 6 data bits plus negate flag.
                // If limb has its 0x40 bit set, we need one more byte.
                if (*limb & 0x40) == 0 {
                    let mut b = *limb & 0x3F;
                    if negate {
                        b |= 0x40;
                    }
                    self.write_rc(b);
                } else {
                    // Emit this limb as continuation, then a zero terminator
                    // carrying only the sign.
                    self.write_rc(0x80 | limb);
                    self.write_rc(if negate { 0x40 } else { 0x00 });
                }
            } else {
                self.write_rc(0x80 | limb);
            }
        }
        Ok(())
    }

    /// MS (modular short, unsigned) per spec §2.7 — 15 data bits per
    /// 16-bit LE module, bit 15 is continuation.
    pub fn write_ms(&mut self, v: u64) {
        if v == 0 {
            self.write_bits(0, 8);
            self.write_bits(0, 8);
            return;
        }
        let mut x = v;
        let mut modules: Vec<u16> = Vec::new();
        while x != 0 {
            modules.push((x & 0x7FFF) as u16);
            x >>= 15;
        }
        for (i, m) in modules.iter().enumerate() {
            let is_last = i == modules.len() - 1;
            let mut module = *m;
            if !is_last {
                module |= 0x8000;
            }
            self.write_bits((module & 0xFF) as u64, 8);
            self.write_bits((module >> 8) as u64, 8);
        }
    }

    /// Handle: 4-bit code, 4-bit counter, counter bytes payload.
    pub fn write_handle(&mut self, code: u8, value: u64) {
        debug_assert!(code <= 0x0F);
        let counter = if value == 0 {
            0
        } else {
            ((64 - value.leading_zeros()) as usize).div_ceil(8)
        };
        debug_assert!(counter <= 0x0F);
        self.write_bits(code as u64, 4);
        self.write_bits(counter as u64, 4);
        for i in (0..counter).rev() {
            self.write_bits((value >> (i * 8)) & 0xFF, 8);
        }
    }
}
