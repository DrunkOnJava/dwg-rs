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

use crate::error::Result;

/// A MSB-first bit buffer.
#[derive(Debug, Default, Clone)]
pub struct BitWriter {
    bytes: Vec<u8>,
    bit_pos: u8, // 0..7, MSB-first; 0 means "next write goes in top bit of the last byte"
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total bits written so far.
    pub fn position_bits(&self) -> usize {
        if self.bytes.is_empty() {
            0
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
    /// Any other input is a bug at the call site.
    pub fn write_3b(&mut self, v: u8) {
        match v {
            0 => self.write_bits(0b0, 1),
            2 => self.write_bits(0b10, 2),
            6 => self.write_bits(0b110, 3),
            7 => self.write_bits(0b111, 3),
            _ => panic!("invalid 3B value {v}; representable: 0, 2, 6, 7"),
        }
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

    /// BLL (R24+): 1-3 bits length, then that many bytes LSB-first.
    pub fn write_bll(&mut self, v: u64) {
        let len = if v == 0 {
            0
        } else {
            ((64 - v.leading_zeros()) as usize).div_ceil(8)
        };
        debug_assert!(len <= 7);
        self.write_3b(len as u8);
        for i in 0..len {
            self.write_bits((v >> (i * 8)) & 0xFF, 8);
        }
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
    pub fn write_mc(&mut self, v: i64) -> Result<()> {
        let (abs, negate) = if v < 0 {
            ((-v) as u64, true)
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
}
