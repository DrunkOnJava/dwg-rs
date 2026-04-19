//! File header parsers for both DWG format families.
//!
//! The two families diverge at offset 0x0B:
//!
//! - **R13-R15** (§3.2): flat section-locator list, plain CRC.
//! - **R2004+** (§4.1): XOR-encrypted 0x6C-byte block containing a section
//!   page map pointer and other bookkeeping.

use crate::cipher;
use crate::error::{Error, Result};
use crate::section::{Section, SectionKind};
use crate::version::Version;
use byteorder::{ByteOrder, LittleEndian};

/// The parsed 128-byte prefix all DWG files share.
///
/// Fields above offset 0x13 have slightly different meanings per family
/// but the layout at byte level is consistent.
#[derive(Debug, Clone)]
pub struct CommonHeader {
    pub version: Version,
    /// Byte 0x0B — maintenance release version.
    pub maint_version: u8,
    /// Bytes 0x0D-0x10 — image seeker (R13-R15) / preview address (R2004+).
    pub image_seeker: u32,
    /// Bytes 0x13-0x14 — DWG codepage (raw short).
    pub codepage: u16,
}

impl CommonHeader {
    /// Parse the first 0x15 bytes that both families agree on.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 0x15 {
            return Err(Error::Truncated {
                offset: 0,
                wanted: 0x15,
                len: bytes.len() as u64,
            });
        }
        let mut magic = [0u8; 6];
        magic.copy_from_slice(&bytes[0..6]);
        let version = Version::from_magic(&magic)?;
        Ok(CommonHeader {
            version,
            maint_version: bytes[0x0B],
            image_seeker: LittleEndian::read_u32(&bytes[0x0D..0x11]),
            codepage: LittleEndian::read_u16(&bytes[0x13..0x15]),
        })
    }
}

// ================================================================
// R13-R15 header (§3.2)
// ================================================================

/// An R13-R15 section-locator record — 9 bytes on disk: one u8 record
/// number, one u32 LE seeker (absolute offset), one u32 LE size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocatorRecord {
    pub number: u8,
    pub seeker: u32,
    pub size: u32,
}

impl LocatorRecord {
    pub const SIZE: usize = 9;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::SIZE {
            return Err(Error::SectionLocator(format!(
                "locator record needs {} bytes, got {}",
                Self::SIZE,
                bytes.len()
            )));
        }
        Ok(LocatorRecord {
            number: bytes[0],
            seeker: LittleEndian::read_u32(&bytes[1..5]),
            size: LittleEndian::read_u32(&bytes[5..9]),
        })
    }
}

/// Fully-parsed R13-R15 header: common prefix + section locator table.
#[derive(Debug, Clone)]
pub struct R13R15Header {
    pub common: CommonHeader,
    pub locator_count: u32,
    pub locators: Vec<LocatorRecord>,
}

impl R13R15Header {
    /// Spec §3.2.6: at offset 0x15 is a u32 count, followed by that many
    /// 9-byte locator records.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let common = CommonHeader::parse(bytes)?;
        if bytes.len() < 0x19 {
            return Err(Error::Truncated {
                offset: 0x15,
                wanted: 4,
                len: bytes.len() as u64,
            });
        }
        let locator_count = LittleEndian::read_u32(&bytes[0x15..0x19]);
        let needed = 0x19 + locator_count as usize * LocatorRecord::SIZE;
        if bytes.len() < needed {
            return Err(Error::Truncated {
                offset: 0x19,
                wanted: locator_count as usize * LocatorRecord::SIZE,
                len: bytes.len() as u64,
            });
        }
        let mut locators = Vec::with_capacity(locator_count as usize);
        for i in 0..locator_count as usize {
            let start = 0x19 + i * LocatorRecord::SIZE;
            let end = start + LocatorRecord::SIZE;
            locators.push(LocatorRecord::parse(&bytes[start..end])?);
        }
        Ok(R13R15Header {
            common,
            locator_count,
            locators,
        })
    }

    /// Convert locator records into the generic `Section` list the reader
    /// exposes to callers.
    pub fn into_sections(&self) -> Vec<Section> {
        self.locators
            .iter()
            .map(|r| Section {
                name: match r.number {
                    0 => "HEADER".to_string(),
                    1 => "CLASSES".to_string(),
                    2 => "OBJECT_MAP".to_string(),
                    3 => "UNKNOWN_C3".to_string(),
                    4 => "MEASUREMENT".to_string(),
                    n => format!("RECORD_{n}"),
                },
                kind: SectionKind::from_r13_record(r.number),
                offset: r.seeker as u64,
                size: r.size as u64,
                compressed: false,
                encrypted: false,
            })
            .collect()
    }
}

// ================================================================
// R2004+ header (§4.1)
// ================================================================

/// The 0x6C-byte decrypted header payload at file offset 0x80.
///
/// Many fields are "ODA writes 0" sentinels; we preserve them so
/// downstream tools can audit for corruption.
#[derive(Debug, Clone)]
pub struct R2004Header {
    pub common: CommonHeader,
    /// Bytes 0x18-0x1B of the plaintext header (pre-encrypted block).
    pub security_flags: u32,
    /// Bytes 0x20-0x23 — summary info address (file offset = value + 0x20).
    pub summary_info_addr: u32,
    /// Bytes 0x24-0x27 — VBA project address (0 if absent).
    pub vba_project_addr: u32,

