//! R2004+ data-section writer (spec §4.5, §4.6).
//!
//! Produces a byte block that mirrors the layout of a decompressed
//! section as it appears on disk:
//!
//! ```text
//! +--------------------------------+
//! | 32-byte data-page header       |   (§4.6, XOR-masked with Sec_Mask)
//! +--------------------------------+
//! | LZ77-compressed bit-stream     |   (§4.7)
//! +--------------------------------+
//! | optional padding to 32-bytes   |
//! +--------------------------------+
//! ```
//!
//! The reader's decoder is in [`crate::reader::DwgFile::read_section`]
//! and [`crate::lz77::decompress`]; this module is the inverse.
//!
//! # Current scope
//!
//! - Emits a valid 32-byte header: page type (0x4163043B),
//!   section-number, data-size, page-size, start-offset (0),
//!   checksum (§4.6.1 32-bit sum), all-zero trailer fields.
//! - Applies Sec_Mask XOR at the given page offset.
//! - LZ77-compresses the payload via the literal-only encoder.
//! - Computes + writes the page checksum.
//!
//! # Deferred
//!
//! - Rewriting the top-level Section Page Map and Section Info so
//!   the reader can *find* the new section. That requires
//!   rewriting the 0x6C encrypted file header and the two system
//!   pages that hold the page-map and section-info — a substantial
//!   additional pass. `DwgFile::to_bytes()` scaffolding for that
//!   pipeline is in [`crate::file_writer`].

use crate::cipher::section_page_mask;
use crate::error::Result;
use crate::lz77_encode;

/// Type tag for data-page headers (spec §4.6, literal `0x4163_043B`).
pub const DATA_PAGE_HEADER_TYPE: u32 = 0x4163_043B;

/// Size of the masked data-page header, in bytes.
pub const HEADER_SIZE: usize = 32;

/// A built section ready to drop into a page buffer.
#[derive(Debug, Clone)]
pub struct BuiltSection {
    /// Final bytes: 32-byte masked header + LZ77 stream + optional
    /// padding to a 32-byte boundary.
    pub bytes: Vec<u8>,
    /// Page-file offset the caller intends to place this section at.
    /// Needed because the Sec_Mask is a function of file position.
    pub page_offset: u32,
    /// Size of the post-compression payload (before padding).
    pub compressed_size: u32,
    /// Size of the uncompressed source bytes.
    pub decompressed_size: u32,
    /// Page checksum value written into the header.
    pub checksum: u32,
}

/// Build a data-page section ready to write at `page_offset` of the
/// host file.
///
/// `section_number` is the 1-based section index assigned by the
/// caller's Section Info table; `page_offset` is the absolute file
/// offset the 32-byte header will land at (used to compute the
/// Sec_Mask).
pub fn build_section(
    decompressed: &[u8],
    section_number: u32,
    page_offset: u32,
) -> Result<BuiltSection> {
    let compressed = lz77_encode::compress(decompressed)?;
    let decompressed_size = decompressed.len() as u32;
    let compressed_size = compressed.len() as u32;

    // Compose the 32-byte header as eight 32-bit LE words.
    //
    // Word | Field
    // -----|--------------------------------------------
    //  0   | page_type (0x4163043B)
    //  1   | section_number
    //  2   | data_size (compressed_size)
    //  3   | page_size (includes header + padding)
    //  4   | start_offset = 0 (relative within section)
    //  5   | page_checksum
    //  6   | data_checksum
    //  7   | unknown (ODA writes 0x80)
    let total_len = HEADER_SIZE + compressed_size as usize;
    let page_size = ((total_len + 31) / 32 * 32) as u32;

    let page_checksum = compute_checksum(&compressed, 0);
    let data_checksum = compute_checksum(decompressed, 0);

    let mut header = [0u8; HEADER_SIZE];
    write_u32_le(&mut header, 0, DATA_PAGE_HEADER_TYPE);
    write_u32_le(&mut header, 4, section_number);
    write_u32_le(&mut header, 8, compressed_size);
    write_u32_le(&mut header, 12, page_size);
    write_u32_le(&mut header, 16, 0);
    write_u32_le(&mut header, 20, page_checksum);
    write_u32_le(&mut header, 24, data_checksum);
    write_u32_le(&mut header, 28, 0x80);

    // Apply the Sec_Mask XOR to all 8 words.
    apply_sec_mask(&mut header, page_offset);

    let mut bytes = Vec::with_capacity(page_size as usize);
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&compressed);
    // Pad to a 32-byte multiple with zeros.
    while bytes.len() < page_size as usize {
        bytes.push(0);
    }

    Ok(BuiltSection {
        bytes,
        page_offset,
        compressed_size,
        decompressed_size,
        checksum: page_checksum,
    })
}

