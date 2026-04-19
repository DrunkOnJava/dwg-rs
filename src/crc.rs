//! CRC algorithms used by the DWG format.
//!
//! # CRC-8 (spec §2.14.1)
//!
//! Despite the name, the DWG "8-bit CRC" produces a 16-bit output. The
//! lookup table contains 256 16-bit values; the algorithm consumes one
//! byte of input per step, XOR-folding the low byte of the running state
//! with the input byte to index the table. This is a standard "reflected
//! CRC-16 with byte-indexed table" variant.
//!
//! CRCs always appear on byte boundaries (callers must `align_to_byte()`
//! their bit cursor first).
//!
//! # CRC-32 (spec §2.14.2, §4.1)
//!
//! Standard reflected IEEE 802.3 CRC-32 (polynomial 0xEDB88320, initial
//! state 0x00000000, no final XOR). Used for the R2004+ file-header
//! checksum and the R2018 encoded-section header.
//!
//! The R2004+ header spec has a specific note: "the CRC calculation is
//! done including the 4 CRC bytes that are initially zero" — so the caller
//! must zero those 4 bytes before computing.

/// Table of 256 16-bit values for the DWG CRC-8 algorithm, as published in
/// the ODA spec §2.14.1 and reproduced by every open implementation of the
/// format (homer.com.au/webdoc/dwgspec/, libopencad, BricsCAD, LibreDWG).
///
/// This is public-domain tabulated data, not source code, so it is safe to
/// reproduce verbatim under any license.
static CRC8_TABLE: [u16; 256] = [
    0x0000, 0xC0C1, 0xC181, 0x0140, 0xC301, 0x03C0, 0x0280, 0xC241, 0xC601, 0x06C0, 0x0780, 0xC741,
    0x0500, 0xC5C1, 0xC481, 0x0440, 0xCC01, 0x0CC0, 0x0D80, 0xCD41, 0x0F00, 0xCFC1, 0xCE81, 0x0E40,
    0x0A00, 0xCAC1, 0xCB81, 0x0B40, 0xC901, 0x09C0, 0x0880, 0xC841, 0xD801, 0x18C0, 0x1980, 0xD941,
    0x1B00, 0xDBC1, 0xDA81, 0x1A40, 0x1E00, 0xDEC1, 0xDF81, 0x1F40, 0xDD01, 0x1DC0, 0x1C80, 0xDC41,
    0x1400, 0xD4C1, 0xD581, 0x1540, 0xD701, 0x17C0, 0x1680, 0xD641, 0xD201, 0x12C0, 0x1380, 0xD341,
    0x1100, 0xD1C1, 0xD081, 0x1040, 0xF001, 0x30C0, 0x3180, 0xF141, 0x3300, 0xF3C1, 0xF281, 0x3240,
    0x3600, 0xF6C1, 0xF781, 0x3740, 0xF501, 0x35C0, 0x3480, 0xF441, 0x3C00, 0xFCC1, 0xFD81, 0x3D40,
    0xFF01, 0x3FC0, 0x3E80, 0xFE41, 0xFA01, 0x3AC0, 0x3B80, 0xFB41, 0x3900, 0xF9C1, 0xF881, 0x3840,
    0x2800, 0xE8C1, 0xE981, 0x2940, 0xEB01, 0x2BC0, 0x2A80, 0xEA41, 0xEE01, 0x2EC0, 0x2F80, 0xEF41,
    0x2D00, 0xEDC1, 0xEC81, 0x2C40, 0xE401, 0x24C0, 0x2580, 0xE541, 0x2700, 0xE7C1, 0xE681, 0x2640,
    0x2200, 0xE2C1, 0xE381, 0x2340, 0xE101, 0x21C0, 0x2080, 0xE041, 0xA001, 0x60C0, 0x6180, 0xA141,
    0x6300, 0xA3C1, 0xA281, 0x6240, 0x6600, 0xA6C1, 0xA781, 0x6740, 0xA501, 0x65C0, 0x6480, 0xA441,
    0x6C00, 0xACC1, 0xAD81, 0x6D40, 0xAF01, 0x6FC0, 0x6E80, 0xAE41, 0xAA01, 0x6AC0, 0x6B80, 0xAB41,
    0x6900, 0xA9C1, 0xA881, 0x6840, 0x7800, 0xB8C1, 0xB981, 0x7940, 0xBB01, 0x7BC0, 0x7A80, 0xBA41,
    0xBE01, 0x7EC0, 0x7F80, 0xBF41, 0x7D00, 0xBDC1, 0xBC81, 0x7C40, 0xB401, 0x74C0, 0x7580, 0xB541,
    0x7700, 0xB7C1, 0xB681, 0x7640, 0x7200, 0xB2C1, 0xB381, 0x7340, 0xB101, 0x71C0, 0x7080, 0xB041,
    0x5000, 0x90C1, 0x9181, 0x5140, 0x9301, 0x53C0, 0x5280, 0x9241, 0x9601, 0x56C0, 0x5780, 0x9741,
    0x5500, 0x95C1, 0x9481, 0x5440, 0x9C01, 0x5CC0, 0x5D80, 0x9D41, 0x5F00, 0x9FC1, 0x9E81, 0x5E40,
    0x5A00, 0x9AC1, 0x9B81, 0x5B40, 0x9901, 0x59C0, 0x5880, 0x9841, 0x8801, 0x48C0, 0x4980, 0x8941,
    0x4B00, 0x8BC1, 0x8A81, 0x4A40, 0x4E00, 0x8EC1, 0x8F81, 0x4F40, 0x8D01, 0x4DC0, 0x4C80, 0x8C41,
    0x4400, 0x84C1, 0x8581, 0x4540, 0x8701, 0x47C0, 0x4680, 0x8641, 0x8201, 0x42C0, 0x4380, 0x8341,
    0x4100, 0x81C1, 0x8081, 0x4040,
];

