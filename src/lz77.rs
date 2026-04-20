//! DWG LZ77 decompressor (spec §4.7).
//!
//! The R2004+ section pages compress their payloads with a custom LZ77
//! dialect. It differs from zlib in three meaningful ways:
//!
//! 1. **0x11 terminates** the stream (instead of relying on external length).
//! 2. **Literal lengths** use a 0x00-byte run-length extension: each 0x00 byte
//!    adds 0xFF to a running total, and the first non-zero byte terminates
//!    and adds its own value plus 3.
//! 3. **Offset encoding depends on opcode class**:
//!    - `0x10` / `0x12-0x1F` add `0x3FFF` to the offset (long back-reference)
//!    - `0x20` / `0x21-0x3F` use offset as-is
//!    - `0x40-0xFF` pack 2 bits of offset into opcode1's low nibble and
//!      encode a 0..3 literal count directly in bits 0-1
//!
//! RLE-style copies (where `comp_bytes > comp_offset`) are supported — the
//! output buffer is re-read byte-by-byte as it grows, so a forward copy
//! wraps and repeats the tail of the window.

use crate::error::{Error, Result};

/// Safety limits for LZ77 decompression.
///
/// The DWG LZ77 stream format has no intrinsic size declaration in the
/// compressed prefix — a truncated or adversarial stream can in
/// principle declare arbitrarily large back-reference runs or literal
/// lengths via the 0x00-extension pattern. The decompressor treats
/// these as bounded-by-input, but the OUTPUT buffer still needs an
/// explicit cap to prevent a decompression-bomb style DoS.
///
/// `Default` picks conservative values that accommodate real-world
/// DWG section sizes while rejecting pathological inputs:
///
/// - `max_output_bytes`: 256 MiB — larger than any real DWG section
///   this author has observed, but far below modern RAM.
/// - `max_backref_len`: 1 MiB — real back-reference copies are
///   typically tens to thousands of bytes; 1 MiB catches obviously
///   malformed copy lengths without clipping legitimate runs.
#[derive(Debug, Clone, Copy)]
pub struct DecompressLimits {
    /// Hard ceiling on the returned `Vec<u8>` length. When a literal
    /// run or back-reference copy would exceed this, decompression
    /// errors with [`Error::Lz77OutputLimitExceeded`].
    pub max_output_bytes: usize,
    /// Ceiling on any single back-reference copy length.
    pub max_backref_len: usize,
}

impl Default for DecompressLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 256 * 1024 * 1024,
            max_backref_len: 1024 * 1024,
        }
    }
}

impl DecompressLimits {
    /// Permissive profile — used by tests that intentionally construct
    /// streams larger than the conservative default. Not recommended
    /// for production.
    pub fn permissive() -> Self {
        Self {
            max_output_bytes: 4 * 1024 * 1024 * 1024,
            max_backref_len: 64 * 1024 * 1024,
        }
    }
}

/// Decompress a DWG LZ77 stream using the default [`DecompressLimits`].
///
/// The stream runs until opcode `0x11` is encountered. `expected_size`
/// is a capacity hint, used to pre-size the output buffer but clamped
/// to [`DecompressLimits::max_output_bytes`] so an adversarial caller
/// cannot trigger a giant up-front allocation by lying about the
/// expected size.
///
/// Errors:
/// - [`Error::Lz77Truncated`] — input ran out before a terminator
/// - [`Error::Lz77InvalidOffset`] — back-reference pointed before the
///   start of the output buffer
/// - [`Error::Lz77InvalidOpcode`] — an opcode in the reserved range
///   `0x00..=0x0F` appeared where an opcode was expected
/// - [`Error::Lz77OutputLimitExceeded`] — decompressed output would
///   exceed [`DecompressLimits::max_output_bytes`] (DoS defense)
/// - [`Error::Lz77BackrefTooLong`] — a single copy would exceed
///   [`DecompressLimits::max_backref_len`]
pub fn decompress(input: &[u8], expected_size: Option<usize>) -> Result<Vec<u8>> {
    decompress_with_limits(input, expected_size, DecompressLimits::default())
}

