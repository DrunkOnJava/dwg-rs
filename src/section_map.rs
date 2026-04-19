//! R2004+ Section Page Map + Section Info parsers (spec §4.4-§4.5).
//!
//! After the header is decrypted, the Section Page Map lives at file
//! offset `r2004_header.section_page_map_addr + 0x100`. The page there
//! carries a 32-byte system-section header (Sec_Mask-encrypted), then an
//! LZ77-compressed payload. Decompress, parse `(page_number, size)` pairs
//! until the sum of sizes reaches `decompressed_size`; negative page
//! numbers mark gaps with a 4-u32 trailer.
//!
//! The Section Info (a.k.a. Section Map) is a DIFFERENT system section
//! located by walking the page map to find the page whose id equals
//! `r2004_header.section_map_id`. Its decompressed content is a 5-u32
//! header + `NumDescriptions` × section description, each of which holds
//! a name, compression flags, and its own list of `(page_number,
//! data_size, start_offset)` page entries.

use crate::crc;
use crate::error::{Error, Result};
use crate::header::R2004Header;
use crate::lz77;
use byteorder::{ByteOrder, LittleEndian};

/// One row of the global Section Page Map.
///
/// Pages are numbered sequentially starting at 1. A positive `number`
/// identifies a real page; a negative `number` marks a gap in the
/// sequence (unused space left behind by a deleted/moved page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionPage {
    /// Signed 32-bit ID. Positive for real pages, negative for gaps.
    pub number: i32,
    /// On-disk size of this page in bytes (includes its 32-byte header).
    pub size: u32,
    /// Computed absolute file offset where this page begins.
    pub file_offset: u64,
    /// True for gap entries (four trailing u32s present in the source).
    pub is_gap: bool,
}

/// One named entry in the Section Info — collectively the "Section Map".
#[derive(Debug, Clone)]
pub struct SectionDescription {
    /// Total decompressed size across all pages.
    pub size: u64,
    /// How many page entries follow this description.
    pub page_count: u32,
    /// Max decompressed size of a single page of this section (usually 0x7400).
    pub max_decomp_page_size: u32,
    /// Compression flag: 1 = uncompressed, 2 = LZ77.
    pub compressed: u32,
    /// ID used to cross-reference the Section Page Map.
    pub section_id: u32,
    /// Encryption flag: 0 = none, 1 = encrypted, 2 = unknown/optional.
    pub encrypted: u32,
    /// UTF-8 readable section name (e.g., "AcDb:Header", "AcDb:Preview").
    pub name: String,
    /// Per-section page list referencing the global Section Page Map.
    pub pages: Vec<SectionPageRef>,
}

/// An entry inside a section description's per-section page list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionPageRef {
    pub page_number: u32,
    pub data_size: u32,
    pub start_offset: u64,
}

// ================================================================
// System page header (Sec_Mask-encrypted)
// ================================================================

#[derive(Debug, Clone, Copy)]
struct SystemPageHeader {
    page_type: u32,
    decomp_size: u32,
    comp_size: u32,
    comp_type: u32,
    _checksum: u32,
}

impl SystemPageHeader {
    /// Page type tags per spec §4.3.
    const PAGE_MAP_TYPE: u32 = 0x4163_0E3B;
    const SECTION_MAP_TYPE: u32 = 0x4163_003B;

    /// Parse a 20-byte system page header.
    ///
    /// Unlike the 32-byte *data* section page header in spec §4.6,
    /// system section pages (the Section Page Map and the Section Map)
    /// use a **plaintext** header — the Sec_Mask XOR is not applied.
    /// Observed against `sample_AC1032.dwg`: the raw u32 at offset
    /// `section_page_map_addr + 0x100` already equals `0x4163_0E3B`,
    /// and the following `comp_size`/`comp_type` fields parse correctly
    /// without any key derivation.
    fn parse(file_bytes: &[u8], file_offset: u64) -> Result<Self> {
        if (file_offset as usize) + 0x14 > file_bytes.len() {
            return Err(Error::Truncated {
                offset: file_offset,
                wanted: 0x14,
                len: file_bytes.len() as u64,
            });
        }
        let hdr = &file_bytes[file_offset as usize..file_offset as usize + 0x14];
        Ok(SystemPageHeader {
            page_type: LittleEndian::read_u32(&hdr[0..4]),
            decomp_size: LittleEndian::read_u32(&hdr[4..8]),
            comp_size: LittleEndian::read_u32(&hdr[8..12]),
            comp_type: LittleEndian::read_u32(&hdr[12..16]),
            _checksum: LittleEndian::read_u32(&hdr[16..20]),
        })
    }
}