/// Compute spec §4.6.1 page checksum: 32-bit rolling sum with bit
/// rotation. `seed` is 0 for a fresh chunk.
pub fn compute_checksum(data: &[u8], seed: u32) -> u32 {
    let mut sum: u32 = seed;
    for &b in data {
        sum = sum.rotate_left(1);
        sum = sum.wrapping_add(b as u32);
    }
    sum
}

fn apply_sec_mask(header: &mut [u8; HEADER_SIZE], page_offset: u32) {
    let mask = section_page_mask(page_offset);
    for chunk in header.chunks_exact_mut(4) {
        let mut word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        word ^= mask;
        let bytes = word.to_le_bytes();
        chunk.copy_from_slice(&bytes);
    }
}

fn write_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    let bytes = value.to_le_bytes();
    buf[offset..offset + 4].copy_from_slice(&bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cipher::section_page_mask;

    #[test]
    fn build_empty_section() {
        let s = build_section(&[], 1, 0x1000).unwrap();
        assert_eq!(s.bytes.len() % 32, 0);
        assert_eq!(s.decompressed_size, 0);
        // compressed is the literal-only empty stream = [0x11] = 1 byte.
        assert_eq!(s.compressed_size, 1);
    }

    #[test]
    fn build_small_section() {
        let data = b"Hello, world!"; // 13 bytes
        let s = build_section(data, 5, 0x2000).unwrap();
        assert_eq!(s.decompressed_size, 13);
        assert!(s.compressed_size > 13); // framing adds a few bytes
        assert_eq!(s.bytes.len() % 32, 0);
    }

    #[test]
    fn header_roundtrips_sec_mask() {
        // Build a section, then strip the Sec_Mask to verify it recovers
        // the literal page_type tag.
        let data = b"ABCDEFGH";
        let s = build_section(data, 1, 0x1000).unwrap();
        let mut header_copy: [u8; HEADER_SIZE] = s.bytes[..32].try_into().unwrap();
        let mask = section_page_mask(0x1000);
        // Undo the mask on the first word.
        let word0 = u32::from_le_bytes(header_copy[..4].try_into().unwrap());
        assert_eq!(word0 ^ mask, DATA_PAGE_HEADER_TYPE);
        // And verify the section-number word recovers correctly.
        let word1 = u32::from_le_bytes(header_copy[4..8].try_into().unwrap());
        assert_eq!(word1 ^ mask, 1);
        // The padded length past the header should equal compressed_size.
        let word2 = u32::from_le_bytes(header_copy[8..12].try_into().unwrap());
        assert_eq!(word2 ^ mask, s.compressed_size);
        header_copy[0] ^= 0; // silence unused-mut
    }

    #[test]
    fn checksum_deterministic() {
        let a = compute_checksum(b"HELLO", 0);
        let b = compute_checksum(b"HELLO", 0);
        assert_eq!(a, b);
        // And different inputs produce different sums in the overwhelming
        // majority of cases.
        let c = compute_checksum(b"HELLO!", 0);
        assert_ne!(a, c);
    }

    #[test]
    fn checksum_seed_chains() {
        // Chaining: checksum(B, seed=checksum(A, 0)) should not equal
        // checksum(AB, 0) in general (the spec's algorithm isn't a
        // simple running checksum), but our implementation should be
        // deterministic regardless.
        let s1 = compute_checksum(b"PART1", 0);
        let s2 = compute_checksum(b"PART2", s1);
        assert_ne!(s2, 0);
    }
}