    // === Decrypted 0x6C block (spec §4.1 table) ===
    /// "AcFssFcAJMB" file-id string at offset 0x00 of the decrypted block.
    pub file_id: [u8; 12],
    /// Decrypted 0x28 — last section page ID.
    pub last_section_page_id: u32,
    /// Decrypted 0x2C — last section-page end address (u64).
    pub last_section_page_end: u64,
    /// Decrypted 0x34 — second-header data address at end of file.
    pub second_header_addr: u64,
    /// Decrypted 0x3C — gap amount.
    pub gap_amount: u32,
    /// Decrypted 0x40 — section page amount.
    pub section_page_amount: u32,
    /// Decrypted 0x50 — Section Page Map ID.
    pub section_page_map_id: u32,
    /// Decrypted 0x54 — Section Page Map address (u64; add 0x100 for file offset).
    pub section_page_map_addr: u64,
    /// Decrypted 0x5C — Section Map ID.
    pub section_map_id: u32,
    /// Decrypted 0x60 — section-page array size.
    pub section_page_array_size: u32,
    /// Decrypted 0x64 — gap array size.
    pub gap_array_size: u32,
    /// Decrypted 0x68 — CRC-32 over the 0x6C block with CRC bytes zeroed.
    pub crc32_stored: u32,
}

impl R2004Header {
    /// Parse the first 0xEC bytes of an R2004+ DWG file.
    ///
    /// `bytes` must start at file offset 0. The function does:
    /// 1. Parse the plaintext 0x80 bytes.
    /// 2. Copy the encrypted 0x6C bytes into a scratch buffer.
    /// 3. XOR-decrypt in place against the magic sequence.
    /// 4. Read structural fields out of the decrypted block.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 0xEC {
            return Err(Error::Truncated {
                offset: 0,
                wanted: 0xEC,
                len: bytes.len() as u64,
            });
        }
        let common = CommonHeader::parse(bytes)?;
        if !common.version.is_r2004_family() {
            return Err(Error::R2004Decrypt(format!(
                "expected R2004-family version (R2004/R2010/R2013/R2018), got {}. \
                 R2007 has its own layout (spec §5) not yet implemented.",
                common.version
            )));
        }
        let security_flags = LittleEndian::read_u32(&bytes[0x18..0x1C]);
        let summary_info_addr = LittleEndian::read_u32(&bytes[0x20..0x24]);
        let vba_project_addr = LittleEndian::read_u32(&bytes[0x24..0x28]);

        // Decrypt the 0x6C block in place on a copy.
        let mut block = [0u8; cipher::MAGIC_LEN];
        block.copy_from_slice(&bytes[0x80..0x80 + cipher::MAGIC_LEN]);
        cipher::xor_in_place(&mut block);

        let mut file_id = [0u8; 12];
        file_id.copy_from_slice(&block[0x00..0x0C]);

        // Minimal sanity check: the file ID in R2004+ is always
        // "AcFssFcAJMB" followed by a NUL. If decryption fails, this
        // string will be garbled and the caller can bail with context.
        if &file_id[..11] != b"AcFssFcAJMB" {
            return Err(Error::R2004Decrypt(format!(
                "decrypted file ID mismatch: expected b\"AcFssFcAJMB\\0\", got {:?}",
                file_id
            )));
        }

        Ok(R2004Header {
            common,
            security_flags,
            summary_info_addr,
            vba_project_addr,
            file_id,
            last_section_page_id: LittleEndian::read_u32(&block[0x28..0x2C]),
            last_section_page_end: LittleEndian::read_u64(&block[0x2C..0x34]),
            second_header_addr: LittleEndian::read_u64(&block[0x34..0x3C]),
            gap_amount: LittleEndian::read_u32(&block[0x3C..0x40]),
            section_page_amount: LittleEndian::read_u32(&block[0x40..0x44]),
            section_page_map_id: LittleEndian::read_u32(&block[0x50..0x54]),
            section_page_map_addr: LittleEndian::read_u64(&block[0x54..0x5C]),
            section_map_id: LittleEndian::read_u32(&block[0x5C..0x60]),
            section_page_array_size: LittleEndian::read_u32(&block[0x60..0x64]),
            gap_array_size: LittleEndian::read_u32(&block[0x64..0x68]),
            crc32_stored: LittleEndian::read_u32(&block[0x68..0x6C]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_header_truncated() {
        let bytes = b"AC1015"; // just the magic, nothing else
        assert!(matches!(
            CommonHeader::parse(bytes),
            Err(Error::Truncated { .. })
        ));
    }

    #[test]
    fn locator_record_round_trip() {
        let raw = [0x01u8, 0x78, 0x56, 0x34, 0x12, 0x34, 0x12, 0x00, 0x00];
        let r = LocatorRecord::parse(&raw).unwrap();
        assert_eq!(r.number, 1);
        assert_eq!(r.seeker, 0x1234_5678);
        assert_eq!(r.size, 0x0000_1234);
    }
}