// ================================================================
// Section Page Map (§4.4)
// ================================================================

/// Decompress and parse the global Section Page Map.
///
/// Returns a `Vec<SectionPage>` with computed absolute file offsets.
/// Pages are emitted in the order they appear in the map; file offsets
/// are accumulated as `0x100 + running_sum_of_sizes`.
pub fn parse_page_map(file_bytes: &[u8], header: &R2004Header) -> Result<Vec<SectionPage>> {
    let page_offset = header.section_page_map_addr + 0x100;
    let sys_hdr = SystemPageHeader::parse(file_bytes, page_offset)?;
    if sys_hdr.page_type != SystemPageHeader::PAGE_MAP_TYPE {
        return Err(Error::SectionMap(format!(
            "page map type tag 0x{:x} != expected 0x{:x}",
            sys_hdr.page_type,
            SystemPageHeader::PAGE_MAP_TYPE
        )));
    }
    if sys_hdr.comp_type != 2 {
        return Err(Error::SectionMap(format!(
            "page map comp_type {} != 2 (LZ77 expected)",
            sys_hdr.comp_type
        )));
    }
    // System page header is 0x14 bytes (spec §4.3), unlike data page
    // headers which are 0x20 bytes (§4.6).
    let payload_start = (page_offset + 0x14) as usize;
    let payload_end = payload_start + sys_hdr.comp_size as usize;
    if payload_end > file_bytes.len() {
        return Err(Error::Truncated {
            offset: payload_start as u64,
            wanted: sys_hdr.comp_size as usize,
            len: file_bytes.len() as u64,
        });
    }
    let decompressed = lz77::decompress(
        &file_bytes[payload_start..payload_end],
        Some(sys_hdr.decomp_size as usize),
    )?;
    decode_page_map_entries(&decompressed)
}

fn decode_page_map_entries(data: &[u8]) -> Result<Vec<SectionPage>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut file_offset: u64 = 0x100; // first page after the global file header
    while i + 8 <= data.len() {
        let number = LittleEndian::read_i32(&data[i..i + 4]);
        let size = LittleEndian::read_i32(&data[i + 4..i + 8]) as u32;
        i += 8;
        let is_gap = number < 0;
        if is_gap {
            // Spec §4.4: 4 extra u32 follow (parent, left, right, 0).
            if i + 16 > data.len() {
                return Err(Error::SectionMap(
                    "page map gap record truncated".to_string(),
                ));
            }
            i += 16;
        }
        out.push(SectionPage {
            number,
            size,
            file_offset,
            is_gap,
        });
        file_offset = file_offset.saturating_add(size as u64);
    }
    Ok(out)
}

// ================================================================
// Section Map / Section Info (§4.5)
// ================================================================

/// Locate the Section Map page in the already-parsed page map, decrypt
/// its header, decompress its body, and emit the named section list.
pub fn parse_section_info(
    file_bytes: &[u8],
    header: &R2004Header,
    page_map: &[SectionPage],
) -> Result<Vec<SectionDescription>> {
    // Find the page whose number == header.section_map_id.
    let sm_page = page_map
        .iter()
        .find(|p| p.number == header.section_map_id as i32)
        .ok_or_else(|| {
            Error::SectionMap(format!(
                "section_map_id {} not found in {} page map entries",
                header.section_map_id,
                page_map.len()
            ))
        })?;

    let sys_hdr = SystemPageHeader::parse(file_bytes, sm_page.file_offset)?;
    if sys_hdr.page_type != SystemPageHeader::SECTION_MAP_TYPE {
        return Err(Error::SectionMap(format!(
            "section map type 0x{:x} != expected 0x{:x}",
            sys_hdr.page_type,
            SystemPageHeader::SECTION_MAP_TYPE
        )));
    }
    if sys_hdr.comp_type != 2 {
        return Err(Error::SectionMap(format!(
            "section map comp_type {} != 2 (LZ77 expected)",
            sys_hdr.comp_type
        )));
    }
    let payload_start = (sm_page.file_offset + 0x14) as usize;
    let payload_end = payload_start + sys_hdr.comp_size as usize;
    if payload_end > file_bytes.len() {
        return Err(Error::Truncated {
            offset: payload_start as u64,
            wanted: sys_hdr.comp_size as usize,
            len: file_bytes.len() as u64,
        });
    }
    let decompressed = lz77::decompress(
        &file_bytes[payload_start..payload_end],
        Some(sys_hdr.decomp_size as usize),
    )?;
    decode_section_descriptions(&decompressed)
}

