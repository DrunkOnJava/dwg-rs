//! DWG LZ77 encoder — mirror of [`crate::lz77::decompress`].
//!
//! Encodes byte streams into the R2004+ LZ77 dialect (spec §4.7 with
//! ACadSharp-verified +1 offset adjustments). Output decompresses
//! bit-for-bit via the reader side; property-round-trip tests lock
//! this in.
//!
//! # Strategy
//!
//! Correctness-first "literal-only" encoder: every input is emitted as
//! one initial literal (possibly extended) followed by the terminator
//! opcode `0x11`. No back-references are generated. This produces
//! output ~1x the input size plus a few bytes of framing, which is
//! larger than AutoCAD's compressed output but decompresses
//! bit-for-bit via any conforming DWG reader.
//!
//! Why literal-only instead of greedy matching? The R2004+ opcode
//! table has dense dispatch rules (0x10, 0x12-0x1F, 0x20, 0x21-0x3F,
//! 0x40-0xFF) each with its own offset encoding, literal-count
//! discipline, and ACadSharp-verified +1 corrections. A matcher-based
//! encoder must emit the *correct* opcode family for every
//! (offset, length) pair, or the stream silently desynchronizes on
//! the decoder side. Since the write pipeline only needs
//! correctness — not compression ratio — for round-trip tests and for
//! interoperability, we skip the matcher entirely.
//!
//! A future optimizer can add a second-pass LZ77 greedy matcher that
//! rewrites runs into back-references without touching the stream
//! framing. The `compress` entry point's input/output contract is
//! stable across that evolution.
//!
//! # Literal-length encoding (spec §4.7)
//!
//! | Stream bytes                  | Literal length |
//! |-------------------------------|----------------|
//! | `0x11` (no length byte)       | 0 (empty)      |
//! | `0x01..=0x0F`                 | byte + 3 = 4..=18 |
//! | `0x00, 0x01..=0xFF`           | 0x0F + byte + 3 = 19..=273 |
//! | `0x00, 0x00, 0x01..=0xFF`     | 0x0F + 0xFF + byte + 3 |
//! | ... (each 0x00 adds 0xFF)     | ... up to `usize::MAX / 0xFF` |
//!
//! **Gap:** lengths 1, 2, and 3 are unrepresentable as an initial
//! literal (bytes 0x01..=0x03 mean length 4..=6, not 1..=3). This
//! encoder returns [`Error::Lz77UnencodableLength`] for those
//! inputs. Real-world DWG payloads don't hit the gap because the
//! smallest compressed section has a header prefix well over 3 bytes.

use crate::error::{Error, Result};

/// Terminator opcode per spec §4.7.
const TERMINATOR: u8 = 0x11;

/// Encode `input` into a DWG LZ77 byte stream.
///
/// # Errors
///
/// Returns [`Error::Lz77UnencodableLength`] if `input.len()` is in
/// `1..=3`, which is unrepresentable as an initial literal in the
/// R2004+ LZ77 dialect (see module docs).
pub fn compress(input: &[u8]) -> Result<Vec<u8>> {
    let n = input.len();
    let mut out = Vec::with_capacity(n + 8);
    if n == 0 {
        // Empty stream: first (and only) byte is the terminator.
        out.push(TERMINATOR);
        return Ok(out);
    }
    if (1..=3).contains(&n) {
        return Err(Error::Lz77UnencodableLength(n));
    }
    if (4..=18).contains(&n) {
        // 0x01..=0x0F literal length byte.
        out.push((n - 3) as u8);
    } else {
        // Extended: 0x00 + running total. Target = n - 3 - 0x0F.
        out.push(0x00);
        let mut remaining = (n - 3).saturating_sub(0x0F);
        while remaining > 0xFF {
            out.push(0x00);
            remaining -= 0xFF;
        }
        // Terminating byte must be non-zero.
        if remaining == 0 {
            // Edge case: total = 0x0F + 0 = 0x0F exactly, meaning
            // n - 3 == 0x0F, i.e. n == 18. That's already covered by
            // the (4..=18) branch above, so this path is unreachable.
            // Emit 0x01 as a defensive terminator — decoder reads it
            // as "add 1, length = 0x0F + 1 + 3 = 19". We've
            // overshoot-encoded by 1, but since n != 18 we can't
            // reach here without a bug.
            debug_assert!(
                false,
                "unreachable: extended literal-length zero remainder at n={n}"
            );
            out.push(0x01);
        } else {
            out.push(remaining as u8);
        }
    }
    out.extend_from_slice(input);
    out.push(TERMINATOR);
    Ok(out)
}

/// Convenience: encode with panic on unencodable length. Use only in
/// tests or when the caller has already validated the input size.
pub fn compress_infallible(input: &[u8]) -> Vec<u8> {
    compress(input).expect("unencodable LZ77 literal length")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lz77;

    fn roundtrip(input: &[u8]) {
        let enc = compress(input).expect("encode");
        let dec = lz77::decompress(&enc, None).expect("decode");
        assert_eq!(dec, input, "round-trip failed for {} bytes", input.len());
    }

    #[test]
    fn encode_empty_round_trips() {
        roundtrip(b"");
    }

    #[test]
    fn encode_short_literal_round_trips() {
        // 5 bytes = within 0x01..=0x0F simple range.
        roundtrip(b"ABCDE");
    }

    #[test]
    fn encode_exact_18_byte_boundary() {
        let input: Vec<u8> = (0..18u8).collect();
        roundtrip(&input);
    }

    #[test]
    fn encode_19_byte_extension_boundary() {
        let input: Vec<u8> = (0..19u8).collect();
        roundtrip(&input);
    }

    #[test]
    fn encode_repeating_round_trips() {
        roundtrip(b"AAAAAAAA");
    }

    #[test]
    fn encode_long_literal_round_trips() {
        // 275 bytes = matches the decoder's `extended_literal_length`
        // fixture (0x0F + 0xFF + 0x02 + 3 = 275).
        let input: Vec<u8> = (0..275).map(|i| (i & 0xFF) as u8).collect();
        roundtrip(&input);
    }

    #[test]
    fn encode_very_long_multi_extension() {
        // 1024 bytes — requires multiple 0x00 extension bytes.
        let input: Vec<u8> = (0..1024).map(|i| (i & 0xFF) as u8).collect();
        roundtrip(&input);
    }

    #[test]
    fn encode_rejects_1_byte_input() {
        assert!(matches!(
            compress(&[0x42]),
            Err(Error::Lz77UnencodableLength(1))
        ));
    }

    #[test]
    fn encode_rejects_3_byte_input() {
        assert!(matches!(
            compress(b"ABC"),
            Err(Error::Lz77UnencodableLength(3))
        ));
    }
}