/// Decompress a DWG LZ77 stream with caller-supplied safety limits.
/// See [`decompress`] for the semantics and [`DecompressLimits`] for
/// the available knobs.
pub fn decompress_with_limits(
    input: &[u8],
    expected_size: Option<usize>,
    limits: DecompressLimits,
) -> Result<Vec<u8>> {
    let mut r = Lz77Reader::new(input);
    // Clamp the capacity hint so a malicious or corrupted caller can't
    // trigger a giant up-front allocation by claiming a huge expected
    // size. The bound still grows lazily if needed up to
    // `limits.max_output_bytes`.
    let cap = expected_size.unwrap_or(4096).min(limits.max_output_bytes);
    let mut out: Vec<u8> = Vec::with_capacity(cap);

    // Initial literal length — always present. If the first byte has its
    // high nibble set, treat as "length 0 + the byte is the first opcode1".
    match read_literal_length_or_peek_opcode(&mut r)? {
        Some(n) => copy_literal(&mut r, &mut out, n, &limits)?,
        None => { /* literal length is 0; opcode byte is unread */ }
    }

    loop {
        let op_pos = r.pos;
        let op1 = r.read()?;
        #[cfg(feature = "lz77-trace")]
        eprintln!(
            "[lz77] iter op_pos={op_pos} op1=0x{op1:02x} out_len={}",
            out.len()
        );
        // The spec text at §4.7 documents offsets as "+0x3FFF" (long) / "+0"
        // (short), but empirical cross-check against AutoCAD-produced files
        // shows offsets are 1-indexed: every class adds one more than the
        // spec suggests (+0x4000 long, +1 short, +1 for 0x40-0xFF). This
        // matches the ACadSharp reference reader (MIT).
        let (comp_bytes, comp_offset, lit_count_opt) = match op1 {
            0x00..=0x0F => {
                return Err(Error::Lz77InvalidOpcode {
                    opcode: op1,
                    pos: op_pos,
                    out_len: out.len(),
                });
            }
            0x11 => break, // terminator
            0x10 => {
                // compBytes: extended via long-compression-offset, +9.
                let cb = read_long_compression_offset(&mut r)? + 9;
                let (off, lit) = read_two_byte_offset(&mut r)?;
                (cb, off + 0x4000, lit)
            }
            0x12..=0x1F => {
                // compBytes: (op1 & 0x07) with extension if zero, +2.
                // compOffset: bit 3 of op1 contributes bit 14 of offset
                // (ACadSharp — supports offsets up to ~80KB).
                let cb = read_compressed_bytes_extended(op1, 0x07, &mut r)?;
                let mut off = ((op1 & 0x08) as usize) << 11;
                let (extra, lit) = read_two_byte_offset(&mut r)?;
                off |= extra;
                off += 0x4000;
                (cb, off, lit)
            }
            0x20 => {
                let cb = read_long_compression_offset(&mut r)? + 0x21;
                let (off, lit) = read_two_byte_offset(&mut r)?;
                (cb, off + 1, lit)
            }
            0x21..=0x3F => {
                // compBytes: (op1 & 0x1F) with extension, +2.
                let cb = read_compressed_bytes_extended(op1, 0x1F, &mut r)?;
                let (off, lit) = read_two_byte_offset(&mut r)?;
                (cb, off + 1, lit)
            }
            0x40..=0xFF => {
                let cb = (((op1 & 0xF0) >> 4) - 1) as usize;
                let op2 = r.read()?;
                let off = (((op2 as usize) << 2) | (((op1 & 0x0C) >> 2) as usize)) + 1;
                let lit = op1 & 0x03;
                (cb, off, lit)
            }
        };

        // Resolve literal count: 0 means "next bytes are a literal length".
        let lit_count: usize = if lit_count_opt == 0 {
            read_literal_length_or_peek_opcode(&mut r)?.unwrap_or_default()
        } else {
            lit_count_opt as usize
        };

        #[cfg(feature = "lz77-trace")]
        eprintln!(
            "[lz77]   cb={comp_bytes} off={comp_offset} lit={lit_count} pre_out_len={}",
            out.len()
        );

        // Back-reference copy. `comp_offset` of 0 would be an infinite
        // loop since there's no forward-motion; spec doesn't explicitly
        // disallow but in practice never emitted.
        if comp_offset == 0 {
            return Err(Error::Lz77InvalidOffset);
        }
        if comp_bytes > limits.max_backref_len {
            return Err(Error::Lz77BackrefTooLong {
                length: comp_bytes,
                limit: limits.max_backref_len,
            });
        }
        if out.len().saturating_add(comp_bytes) > limits.max_output_bytes {
            return Err(Error::Lz77OutputLimitExceeded {
                limit: limits.max_output_bytes,
            });
        }
        let start = out
            .len()
            .checked_sub(comp_offset)
            .ok_or(Error::Lz77InvalidOffset)?;
        // Byte-at-a-time to allow the self-reading RLE case where
        // `comp_bytes > comp_offset`.
        for i in 0..comp_bytes {
            let b = out[start + i];
            out.push(b);
        }

        copy_literal(&mut r, &mut out, lit_count, &limits)?;
    }

    Ok(out)
}