/// Compute DWG-flavor CRC-8 (16-bit output) over `data` starting from `seed`.
///
/// The `seed` is the running state; for new calculations use 0 (or the
/// section-specific seed defined in the spec). Returns the final 16-bit
/// CRC that should match the bytes stored in the file.
pub fn crc8(seed: u16, data: &[u8]) -> u16 {
    let mut crc = seed;
    for &b in data {
        let idx = ((crc ^ b as u16) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC8_TABLE[idx];
    }
    crc
}

/// R13-R15 section-locator seeds per spec §3.2.6 — after computing the CRC
/// over the locator table, XOR with one of these constants based on how many
/// records were present:
///
/// | Count | XOR constant |
/// |-------|--------------|
/// | 3     | 0xA598       |
/// | 4     | 0x8101       |
/// | 5     | 0x3CC4       |
/// | 6     | 0x8461       |
pub fn r13_locator_seed(record_count: usize) -> Option<u16> {
    Some(match record_count {
        3 => 0xA598,
        4 => 0x8101,
        5 => 0x3CC4,
        6 => 0x8461,
        _ => return None,
    })
}

// ================================================================
// CRC-32 (standard IEEE 802.3 / zip)
// ================================================================

/// Standard IEEE 802.3 CRC-32 over `data` starting from `seed`.
///
/// The R2004+ file-header CRC uses seed=0; the calculation must include the
/// 4 CRC bytes themselves *set to 0* (see spec §4.1). Data section CRC
/// uses seed=previous-checksum per §4.2.
pub fn crc32(seed: u32, data: &[u8]) -> u32 {
    let mut crc = !seed;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB88320u32 & ((crc & 1).wrapping_neg()));
        }
    }
    !crc
}

/// R2004+ section-page checksum (spec §4.2).
///
/// This is *not* CRC-32 — it's an Adler-32-style rolling sum with the
/// interesting twist that the max chunk size is 0x15B0 before modulo.
/// The page header stores two such values: one over the (zero-CRC'd)
/// header and one over the compressed payload.
pub fn section_page_checksum(seed: u32, data: &[u8]) -> u32 {
    let mut sum1 = seed & 0xFFFF;
    let mut sum2 = seed >> 16;
    let mut remaining = data;
    while !remaining.is_empty() {
        let chunk_size = remaining.len().min(0x15B0);
        let (chunk, rest) = remaining.split_at(chunk_size);
        for &b in chunk {
            sum1 = sum1.wrapping_add(b as u32);
            sum2 = sum2.wrapping_add(sum1);
        }
        sum1 %= 0xFFF1;
        sum2 %= 0xFFF1;
        remaining = rest;
    }
    (sum2 << 16) | (sum1 & 0xFFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A CRC table has a trivially-testable invariant: index 0 must be 0
    /// (the CRC of an empty byte at state 0 stays 0).
    #[test]
    fn crc8_table_index_0_is_zero() {
        assert_eq!(CRC8_TABLE[0], 0x0000);
    }

    /// Self-consistency: running the table through a known byte sequence
    /// and comparing against a direct polynomial evaluation of the same
    /// reflected CRC-16 family (poly 0xA001 = reflect of 0x8005).
    #[test]
    fn crc8_matches_reflected_poly_a001() {
        // Bitwise-compute the CRC for "123456789" (classic CRC test vector).
        let msg = b"123456789";
        let mut bit_crc: u16 = 0;
        for &b in msg {
            bit_crc ^= b as u16;
            for _ in 0..8 {
                if bit_crc & 1 != 0 {
                    bit_crc = (bit_crc >> 1) ^ 0xA001;
                } else {
                    bit_crc >>= 1;
                }
            }
        }
        let tbl_crc = crc8(0, msg);
        assert_eq!(tbl_crc, bit_crc);
    }

    #[test]
    fn crc32_ieee_test_vector() {
        // "123456789" under IEEE 802.3 CRC-32 is 0xCBF43926.
        assert_eq!(crc32(0, b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn r13_locator_seeds_known_table() {
        assert_eq!(r13_locator_seed(3), Some(0xA598));
        assert_eq!(r13_locator_seed(6), Some(0x8461));
        assert_eq!(r13_locator_seed(99), None);
    }

    #[test]
    fn section_page_checksum_empty_is_seed() {
        assert_eq!(section_page_checksum(0, &[]), 0);
        assert_eq!(section_page_checksum(0xDEAD_BEEF, &[]), 0xDEAD_BEEF);
    }

    #[test]
    fn section_page_checksum_is_deterministic() {
        let data = b"The DWG file format version 2004 compression is a variation on LZ77.";
        let a = section_page_checksum(0, data);
        let b = section_page_checksum(0, data);
        assert_eq!(a, b);
        // Non-zero for non-empty input.
        assert_ne!(a, 0);
    }
}
