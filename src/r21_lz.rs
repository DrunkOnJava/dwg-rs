//! R2007 (`AC1021`) LZ decompression — ODA spec §5.10.
//!
//! R2007 uses a different LZ77 variant from the one R2004 and R2010+ use
//! ([`crate::lz77`], spec §4.7). The stream is a sequence of *compressed
//! chunks* (literal runs copied out of the source buffer) interleaved with
//! *decompressed chunks* (back-references into the output produced so far).
//!
//! # Stream shape (§5.10)
//!
//! ```text
//! opcode0
//!   if (opcode0 >> 4) == 2 → skip 2 bytes, literal length = next byte & 7,
//!                            skip that byte too (3 bytes consumed after the
//!                            opcode)
//!   otherwise              → opcode0 IS the first literal-length opcode
//! loop {
//!   literal run  (length 0 ⇒ read it from the next opcode, §5.10.1)
//!   one or more back-references (§5.10.2 `ReadInstructions`)
//! }
//! ```
//!
//! # Literal length (§5.10.1)
//!
//! `length = opcode + 8`. The value `0x17` escapes: one extra byte is added,
//! and if that byte is `0xFF` further little-endian 16-bit words are added
//! until one is not `0xFFFF`. Reaching this path means the opcode's high
//! nibble was zero, so the opcode is in `0x00..=0x0F` and the un-escaped
//! range is `8..=0x16`.
//!
//! # The literal copy is *permuted*, not a `memcpy` (§5.10.1 table)
//!
//! This is the part of §5.10 that is easy to skip past and impossible to
//! work around. The spec says "the order of bytes in source and target
//! buffer are different … For copying 1-32 bytes, a combination of sub byte
//! blocks is made", and then prints a table of `(block size)[source index]`
//! sequences for each count 1..=32.
//!
//! The blocks are the *smaller copy functions themselves*, applied
//! recursively — that is what "a combination of sub byte blocks" means, and
//! it is what the bytes agree with. [`COPY_BLOCKS`] holds the table
//! verbatim; [`emit_permuted`] recurses through it. Concretely:
//!
//! | count | table row | expands to |
//! |---|---|---|
//! | 2 | `1 [1], 1 [0]` | `src[1], src[0]` — reversed |
//! | 4 | `1 [0], 1 [1], 1 [2], 1 [3]` | straight |
//! | 8 | `4 [0], 4 [4]` | straight (each `4` is straight) |
//! | 11 | `2 [9], 8 [1], 1 [0]` | `src[10], src[9]`, `src[1..9]`, `src[0]` |
//! | 16 | `8 [8], 8 [0]` | the two halves swapped |
//!
//! Two independent measurements pin the recursion (see
//! `examples/probe_r2007_container.rs`):
//!
//! - The **file header** of `line_2007.dwg` opens with a 2-byte literal
//!   whose source bytes are `00 70`; the decompressed header's first field
//!   is `Header size` = `0x70`, so the pair must come out `70 00`. A
//!   straight copy produces `00 70` and the whole 272-byte header is
//!   garbage from byte 0.
//! - The **section map** of the same file contains an 11-byte literal
//!   inside the section name `AcDb:AppInfoHistory`. With the `2 [9]` block
//!   copied straight the name reads `AcDb:Ap\0pInfoHistory` — an invalid
//!   UTF-16 sequence — and the following `hashcode` fields stop matching
//!   the §5.2 table. With the block reversed all thirteen section names
//!   decode and all twelve spec-documented hashcodes match exactly.
//!
//! Runs longer than 32 bytes are copied as whole 32-byte blocks first and
//! the 1..=31 remainder last, in source order.
//!
//! # Back-references (§5.10.2)
//!
//! [`read_instructions`] is the spec's `ReadInstructions` switch on the
//! opcode's high nibble, transcribed field-for-field. After each copy the
//! low three bits of the running opcode give the next literal length; a
//! zero there means another back-reference follows, unless the next
//! opcode's high nibble is zero — in which case that opcode is the next
//! literal-length opcode.