// ================================================================
// Internal byte reader
// ================================================================

struct Lz77Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lz77Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn read(&mut self) -> Result<u8> {
        if self.pos >= self.bytes.len() {
            return Err(Error::Lz77Truncated);
        }
        let b = self.bytes[self.pos];
        self.pos += 1;
        Ok(b)
    }
    fn rewind_one(&mut self) {
        debug_assert!(self.pos > 0);
        self.pos -= 1;
    }
}

// ================================================================
// Literal length (§4.7 "Literal Length")
// ================================================================

/// Read an extended compressed-byte count for opcode classes 0x10-0x1F
/// and 0x20-0x3F. If `(opcode1 & valid_bits)` is non-zero, use that
/// value directly; otherwise accumulate 0xFF per subsequent 0x00 byte
/// and terminate on the first non-zero byte (adding `valid_bits`).
/// Always add 2 at the end.
fn read_compressed_bytes_extended(
    op1: u8,
    valid_bits: u8,
    r: &mut Lz77Reader<'_>,
) -> Result<usize> {
    let mut cb = (op1 & valid_bits) as usize;
    if cb == 0 {
        loop {
            let b = r.read()?;
            if b == 0 {
                cb = cb.saturating_add(0xFF);
            } else {
                cb = cb
                    .saturating_add(b as usize)
                    .saturating_add(valid_bits as usize);
                break;
            }
        }
    }
    Ok(cb + 2)
}

/// Read a literal-length byte. Returns `Some(n)` if a valid literal
/// length was consumed, or `None` if the peeked byte was an opcode
/// (high nibble set), in which case the byte has been rolled back.
fn read_literal_length_or_peek_opcode(r: &mut Lz77Reader<'_>) -> Result<Option<usize>> {
    let b = r.read()?;
    // 0x00 starts a running-total extension: each subsequent 0x00 adds
    // 0xFF; the first non-zero byte adds itself and terminates. Add 3
    // at the end.
    if b == 0x00 {
        let mut total: usize = 0x0F;
        loop {
            let bb = r.read()?;
            if bb == 0x00 {
                total = total.saturating_add(0xFF);
            } else {
                total = total.saturating_add(bb as usize);
                return Ok(Some(total + 3));
            }
        }
    }
    // Any byte with its high nibble set is actually opcode1 — not a
    // literal length. Roll the cursor back so the main loop can re-read.
    if (b & 0xF0) != 0 {
        r.rewind_one();
        return Ok(None);
    }
    // Remaining range: 0x01..=0x0F → add 3, yielding 4..=0x12.
    Ok(Some((b as usize) + 3))
}

