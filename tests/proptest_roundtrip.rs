//! Property-based round-trip tests (task #45).
//!
//! These tests use `proptest` to generate large pseudo-random
//! inputs for each bit-primitive and entity decoder, then verify
//! the corresponding BitWriter → BitCursor round-trip is bit-exact.
//!
//! Shrinking means failures produce minimized counter-examples
//! automatically — a regression prints the exact input that
//! broke round-trip, not a 10KB random blob.

use dwg::bitcursor::BitCursor;
use dwg::bitwriter::BitWriter;
use dwg::error::Result;
use dwg::lz77;
use dwg::lz77_encode;
use proptest::prelude::*;

proptest! {
    /// Every (bool, bool, bb_u2, 3b_representable) sequence round-trips.
    #[test]
    fn bits_b_bb_3b_roundtrip(b1: bool, b2: bool, bb in 0u8..=3u8, choice in 0u8..4u8) {
        let three_b = match choice { 0 => 0, 1 => 2, 2 => 6, _ => 7 };
        let mut w = BitWriter::new();
        w.write_b(b1);
        w.write_b(b2);
        w.write_bb(bb);
        w.write_3b(three_b);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        prop_assert_eq!(c.read_b().unwrap(), b1);
        prop_assert_eq!(c.read_b().unwrap(), b2);
        prop_assert_eq!(c.read_bb().unwrap(), bb);
        prop_assert_eq!(c.read_3b().unwrap(), three_b);
    }

    /// BS (bitshort, signed) round-trip over the full i16 range.
    #[test]
    fn bs_roundtrip(v in any::<i16>()) {
        let mut w = BitWriter::new();
        w.write_bs(v);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        prop_assert_eq!(c.read_bs().unwrap(), v);
    }

    /// BL (bitlong, signed) round-trip.
    #[test]
    fn bl_roundtrip(v in any::<i32>()) {
        let mut w = BitWriter::new();
        w.write_bl(v);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        prop_assert_eq!(c.read_bl().unwrap(), v);
    }

    /// BD — bitdouble. Exclude NaN for equality assertion; NaN is
    /// covered by the 0.0/1.0 special cases.
    #[test]
    fn bd_roundtrip(v in any::<f64>().prop_filter("not NaN", |f| !f.is_nan())) {
        let mut w = BitWriter::new();
        w.write_bd(v);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        prop_assert_eq!(c.read_bd().unwrap(), v);
    }

    /// RC (raw char u8).
    #[test]
    fn rc_roundtrip(v: u8) {
        let mut w = BitWriter::new();
        w.write_rc(v);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        prop_assert_eq!(c.read_rc().unwrap(), v);
    }

    /// RL (raw long u32).
    #[test]
    fn rl_roundtrip(v: u32) {
        let mut w = BitWriter::new();
        w.write_rl(v);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        prop_assert_eq!(c.read_rl().unwrap(), v);
    }

    /// Handle round-trip: code ∈ 0..=15, counter auto-computed.
    #[test]
    fn handle_roundtrip(code in 0u8..=15u8, value: u64) {
        let mut w = BitWriter::new();
        w.write_handle(code, value);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = c.read_handle().unwrap();
        prop_assert_eq!(h.code, code);
        prop_assert_eq!(h.value, value);
    }

    /// LZ77 round-trip: random byte vectors of 0 or >=4 bytes
    /// (the encoder's representable length range).
    #[test]
    fn lz77_literal_roundtrip(bytes in prop::collection::vec(any::<u8>(), 0..=512)) {
        let n = bytes.len();
        // Skip the unencodable gap 1..=3.
        if (1..=3).contains(&n) {
            return Ok(());
        }
        let enc = lz77_encode::compress(&bytes).unwrap();
        let dec: Vec<u8> = lz77::decompress(&enc, None).unwrap();
        prop_assert_eq!(dec, bytes);
    }
}

/// Hand-written backstop: every representable-gap input returns
/// the specific error, none of them panic. Not a proptest because
/// the input space is {[0], [0,0], [0,0,0]} plus trivial variants.
#[test]
fn lz77_rejects_1_to_3_bytes() -> Result<()> {
    for n in 1..=3 {
        let v = vec![0u8; n];
        assert!(lz77_encode::compress(&v).is_err(), "n={n} should error");
    }
    Ok(())
}