use crate::error::{Error, Result};
use crate::lz77::DecompressLimits;

/// One row of the §5.10.1 sub-block table: `(block size, source index)`.
type Block = (usize, usize);

/// The §5.10.1 literal copy table, indexed by byte count.
///
/// Slot 0 is unused and slot 1 is the base case (a single byte); every
/// other slot lists the sub-blocks that make up that count's copy
/// function, in output order. See the module docs for why the blocks are
/// expanded recursively.
const COPY_BLOCKS: [&[Block]; 33] = [
    &[],                                    // 0 — unused
    &[(1, 0)],                              // 1
    &[(1, 1), (1, 0)],                      // 2
    &[(1, 2), (1, 1), (1, 0)],              // 3
    &[(1, 0), (1, 1), (1, 2), (1, 3)],      // 4
    &[(1, 4), (4, 0)],                      // 5
    &[(1, 5), (4, 1), (1, 0)],              // 6
    &[(2, 5), (4, 1), (1, 0)],              // 7
    &[(4, 0), (4, 4)],                      // 8
    &[(1, 8), (8, 0)],                      // 9
    &[(1, 9), (8, 1), (1, 0)],              // 10
    &[(2, 9), (8, 1), (1, 0)],              // 11
    &[(4, 8), (8, 0)],                      // 12
    &[(1, 12), (4, 8), (8, 0)],             // 13
    &[(1, 13), (4, 9), (8, 1), (1, 0)],     // 14
    &[(2, 13), (4, 9), (8, 1), (1, 0)],     // 15
    &[(8, 8), (8, 0)],                      // 16
    &[(8, 9), (1, 8), (8, 0)],              // 17
    &[(1, 17), (16, 1), (1, 0)],            // 18
    &[(3, 16), (16, 0)],                    // 19
    &[(4, 16), (16, 0)],                    // 20
    &[(1, 20), (4, 16), (16, 0)],           // 21
    &[(2, 20), (4, 16), (16, 0)],           // 22
    &[(3, 20), (4, 16), (16, 0)],           // 23
    &[(8, 16), (16, 0)],                    // 24
    &[(8, 17), (1, 16), (16, 0)],           // 25
    &[(1, 25), (8, 17), (1, 16), (16, 0)],  // 26
    &[(2, 25), (8, 17), (1, 16), (16, 0)],  // 27
    &[(4, 24), (8, 16), (16, 0)],           // 28
    &[(1, 28), (4, 24), (8, 16), (16, 0)],  // 29
    &[(2, 28), (4, 24), (8, 16), (16, 0)],  // 30
    &[(1, 30), (4, 26), (8, 18), (16, 2), (2, 0)], // 31
    &[(16, 16), (16, 0)],                   // 32
];

/// Append the `count` bytes of `src` starting at `base` to `out`, in the
/// order the §5.10.1 table prescribes for that count.
///
/// `count` must be in `1..=32`; the caller splits longer runs into 32-byte
/// blocks plus a remainder. Returns [`Error::Lz77Truncated`] if the source
/// range is not fully present.
fn emit_permuted(src: &[u8], base: usize, count: usize, out: &mut Vec<u8>) -> Result<()> {
    if base + count > src.len() {
        return Err(Error::Lz77Truncated);
    }
    if count == 1 {
        out.push(src[base]);
        return Ok(());
    }
    for &(size, idx) in COPY_BLOCKS[count] {
        emit_permuted(src, base + idx, size, out)?;
    }
    Ok(())
}

/// Copy a literal run of `count` bytes from `src[pos..]` into `out`,
/// applying the §5.10.1 permutation. Returns the new source position.
fn copy_literal(src: &[u8], mut pos: usize, mut count: usize, out: &mut Vec<u8>) -> Result<usize> {
    while count >= 32 {
        emit_permuted(src, pos, 32, out)?;
        pos += 32;
        count -= 32;
    }
    if count > 0 {
        emit_permuted(src, pos, count, out)?;
        pos += count;
    }
    Ok(pos)
}