/// Copy `n` literal bytes from input to output, bounded by
/// `limits.max_output_bytes`.
fn copy_literal(
    r: &mut Lz77Reader<'_>,
    out: &mut Vec<u8>,
    n: usize,
    limits: &DecompressLimits,
) -> Result<()> {
    if out.len().saturating_add(n) > limits.max_output_bytes {
        return Err(Error::Lz77OutputLimitExceeded {
            limit: limits.max_output_bytes,
        });
    }
    out.reserve(n);
    for _ in 0..n {
        let b = r.read()?;
        out.push(b);
    }
    Ok(())
}

// ================================================================
// Two-byte offset (§4.7 "Two Byte Offset")
//
//   firstByte, secondByte = next two bytes
//   offset    = (firstByte >> 2) | (secondByte << 6)
//   litCount  = firstByte & 0x03
// ================================================================

fn read_two_byte_offset(r: &mut Lz77Reader<'_>) -> Result<(usize, u8)> {
    let first = r.read()?;
    let second = r.read()?;
    let offset = ((first as usize) >> 2) | ((second as usize) << 6);
    let lit_count = first & 0x03;
    Ok((offset, lit_count))
}

// ================================================================
// Long compression offset (§4.7 "Long Compression Offset")
//
//   First byte non-zero  → use as-is.
//   First byte == 0x00   → running total starts at 0xFF. Each 0x00 byte
//                          adds 0xFF; first non-zero byte adds itself
//                          and terminates.
// ================================================================

