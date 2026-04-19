//! R2007 (AC1021) layout support — spec §5.
//!
//! R2007 is the single odd release in the R2004-family: instead of
//! the clean XOR-then-LZ77 pipeline used by R2004 / R2010 / R2013 /
//! R2018, R2007 wraps its section data in a two-layer obfuscation
//! called **Sec_Mask**. The first layer is a byte-stream XOR against
//! a 75-byte rolling mask; the second is a bit-level rotation
//! applied to 7-byte windows.
//!
//! Every other R2004-family version uses the straightforward
//! [`crate::cipher::xor_in_place`] 0x6C-byte XOR for the file
//! header, and raw LZ77 for section payloads — no Sec_Mask involved.
//! R2007 layered Sec_Mask on top of that for *both* the header and
//! section bodies, for reasons Autodesk never explained publicly.
//!
//! # Current state
//!
//! This module decodes and re-encodes the **first layer** (byte XOR
//! with the 75-byte Sec_Mask seed). The **second layer** (bit-level
//! rotation in 7-byte windows) is scaffolded but not wired into
//! [`crate::reader::DwgFile`]; R2007 files currently return early
//! with an error from the reader's section-map parser.
//!
//! # Spec references
//!
//! - §5.1: system section layout (R2007)
//! - §5.2: two-layer Sec_Mask (encrypted bit-stream bookkeeping)
//! - §5.3: Sec_Mask seed derivation from section handle
//! - §5.4: per-section header (shifted byte offsets from R2004)

/// The 75-byte Sec_Mask seed produced by mixing the global section
/// parameters with a per-section handle. Real R2007 files seed this
/// from data in the encrypted file-open header.
pub const SEC_MASK_LEN: usize = 75;

/// First-layer Sec_Mask: XOR each byte of `data` with a rolling
/// pattern derived from `seed`. Symmetric — calling twice restores
/// the original bytes.
///
/// The rolling pattern is a simple LCG: `state = state * 0x343FD + 0x269EC3`,
/// output byte = high byte of `state`. Different seed values produce
/// independent mask sequences; R2007 derives the seed from the
/// section number during section-map parsing.
pub fn xor_layer_1(data: &mut [u8], seed: u32) {
    let mut state: u32 = seed;
    for byte in data.iter_mut() {
        state = state.wrapping_mul(0x0003_43FD);
        state = state.wrapping_add(0x0026_9EC3);
        let mask = ((state >> 0x10) & 0xFF) as u8;
        *byte ^= mask;
    }
}

/// Second-layer Sec_Mask: bit-level rotation of 7-byte windows. For
/// each window, rotate the 56-bit value left by `k` bits where
/// `k = (offset / 7) % 56`.
///
/// This is the *partial* implementation — the exact derivation of
/// per-window rotation amounts requires the full Sec_Mask
/// bookkeeping from spec §5.2 which tracks cumulative bit offsets
/// through the section. A working prototype for simple section
/// payloads is here; the full bookkeeping lives in R2007
/// [`crate::section_map`] once implemented.
pub fn rotate_layer_2(data: &mut [u8], bit_offset: usize) {
    let mut offset = bit_offset;
    for window in data.chunks_exact_mut(7) {
        let shift = (offset / 7) % 56;
        let mut value: u64 = 0;
        for (i, &b) in window.iter().enumerate() {
            value |= (b as u64) << (i * 8);
        }
        // rotate_left on u64 gives us 0..=63; we mask to 56-bit.
        let rotated = (value << shift | value >> (56 - shift)) & ((1u64 << 56) - 1);
        for (i, dst) in window.iter_mut().enumerate() {
            *dst = ((rotated >> (i * 8)) & 0xFF) as u8;
        }
        offset += 56;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor_layer_1_is_involution() {
        let original = b"Hello, R2007 Sec_Mask!".to_vec();
        let mut buf = original.clone();
        xor_layer_1(&mut buf, 0xDEADBEEF);
        assert_ne!(buf, original);
        xor_layer_1(&mut buf, 0xDEADBEEF);
        assert_eq!(buf, original);
    }

    #[test]
    fn xor_layer_1_different_seeds_different_outputs() {
        let base = b"TESTTESTTESTTEST".to_vec();
        let mut a = base.clone();
        let mut b = base.clone();
        xor_layer_1(&mut a, 0x1234);
        xor_layer_1(&mut b, 0x5678);
        assert_ne!(a, b);
    }

    #[test]
    fn xor_layer_1_zero_seed_reproduces_system_header_mask() {
        // Seed of 1 with 108 iterations should recreate the R2004
        // magic sequence (by definition — same LCG, same seed).
        let mut buf = [0u8; 108];
        xor_layer_1(&mut buf, 1);
        // The magic sequence is XOR'd *onto* zeros, so we should get
        // the magic sequence back out.
        let magic = crate::cipher::magic_sequence();
        assert_eq!(&buf[..], &magic[..]);
    }

    #[test]
    fn rotate_layer_2_handles_single_window() {
        let mut buf = vec![0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
        let original = buf.clone();
        rotate_layer_2(&mut buf, 0);
        // Zero offset ⇒ shift by 0 ⇒ bytes unchanged.
        assert_eq!(buf, original);
    }

    #[test]
    fn rotate_layer_2_partial_window_passthrough() {
        let mut buf = vec![0xAAu8; 10]; // 1 full 7-byte window + 3 leftover
        let original = buf.clone();
        rotate_layer_2(&mut buf, 0);
        // First 7 bytes: shift-0 leaves them alone.
        assert_eq!(&buf[..7], &original[..7]);
        // Last 3 are left alone (incomplete window).
        assert_eq!(&buf[7..], &original[7..]);
    }
}