/// Fetch one byte, mapping the end of the buffer to a typed error.
fn byte_at(src: &[u8], pos: usize) -> Result<u8> {
    src.get(pos).copied().ok_or(Error::Lz77Truncated)
}

/// The spec's `ReadInstructions` (§5.10.2): decode a back-reference from
/// `opcode` plus 1-3 further source bytes.
///
/// Returns `(new source position, new opcode, source offset, length)`. The
/// returned opcode carries the next instruction's low bits — its `& 7` is
/// the length of the literal run that follows once the back-reference
/// chain ends.
fn read_instructions(src: &[u8], mut pos: usize, mut opcode: u8) -> Result<(usize, u8, u64, u64)> {
    let offset: u64;
    let mut length: u64;
    match opcode >> 4 {
        0 => {
            length = u64::from(opcode & 0x0F) + 0x13;
            let lo = u64::from(byte_at(src, pos)?);
            pos += 1;
            opcode = byte_at(src, pos)?;
            pos += 1;
            length += u64::from((opcode >> 3) & 0x10);
            offset = (u64::from(opcode & 0x78) << 5) + 1 + lo;
        }
        1 => {
            length = u64::from(opcode & 0x0F) + 3;
            let lo = u64::from(byte_at(src, pos)?);
            pos += 1;
            opcode = byte_at(src, pos)?;
            pos += 1;
            offset = (u64::from(opcode & 0xF8) << 5) + 1 + lo;
        }
        2 => {
            let lo = u64::from(byte_at(src, pos)?);
            pos += 1;
            let hi = u64::from(byte_at(src, pos)?);
            pos += 1;
            let mut off = ((hi << 8) & 0xFF00) | lo;
            length = u64::from(opcode & 7);
            if opcode & 8 == 0 {
                opcode = byte_at(src, pos)?;
                pos += 1;
                length += u64::from(opcode & 0xF8);
            } else {
                off += 1;
                length += u64::from(byte_at(src, pos)?) << 3;
                pos += 1;
                opcode = byte_at(src, pos)?;
                pos += 1;
                length += (u64::from(opcode & 0xF8) << 8) + 0x100;
            }
            offset = off;
        }
        _ => {
            length = u64::from(opcode >> 4);
            let lo = u64::from(opcode & 0x0F);
            opcode = byte_at(src, pos)?;
            pos += 1;
            offset = (u64::from(opcode & 0xF8) << 1) + lo + 1;
        }
    }
    Ok((pos, opcode, offset, length))
}

/// The spec's `ReadLiteralLength` (§5.10.1): `opcode + 8`, with `0x17`
/// escaping to one extra byte and then to a chain of 16-bit words.
fn literal_length(src: &[u8], mut pos: usize, opcode: u8) -> Result<(usize, u64)> {
    let mut length = u64::from(opcode) + 8;
    if length == 0x17 {
        let n = u64::from(byte_at(src, pos)?);
        pos += 1;
        length += n;
        if n == 0xFF {
            loop {
                let lo = u64::from(byte_at(src, pos)?);
                let hi = u64::from(byte_at(src, pos + 1)?);
                pos += 2;
                let word = (hi << 8) | lo;
                length += word;
                if word != 0xFFFF {
                    break;
                }
            }
        }
    }
    Ok((pos, length))
}