fn read_long_compression_offset(r: &mut Lz77Reader<'_>) -> Result<usize> {
    let b = r.read()?;
    if b != 0x00 {
        return Ok(b as usize);
    }
    let mut total: usize = 0xFF;
    loop {
        let bb = r.read()?;
        if bb == 0x00 {
            total = total.saturating_add(0xFF);
        } else {
            return Ok(total.saturating_add(bb as usize));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Literal-only stream: literal length of 5 (encoded as 0x02 → +3),
    /// 5 literal bytes "Hello", then terminator 0x11.
    #[test]
    fn literal_only_stream() {
        let stream = [0x02, b'H', b'e', b'l', b'l', b'o', 0x11];
        let out = decompress(&stream, None).unwrap();
        assert_eq!(&out, b"Hello");
    }

    /// Initial literal length of 0 via high-nibble peekback: first byte
    /// is 0x11 (opcode = terminator). Output is empty.
    #[test]
    fn empty_stream_via_immediate_terminator() {
        let stream = [0x11];
        let out = decompress(&stream, None).unwrap();
        assert!(out.is_empty());
    }

    /// Simple back-reference with opcode 0x22 (compBytes = 4, class
    /// 0x21-0x3F which gets +1 offset adjustment):
    /// Literal "ABCDEFG" (len 7 = 0x04 + 3), copy 4 bytes at effective
    /// offset 3 (= base 2 + 1), then 2 literal bytes "XY", terminator.
    ///
    /// Copy starts at position 7-3 = 4, copies out[4..4+4] = "EFGE"
    /// (the 4th byte wraps since we're reading what we just wrote).
    #[test]
    fn back_reference_with_rle_wrap() {
        // Two-byte offset: base=2, litCount=2
        //   first = (2 << 2) | 2 = 0x0A
        //   second = 0
        // Class 0x21-0x3F adds +1 → effective offset = 3.
        let stream = [
            0x04, b'A', b'B', b'C', b'D', b'E', b'F', b'G', // literal run
            0x22, 0x0A, 0x00, // opcode 0x22 → copy 4 at offset 3, litCount 2
            b'X', b'Y', // 2 literal bytes
            0x11, // terminator
        ];
        let out = decompress(&stream, None).unwrap();
        assert_eq!(&out, b"ABCDEFGEFGEXY");
    }

    /// Literal length extended via 0x00 run: `0x00 0x00 0x02` means
    /// `0x0F + 0xFF + 0x02 + 3 = 275` bytes of literal.
    #[test]
    fn extended_literal_length() {
        const N: usize = 275;
        let mut stream = vec![0x00u8, 0x00, 0x02];
        for i in 0..N {
            stream.push((i & 0xFF) as u8);
        }
        stream.push(0x11);
        let out = decompress(&stream, None).unwrap();
        assert_eq!(out.len(), N);
        for (i, &b) in out.iter().enumerate() {
            assert_eq!(b, (i & 0xFF) as u8);
        }
    }

    /// Opcode 0x40-0xFF encoding: litCount in bits 0-1 of opcode1. Offset
    /// is computed as `((op1 >> 2) & 3) | (op2 << 2) + 1` per the ACadSharp
    /// reference (the spec's "+0" appears to be a typo).
    ///
    /// opcode1 = 0x41 = 0b0100_0001:
    ///   compBytes = ((0x41 & 0xF0) >> 4) - 1 = 3.
    ///   litCount  = 0x41 & 0x03 = 1.
    /// opcode2 = 0x01:
    ///   compOffset = ((0x41 >> 2) & 3) | (0x01 << 2) + 1
    ///              = 0 | 4 + 1 = 5.
    ///
    /// Literal "ABCDEFG" (len 7), copy 3 at offset 5 → out[2..5] = "CDE",
    /// then 1 literal "Z", then terminator. Total: "ABCDEFGCDEZ".
    #[test]
    fn opcode_0x40_family() {
        let stream = [
            0x04, b'A', b'B', b'C', b'D', b'E', b'F', b'G', // 7 literals
            0x41, 0x01, // compBytes=3, offset=5, litCount=1
            b'Z', 0x11,
        ];
        let out = decompress(&stream, None).unwrap();
        assert_eq!(&out, b"ABCDEFGCDEZ");
    }

    /// Truncated input should error cleanly, not panic.
    #[test]
    fn truncated_stream_errors() {
        let stream = [0x04, b'A', b'B']; // says 7 literals, only has 2
        let e = decompress(&stream, None).unwrap_err();
        assert!(matches!(e, Error::Lz77Truncated));
    }

    /// Reserved opcode range 0x00-0x0F is not valid AFTER the initial
    /// literal length. Construct: 0 literal length (via peekback of 0x11
    /// is too trivial; use 0 via direct). We need a stream that reaches
    /// the main loop with opcode < 0x10.
    ///
    /// Easiest: 1-byte literal (0x0F? but that's a valid literal length)
    /// → use 0x01 = length 4, 4 literals, then opcode 0x05 (invalid).
    #[test]
    fn reserved_opcode_errors() {
        let stream = [0x01, b'A', b'B', b'C', b'D', 0x05];
        let e = decompress(&stream, None).unwrap_err();
        assert!(matches!(e, Error::Lz77InvalidOpcode { opcode: 0x05, .. }));
    }

    // ================================================================
    // Decompression-bomb defense regression tests.
    // ================================================================

    /// A literal run that asks for more bytes than the configured
    /// `max_output_bytes` must error before any allocation.
    #[test]
    fn output_limit_rejects_oversize_literal_run() {
        let limits = DecompressLimits {
            max_output_bytes: 8,
            max_backref_len: 8,
        };
        // Literal length 8 → 0x05 + 3 = 8; 8 bytes of literal.
        let mut stream = vec![0x05];
        stream.extend_from_slice(b"AAAAAAAA");
        stream.push(0x11);
        // 8 bytes fits exactly; must succeed.
        let out = decompress_with_limits(&stream, None, limits).unwrap();
        assert_eq!(out.len(), 8);

        // Nudge the literal length up one byte (0x06 + 3 = 9) and
        // expect the limit to fire.
        let mut bomb = vec![0x06];
        bomb.extend_from_slice(b"AAAAAAAAA");
        bomb.push(0x11);
        let err = decompress_with_limits(&bomb, None, limits).unwrap_err();
        assert!(matches!(err, Error::Lz77OutputLimitExceeded { limit: 8 }));
    }

    /// A back-reference copy whose length alone would exceed the
    /// configured limit must error.
    #[test]
    fn output_limit_rejects_oversize_backref() {
        // Build a stream: literal "AB" (len=2 → 0x00+3 via extension),
        // then a 0x20 opcode that claims comp_bytes = 64 and offset = 2.
        // Skip 0x20: uses the long-compression-offset extension.
        //
        // Simpler: use opcode 0x21 (class 0x21-0x3F) with a small cb.
        // cb = (0x21 & 0x1F) + 2 = 1 + 2 = 3. Not useful for a big copy.
        //
        // Use opcode 0x20 which extends comp_bytes via the long form.
        // cb = long_extension + 0x21.
        // To hit cb = 100, long_extension = 79, encoded as single byte 0x4F.
        //
        // Layout: [literal_len=0x02 "ABC", terminator would be 0x11, but we
        //  need the main-loop opcode]. Easier: use the 0x40-0xFF family
        //  where cb = (op1>>4 - 1). E.g., op1=0xC1 → cb = (0xC >> 0) - 1
        //  = 0xB = 11. Still small. Combined with class 0x20 is fastest.
        let mut stream = vec![
            0x01, b'A', b'B', b'C', b'D', // 4 literal bytes
            0x20, // class 0x20 → cb = long_extension + 0x21
            0x4F, // long_extension first byte = 0x4F (79). cb = 79 + 0x21 = 112.
            0x00, 0x00, // two-byte offset, small value → offset = 0 + 1 = 1, lit=0
            0x11, // terminator (won't be reached)
        ];
        // Force literal-len-extension for the lit=0 case to avoid the
        // reserved-opcode path; not needed since our terminator 0x11 is
        // NOT in 0x00..=0x0F.
        // (literal length 0 via peekback: the first post-opcode byte is
        //  the lit_count from the two-byte-offset, which is 0, then we
        //  read a real literal length. Use 0x11 as the peeked byte to
        //  terminate via the opcode path.)
        stream.push(0x11);

        let limits = DecompressLimits {
            max_output_bytes: 1024 * 1024,
            max_backref_len: 50, // less than the 112-byte copy above
        };
        let err = decompress_with_limits(&stream, None, limits).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Lz77BackrefTooLong {
                    length: 112,
                    limit: 50
                }
            ),
            "err={err:?}"
        );
    }

    /// `expected_size` must be clamped to the output limit so a
    /// caller-claimed huge size cannot trigger a giant up-front
    /// `Vec::with_capacity` allocation.
    #[test]
    fn expected_size_is_clamped_to_output_limit() {
        let limits = DecompressLimits {
            max_output_bytes: 16,
            max_backref_len: 16,
        };
        // Decompressing the empty-terminator stream with a huge
        // expected_size must not allocate 1 TiB. This test does not
        // observe memory directly — it asserts behavior (no error,
        // empty output) that proves the function didn't try to
        // allocate more than the limit.
        let stream = [0x11u8];
        let out = decompress_with_limits(&stream, Some(1usize << 40), limits).unwrap();
        assert!(out.is_empty());
    }

    /// The default `DecompressLimits` profile ships with a 256 MiB
    /// output cap. This pins the contract so a future config tweak
    /// has to consciously lower/raise it rather than drift unnoticed.
    #[test]
    fn default_limits_cap_output_at_256_mib() {
        let d = DecompressLimits::default();
        assert_eq!(d.max_output_bytes, 256 * 1024 * 1024);
        assert_eq!(d.max_backref_len, 1024 * 1024);
    }

    /// Compression-bomb regression: a small input that falsely claims
    /// a huge `expected_size` must not allocate anywhere near that
    /// size. Caller-supplied hint gets clamped to the output limit,
    /// and the decoded output is small and accurate — no panic, no
    /// OOM, no over-read.
    #[test]
    fn small_input_with_huge_expected_size_stays_bounded() {
        // 6-byte input decompresses to a 5-byte literal "HELLO".
        // expected_size claims 1 TiB — must be clamped by the default
        // DecompressLimits (256 MiB) and allocate accordingly.
        let stream = [0x02, b'H', b'E', b'L', b'L', b'O', 0x11];
        let huge = 1usize << 40;
        let out = decompress(&stream, Some(huge)).unwrap();
        assert_eq!(&out, b"HELLO");
    }
}