fn decode_section_descriptions(data: &[u8]) -> Result<Vec<SectionDescription>> {
    if data.len() < 0x14 {
        return Err(Error::SectionMap(format!(
            "section info shorter than 0x14-byte header: {} bytes",
            data.len()
        )));
    }
    let num_descriptions = LittleEndian::read_u32(&data[0x00..0x04]);
    // We don't validate the other header u32s — ODA writes NumDescriptions
    // at 0x10 but the spec doesn't require a specific value.
    let mut cursor = 0x14usize;
    let mut out = Vec::with_capacity(num_descriptions as usize);
    for _ in 0..num_descriptions {
        if cursor + 0x60 > data.len() {
            return Err(Error::SectionMap(format!(
                "section description #{} at offset 0x{:x} overruns buffer",
                out.len(),
                cursor
            )));
        }
        let size = LittleEndian::read_u64(&data[cursor..cursor + 8]);
        let page_count = LittleEndian::read_u32(&data[cursor + 8..cursor + 12]);
        let max_decomp = LittleEndian::read_u32(&data[cursor + 12..cursor + 16]);
        let _unknown = LittleEndian::read_u32(&data[cursor + 16..cursor + 20]);
        let compressed = LittleEndian::read_u32(&data[cursor + 20..cursor + 24]);
        let section_id = LittleEndian::read_u32(&data[cursor + 24..cursor + 28]);
        let encrypted = LittleEndian::read_u32(&data[cursor + 28..cursor + 32]);
        // Name is 64 UTF-8 bytes, NUL-padded.
        let name_bytes = &data[cursor + 32..cursor + 96];
        let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(64);
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();
        cursor += 0x60;

        // Per-section page list: page_count × {u32, u32, u64} = 16 bytes each.
        let mut pages = Vec::with_capacity(page_count as usize);
        for p in 0..page_count {
            if cursor + 16 > data.len() {
                return Err(Error::SectionMap(format!(
                    "page ref {p} of section '{name}' at 0x{cursor:x} overruns buffer"
                )));
            }
            let page_number = LittleEndian::read_u32(&data[cursor..cursor + 4]);
            let data_size = LittleEndian::read_u32(&data[cursor + 4..cursor + 8]);
            let start_offset = LittleEndian::read_u64(&data[cursor + 8..cursor + 16]);
            cursor += 16;
            pages.push(SectionPageRef {
                page_number,
                data_size,
                start_offset,
            });
        }

        out.push(SectionDescription {
            size,
            page_count,
            max_decomp_page_size: max_decomp,
            compressed,
            section_id,
            encrypted,
            name,
            pages,
        });
    }
    Ok(out)
}

// Re-export of the CRC module so downstream code doesn't have to name it
// just to get a checksum; mirrors the shape of the public API.
#[allow(unused_imports)]
pub(crate) use crc::section_page_checksum as _unused_export;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_map_empty_input() {
        let pages = decode_page_map_entries(&[]).unwrap();
        assert!(pages.is_empty());
    }

    #[test]
    fn page_map_single_positive_entry() {
        // number = 1, size = 0x7400
        let mut data = Vec::new();
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&0x7400u32.to_le_bytes());
        let pages = decode_page_map_entries(&data).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].number, 1);
        assert_eq!(pages[0].size, 0x7400);
        assert_eq!(pages[0].file_offset, 0x100);
        assert!(!pages[0].is_gap);
    }

    #[test]
    fn page_map_gap_entry_consumes_four_trailing_u32s() {
        // number = -2 (gap), size = 0x1000, then parent=9 left=10 right=11 end=0
        let mut data = Vec::new();
        data.extend_from_slice(&(-2i32).to_le_bytes());
        data.extend_from_slice(&0x1000u32.to_le_bytes());
        data.extend_from_slice(&9u32.to_le_bytes());
        data.extend_from_slice(&10u32.to_le_bytes());
        data.extend_from_slice(&11u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        // then a valid positive entry to verify cursor advanced right
        data.extend_from_slice(&3i32.to_le_bytes());
        data.extend_from_slice(&0x200u32.to_le_bytes());
        let pages = decode_page_map_entries(&data).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].number, -2);
        assert!(pages[0].is_gap);
        assert_eq!(pages[0].size, 0x1000);
        assert_eq!(pages[1].number, 3);
        assert_eq!(pages[1].file_offset, 0x100 + 0x1000);
    }

    #[test]
    fn section_info_empty_descriptions() {
        // Header with NumDescriptions = 0 and 4 more u32 padding.
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&0x7400u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        let descs = decode_section_descriptions(&data).unwrap();
        assert!(descs.is_empty());
    }
}
