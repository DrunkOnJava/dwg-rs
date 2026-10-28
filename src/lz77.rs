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

#[cfg(feature = "lz77-trace")]
fn lz77_trace_enabled() -> bool {
    use std::sync::OnceLock;

    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("DWG_LZ77_TRACE").is_some())
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        if lz77_trace_enabled() {
            eprintln!(
                "[lz77] iter op_pos={op_pos} op1=0x{op1:02x} out_len={}",
                out.len()
            );
        }
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
        if lz77_trace_enabled() {
            eprintln!(
                "[lz77]   cb={comp_bytes} off={comp_offset} lit={lit_count} pre_out_len={}",
                out.len()
            );
        }

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
