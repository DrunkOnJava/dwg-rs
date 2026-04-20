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
//! Primitives: B, BB, 3B, BS, BL, BLL, BD, RC, RS, RL, RD, MC, MS, H,
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

    pub fn write_b(&mut self, v: bool) {
        self.write_bits(if v { 1 } else { 0 }, 1);
    }

    pub fn write_bb(&mut self, v: u8) {
        debug_assert!(v <= 3);
        self.write_bits(v as u64, 2);
    }

    /// 3B per spec §2.1 (as interpreted by [`crate::bitcursor::BitCursor::read_3b`]).
    ///
    /// The reader accumulates bits MSB-first, shifting into a `u8`,
    /// stopping early on a 0-bit or after 3 bits. That produces a
    /// closed set of four representable values:
    ///
    /// | bits | value |
    /// |------|-------|
    /// | `0`   | 0 |
    /// | `10`  | 2 |
    /// | `110` | 6 |
    /// | `111` | 7 |
    ///
    /// Returns [`Error::Invalid3B`] for any value outside that set —
    /// this is the preferred fallible API. [`write_3b`](Self::write_3b)
    /// is kept as a convenience wrapper that `.expect`s the result for
    /// call sites that have statically validated the input.
    pub fn try_write_3b(&mut self, v: u8) -> Result<()> {
        match v {
            0 => self.write_bits(0b0, 1),
            2 => self.write_bits(0b10, 2),
            6 => self.write_bits(0b110, 3),
            7 => self.write_bits(0b111, 3),
            _ => return Err(Error::Invalid3B { value: v }),
        }
        Ok(())
    }

    /// Convenience wrapper around [`try_write_3b`](Self::try_write_3b)
    /// that panics on invalid input. Prefer the fallible form at any
    /// call site where the value is not statically one of `{0, 2, 6, 7}`.
    ///
    /// # Panics
    ///
    /// Panics with [`Error::Invalid3B`] formatting when `v` is outside
    /// the representable set.
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

    pub fn write_bl_u(&mut self, v: u32) {
        self.write_bl(v as i32);
    }

    /// BLL (R24+) per spec §2.4: a 3B prefix-coded length followed by
    /// that many little-endian data bytes.
    ///
    /// Because 3B can only represent lengths `{0, 2, 6, 7}` (spec §2.1,
    /// prefix code), the writer must choose the smallest representable
    /// length that fits the value. Encoding layout:
    ///
    /// | range of `v`              | length | payload bytes | total bits (incl. prefix) |
    /// |---------------------------|--------|---------------|---------------------------|
    /// | `0`                       | 0      | 0             | 1                         |
    /// | `1..=0xFFFF`              | 2      | 2             | 18                        |
    /// | `0x10000..=0xFFFF_FFFF_FFFF` | 6    | 6             | 51                        |
    /// | `0x1_0000_0000_0000..=(1<<56)-1` | 7 | 7           | 59                        |
    ///
    /// Values requiring more than 56 bits (`v >= 1 << 56`) return
    /// [`Error::BllOverflow`] — the encoding cannot represent them.
    pub fn write_bll(&mut self, v: u64) -> Result<()> {
        let byte_count = if v == 0 {
            0
        } else {
            ((64 - v.leading_zeros()) as usize).div_ceil(8)
        };
        // Round up to the nearest representable prefix-coded length.
        let len: u8 = match byte_count {
            0 => 0,
            1 | 2 => 2,
            3..=6 => 6,
            7 => 7,
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

    pub fn write_rc(&mut self, v: u8) {
        self.write_bits(v as u64, 8);
    }

    pub fn write_rs(&mut self, v: i16) {
        let w = v as u16;
        self.write_bits((w & 0xFF) as u64, 8);
        self.write_bits((w >> 8) as u64, 8);
    }

    pub fn write_rl(&mut self, v: u32) {
        for i in 0..4 {
            self.write_bits(((v >> (i * 8)) & 0xFF) as u64, 8);
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitcursor::BitCursor;

    /// Round-trip: write a set of primitives, read them back, assert
    /// equality.
    #[test]
    fn roundtrip_b_bb_3b() {
        let mut w = BitWriter::new();
        w.write_b(true);
        w.write_b(false);
        w.write_bb(2);
        w.write_3b(6);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        assert!(c.read_b().unwrap());
        assert!(!c.read_b().unwrap());
        assert_eq!(c.read_bb().unwrap(), 2);
        assert_eq!(c.read_3b().unwrap(), 6);
    }

    #[test]
    fn roundtrip_bs_specials() {
        for v in [0i16, 256, 1, -1, 100, -100, 12345, -12345] {
            let mut w = BitWriter::new();
            w.write_bs(v);
            let bytes = w.into_bytes();
            let mut c = BitCursor::new(&bytes);
            assert_eq!(c.read_bs().unwrap(), v, "v={v}");
        }
    }

    #[test]
    fn roundtrip_bl_specials() {
        for v in [0i32, 1, -1, 255, 256, 100_000, -100_000, i32::MAX, i32::MIN] {
            let mut w = BitWriter::new();
            w.write_bl(v);
            let bytes = w.into_bytes();
            let mut c = BitCursor::new(&bytes);
            assert_eq!(c.read_bl().unwrap(), v, "v={v}");
        }
    }

    #[test]
    fn roundtrip_bd_specials() {
        for v in [0.0f64, 1.0, 2.5, -42.125, 1e100, -1e-100] {
            let mut w = BitWriter::new();
            w.write_bd(v);
            let bytes = w.into_bytes();
            let mut c = BitCursor::new(&bytes);
            let got = c.read_bd().unwrap();
            if v == 0.0 {
                assert_eq!(got, 0.0);
            } else if v == 1.0 {
                assert_eq!(got, 1.0);
            } else {
                assert!((got - v).abs() < 1e-10, "v={v} got={got}");
            }
        }
    }

    #[test]
    fn roundtrip_rc_rs_rl_rd() {
        let mut w = BitWriter::new();
        w.write_rc(0xAB);
        w.write_rs(-12345);
        w.write_rl(0xDEAD_BEEF);
        w.write_rd(42.5);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        assert_eq!(c.read_rc().unwrap(), 0xAB);
        assert_eq!(c.read_rs().unwrap(), -12345);
        assert_eq!(c.read_rl().unwrap(), 0xDEAD_BEEF);
        assert_eq!(c.read_rd().unwrap(), 42.5);
    }

    #[test]
    fn roundtrip_handle() {
        let mut w = BitWriter::new();
        w.write_handle(5, 0x0F);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = c.read_handle().unwrap();
        assert_eq!(h.code, 5);
        assert_eq!(h.counter, 1);
        assert_eq!(h.value, 0x0F);
    }

    // ================================================================
    // Regression tests for correctness bugs surfaced by the external
    // release-readiness audit.
    // ================================================================

    /// `position_bits` must return `N * 8` after exactly `N` whole
    /// bytes have been written, not `(N - 1) * 8`.
    #[test]
    fn position_bits_after_whole_bytes() {
        let mut w = BitWriter::new();
        assert_eq!(w.position_bits(), 0);
        w.write_bits(0xAB, 8);
        assert_eq!(w.position_bits(), 8, "after 1 byte");
        w.write_bits(0xCD, 8);
        assert_eq!(w.position_bits(), 16, "after 2 bytes");
        w.write_bits(0x1, 1);
        assert_eq!(w.position_bits(), 17, "after 2 bytes + 1 bit");
        // Fill out the current byte.
        w.write_bits(0, 7);
        assert_eq!(w.position_bits(), 24, "after 3 bytes (boundary)");
    }

    /// `write_mc` must not overflow on `i64::MIN`. The two's-complement
    /// negation `-i64::MIN` is unrepresentable as `i64`; the encoder
    /// must use `unsigned_abs` instead.
    #[test]
    fn write_mc_i64_min_does_not_overflow() {
        let mut w = BitWriter::new();
        w.write_mc(i64::MIN).unwrap();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        // Round-trip must preserve the value.
        assert_eq!(c.read_mc().unwrap(), i64::MIN);
    }

    #[test]
    fn write_mc_i64_max_roundtrips() {
        let mut w = BitWriter::new();
        w.write_mc(i64::MAX).unwrap();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        assert_eq!(c.read_mc().unwrap(), i64::MAX);
    }

    /// `try_write_3b` returns `Err(Error::Invalid3B)` for values
    /// outside `{0, 2, 6, 7}` instead of panicking.
    #[test]
    fn try_write_3b_rejects_invalid_values() {
        for bad in [1u8, 3, 4, 5, 8, 15, 255] {
            let mut w = BitWriter::new();
            let err = w.try_write_3b(bad).unwrap_err();
            assert!(
                matches!(err, crate::error::Error::Invalid3B { value } if value == bad),
                "bad={bad} err={err:?}"
            );
        }
    }

    #[test]
    fn try_write_3b_accepts_representable_values() {
        for good in [0u8, 2, 6, 7] {
            let mut w = BitWriter::new();
            assert!(w.try_write_3b(good).is_ok(), "good={good}");
        }
    }

    /// BLL must round-trip across the full byte-length spectrum the
    /// prefix code can represent. The encoder upgrades counts of
    /// `{1, 3, 4, 5, 7, 8}` bytes to the next larger representable
    /// length, padding the value with zeros — the reader reconstructs
    /// the original integer either way.
    #[test]
    fn bll_roundtrip_spans_representable_range() {
        let values: [u64; 11] = [
            0,
            1,
            0xFF,
            0x100,
            0xFFFF,
            0x10000,
            0xFFFF_FFFF,
            0xFFFF_FFFF_FFFF,
            1u64 << 40,
            1u64 << 48,
            (1u64 << 56) - 1,
        ];
        for v in values {
            let mut w = BitWriter::new();
            w.write_bll(v)
                .unwrap_or_else(|e| panic!("write_bll({v}): {e}"));
            let bytes = w.into_bytes();
            let mut c = BitCursor::new(&bytes);
            let got = c.read_bll().unwrap();
            assert_eq!(got, v, "roundtrip mismatch for v={v:#018X}");
        }
    }

    #[test]
    fn bll_rejects_values_requiring_more_than_56_bits() {
        let mut w = BitWriter::new();
        let res = w.write_bll(1u64 << 56);
        assert!(
            matches!(res, Err(crate::error::Error::BllOverflow { .. })),
            "res={res:?}"
        );
    }
}