/// Decompress an R2007 (`AC1021`) LZ stream (spec §5.10).
///
/// `expected` is the decompressed size the container recorded for this
/// buffer — the R2007 page/section tables always carry one, and the
/// decoder stops as soon as it has produced that many bytes. `limits`
/// bounds the output for the decompression-bomb defense described in
/// `SECURITY.md`; the effective cap is the smaller of `expected` and
/// [`DecompressLimits::max_output_bytes`].
///
/// Returns [`Error::Lz77OutputLimitExceeded`] if the cap is hit,
/// [`Error::Lz77InvalidOffset`] on a back-reference that points before the
/// start of the output, and [`Error::Lz77Truncated`] if the stream ends
/// mid-instruction.
pub fn decompress(src: &[u8], expected: usize, limits: DecompressLimits) -> Result<Vec<u8>> {
    if expected > limits.max_output_bytes {
        return Err(Error::Lz77OutputLimitExceeded {
            limit: limits.max_output_bytes,
        });
    }
    if src.is_empty() {
        return Ok(Vec::new());
    }
    let mut out: Vec<u8> = Vec::with_capacity(expected);
    let mut pos = 0usize;
    let mut opcode = byte_at(src, pos)?;
    pos += 1;
    // §5.10: a leading opcode whose high nibble is 2 carries the first
    // literal length in its third following byte; anything else IS the
    // first literal-length opcode.
    let mut length: u64;
    let mut pending_literal_opcode: Option<u8>;
    if opcode >> 4 == 2 {
        pos += 2;
        length = u64::from(byte_at(src, pos)? & 0x07);
        pos += 1;
        pending_literal_opcode = None;
    } else {
        length = 0;
        pending_literal_opcode = Some(opcode);
    }

    loop {
        if length == 0 {
            let op = match pending_literal_opcode.take() {
                Some(op) => op,
                None => {
                    let op = byte_at(src, pos)?;
                    pos += 1;
                    op
                }
            };
            let (next, len) = literal_length(src, pos, op)?;
            pos = next;
            length = len;
        }
        pending_literal_opcode = None;
        let want = usize::try_from(length).map_err(|_| Error::Lz77Truncated)?;
        if out.len() + want > expected {
            // The container's recorded size is the contract; a stream that
            // wants to write past it is malformed.
            return Err(Error::Lz77OutputLimitExceeded { limit: expected });
        }
        pos = copy_literal(src, pos, want, &mut out)?;
        if out.len() >= expected || pos >= src.len() {
            break;
        }
        opcode = byte_at(src, pos)?;
        pos += 1;
        loop {
            let (next, next_opcode, offset, len) = read_instructions(src, pos, opcode)?;
            pos = next;
            opcode = next_opcode;
            let copy_len = usize::try_from(len).map_err(|_| Error::Lz77Truncated)?;
            if copy_len > limits.max_backref_len {
                return Err(Error::Lz77BackrefTooLong {
                    length: copy_len,
                    limit: limits.max_backref_len,
                });
            }
            let back = usize::try_from(offset).map_err(|_| Error::Lz77InvalidOffset)?;
            let start = out.len().checked_sub(back).ok_or(Error::Lz77InvalidOffset)?;
            if out.len() + copy_len > expected {
                return Err(Error::Lz77OutputLimitExceeded { limit: expected });
            }
            for i in 0..copy_len {
                let b = out[start + i];
                out.push(b);
            }
            length = u64::from(opcode & 7);
            if length != 0 || pos >= src.len() {
                break;
            }
            opcode = byte_at(src, pos)?;
            pos += 1;
            if opcode >> 4 == 0 {
                // This opcode is the next literal-run length, not another
                // back-reference.
                pending_literal_opcode = Some(opcode);
                break;
            }
            if opcode >> 4 == 15 {
                opcode &= 15;
            }
        }
        if out.len() >= expected || pos >= src.len() {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first sixteen bytes of `line_2007.dwg`'s Reed-Solomon-decoded
    /// file-header payload, decompressed by hand from the spec:
    ///
    /// - `20 00 00 02` — leading opcode `0x20` (high nibble 2), so the
    ///   literal length is `src[3] & 7` = 2.
    /// - literal `00 70` → permuted to `70 00` (header size `0x70`).
    /// - opcode `0x70` → high nibble 7: length 7, offset 1 → seven zero
    ///   bytes, completing the `u64` `0x70`.
    /// - `0x02 & 7` = 2 → literal `01 03` → permuted to `03 01`.
    /// - opcode `0x58` → length 5, offset 9 → five zero bytes.
    ///
    /// Result: `70 00 00 00 00 00 00 00` (header size 112) followed by
    /// `00 03 01 00 00 00 00 00` (file size 0x10300 = 66304, which is
    /// `line_2007.dwg`'s exact byte count).
    const HEADER_PREFIX: &[u8] = &[
        0x20, 0x00, 0x00, 0x02, 0x00, 0x70, 0x70, 0x02, 0x01, 0x03, 0x58, 0x00,
    ];

    #[test]
    fn header_prefix_decodes_to_the_two_documented_u64_fields() {
        let out = decompress(HEADER_PREFIX, 16, DecompressLimits::default()).unwrap();
        assert_eq!(out.len(), 16);
        assert_eq!(u64::from_le_bytes(out[0..8].try_into().unwrap()), 0x70);
        assert_eq!(u64::from_le_bytes(out[8..16].try_into().unwrap()), 66304);
    }

    #[test]
    fn two_byte_literal_block_is_reversed() {
        let mut out = Vec::new();
        emit_permuted(&[0xAA, 0xBB], 0, 2, &mut out).unwrap();
        assert_eq!(out, vec![0xBB, 0xAA]);
    }

    #[test]
    fn four_and_eight_byte_literal_blocks_are_straight() {
        let src: Vec<u8> = (0..8).collect();
        let mut four = Vec::new();
        emit_permuted(&src, 0, 4, &mut four).unwrap();
        assert_eq!(four, vec![0, 1, 2, 3]);
        let mut eight = Vec::new();
        emit_permuted(&src, 0, 8, &mut eight).unwrap();
        assert_eq!(eight, vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn sixteen_byte_literal_block_swaps_its_halves() {
        let src: Vec<u8> = (0..16).collect();
        let mut out = Vec::new();
        emit_permuted(&src, 0, 16, &mut out).unwrap();
        assert_eq!(
            out,
            vec![8, 9, 10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7]
        );
    }

    /// The `AcDb:AppInfoHistory` measurement from the module docs: an
    /// 11-byte literal whose `2 [9]` block must come out reversed for the
    /// UTF-16 name to be well formed.
    #[test]
    fn eleven_byte_literal_reverses_only_its_two_byte_block() {
        let src: Vec<u8> = (0..11).collect();
        let mut out = Vec::new();
        emit_permuted(&src, 0, 11, &mut out).unwrap();
        assert_eq!(out, vec![10, 9, 1, 2, 3, 4, 5, 6, 7, 8, 0]);
    }

    #[test]
    fn thirty_two_byte_block_swaps_its_halves_and_recurses() {
        let src: Vec<u8> = (0..32).collect();
        let mut out = Vec::new();
        emit_permuted(&src, 0, 32, &mut out).unwrap();
        let expect: Vec<u8> = (24..32).chain(16..24).chain(8..16).chain(0..8).collect();
        assert_eq!(out, expect);
    }

    #[test]
    fn truncated_stream_errors_rather_than_panicking() {
        let err = decompress(&[0x20, 0x00, 0x00, 0x07], 64, DecompressLimits::default()).unwrap_err();
        assert!(matches!(err, Error::Lz77Truncated));
    }

    #[test]
    fn output_cap_is_enforced() {
        let limits = DecompressLimits {
            max_output_bytes: 8,
            ..DecompressLimits::default()
        };
        let err = decompress(HEADER_PREFIX, 16, limits).unwrap_err();
        assert!(matches!(err, Error::Lz77OutputLimitExceeded { limit: 8 }));
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(
            decompress(&[], 0, DecompressLimits::default())
                .unwrap()
                .is_empty()
        );
    }
}
