//! Top-level `DwgFile` API — the entry point most callers use.
//!
//! Opens a DWG file, detects its version, and exposes a unified interface
//! over both the R13-R15 flat-locator family and the R2004+ section-map
//! family.

use crate::cipher;
use crate::crc;
use crate::error::{Error, Result};
use crate::header::{CommonHeader, R13R15Header, R2004Header};
use crate::section::{Section, SectionKind};
use crate::version::Version;
use byteorder::{ByteOrder, LittleEndian};
use std::fs;
use std::path::Path;

/// A parsed DWG file held entirely in memory.
///
/// For Phase A we read the full file into a `Vec<u8>`. That is fine for
/// the typical 10 KB - 10 MB CAD drawing. Files over ~50 MB would benefit
/// from `memmap2` backing; adding that is deferred to Phase B because it
/// changes lifetimes on returned `Section` payloads.
#[derive(Debug, Clone)]
pub struct DwgFile {
    bytes: Vec<u8>,
    version: Version,
    sections: Vec<Section>,
    /// Populated for R2004 / R2010 / R2013 / R2018 (the R2004 family).
    r2004: Option<R2004Header>,
    /// Populated only for R13-R15 files.
    r13: Option<R13R15Header>,
    /// Populated only for R2007 — Phase A records just the common header
    /// and defers full layout parsing (spec §5, 33 pages) to Phase B.
    r2007_common: Option<CommonHeader>,
}

impl DwgFile {
    /// Open a DWG file at `path`, read it into memory, and parse enough
    /// metadata to enumerate sections.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = fs::read(path)?;
        Self::from_bytes(bytes)
    }

    /// Parse an already-loaded byte buffer.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < 6 {
            return Err(Error::Truncated {
                offset: 0,
                wanted: 6,
                len: bytes.len() as u64,
            });
        }
        let mut magic = [0u8; 6];
        magic.copy_from_slice(&bytes[0..6]);
        let version = Version::from_magic(&magic)?;

        if version.is_r13_r15() {
            let header = R13R15Header::parse(&bytes)?;
            let sections = header.into_sections();
            Ok(Self {
                bytes,
                version,
                sections,
                r2004: None,
                r13: Some(header),
                r2007_common: None,
            })
        } else if version.is_r2004_family() {
            let header = R2004Header::parse(&bytes)?;
            let sections = extract_r2004_sections(&bytes, &header)?;
            Ok(Self {
                bytes,
                version,
                sections,
                r2004: Some(header),
                r13: None,
                r2007_common: None,
            })
        } else {
            // R2007: spec §5, distinct layout not yet implemented.
            // Parse the common prefix so metadata tools can still identify
            // the file, and emit a placeholder "Preview" section from the
            // image seeker if present.
            let common = CommonHeader::parse(&bytes)?;
            let mut sections = Vec::new();
            if common.image_seeker != 0
                && (common.image_seeker as u64) < bytes.len() as u64
            {
                sections.push(Section {
                    name: "AcDb:Preview".to_string(),
                    kind: SectionKind::Preview,
                    offset: common.image_seeker as u64 + 0x20,
                    size: 0,
                    compressed: false,
                    encrypted: false,
                });
            }
            sections.push(Section {
                name: "R2007-deferred".to_string(),
                kind: SectionKind::Unknown,
                offset: 0x80,
                size: (bytes.len() as u64).saturating_sub(0x80),
                compressed: true,
                encrypted: true,
            });
            Ok(Self {
                bytes,
                version,
                sections,
                r2004: None,
                r13: None,
                r2007_common: Some(common),
            })
        }
    }

    /// Detected DWG format version.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Total file size in bytes.
    pub fn file_size(&self) -> u64 {
        self.bytes.len() as u64
    }

    /// All enumerated sections, in on-disk order.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Find the first section with a given kind (or `None` if absent).
    pub fn section_of_kind(&self, kind: SectionKind) -> Option<&Section> {
        self.sections.iter().find(|s| s.kind == kind)
    }

    /// Find a section by name (case-sensitive, exact).
    pub fn section_by_name(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// Parsed R2004+ header, if applicable.
    pub fn r2004_header(&self) -> Option<&R2004Header> {
        self.r2004.as_ref()
    }

    /// Parsed R13-R15 header, if applicable.
    pub fn r13_header(&self) -> Option<&R13R15Header> {
        self.r13.as_ref()
    }

    /// Parsed common header for R2007 files — a minimal parse because
    /// spec §5 full layout is deferred to Phase B.
    pub fn r2007_common(&self) -> Option<&CommonHeader> {
        self.r2007_common.as_ref()
    }

    /// Raw file bytes (useful for downstream tools that want to feed into
    /// decoders without a second read from disk).
    pub fn raw_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Walk the R2004+ Section Page Map → Section Info chain and emit a
/// `Section` list.
///
/// This does *not* decompress the page map. Instead, it treats the map as
/// an opaque payload located at the file address given in the decrypted
/// header. For Phase A, we enumerate the *page-map bounding box* (start
/// offset + size) as a single `SystemSection`, plus report the named data
/// sections that the header directly reveals (summary info + VBA project).
///
/// Full walk of the section name list requires LZ77 decompression, which
/// is Phase B. The contract is that once we implement that, this function
/// replaces its stub output with the complete named list — no callers need
/// to change.
fn extract_r2004_sections(bytes: &[u8], header: &R2004Header) -> Result<Vec<Section>> {
    let mut out = Vec::new();

    // 1. Section Page Map — lives at header.section_page_map_addr + 0x100.
    //    This is a system section.
    let page_map_offset = header.section_page_map_addr + 0x100;
    out.push(Section {
        name: "SectionPageMap".to_string(),
        kind: SectionKind::SystemSection,
        offset: page_map_offset,
        size: 0, // unknown without decompressing
        compressed: true,
        encrypted: false,
    });

    // 2. Summary Info — if the file_header pointer is non-zero.
    if header.summary_info_addr != 0 {
        out.push(Section {
            name: "AcDb:SummaryInfo".to_string(),
            kind: SectionKind::SummaryInfo,
            offset: header.summary_info_addr as u64 + 0x20,
            size: 0,
            compressed: false,
            encrypted: false,
        });
    }

    // 3. VBA Project — optional.
    if header.vba_project_addr != 0 {
        out.push(Section {
            name: "AcDb:VBAProject".to_string(),
            kind: SectionKind::VbaProject,
            offset: header.vba_project_addr as u64 + 0x20,
            size: 0,
            compressed: false,
            encrypted: false,
        });
    }

    // 4. Preview — taken from the *plaintext* header byte 0x0D (image_seeker).
    if header.common.image_seeker != 0
        && (header.common.image_seeker as u64) < bytes.len() as u64
    {
        out.push(Section {
            name: "AcDb:Preview".to_string(),
            kind: SectionKind::Preview,
            offset: header.common.image_seeker as u64 + 0x20,
            size: 0,
            compressed: false,
            encrypted: false,
        });
    }

    // 5. Second header copy — end-of-file redundant header, diagnostic only.
    if header.second_header_addr != 0
        && header.second_header_addr < bytes.len() as u64
    {
        out.push(Section {
            name: "SecondHeader".to_string(),
            kind: SectionKind::SystemSection,
            offset: header.second_header_addr,
            size: 0,
            compressed: false,
            encrypted: false,
        });
    }

    // 6. Sniff for the page-at-offset-0x100 — that's always the first
    //    user-visible page. We can peek its 32-byte header, decrypt it with
    //    the section_page_mask, and recover its decompressed size.
    if bytes.len() >= 0x120 {
        if let Some(s) = sniff_first_page_at_0x100(bytes) {
            out.push(s);
        }
    }

    Ok(out)
}

/// Peek the 32-byte encrypted header of the section page at file offset
/// 0x100. Returns a `Section` describing the page bounds if the header
/// passes a sanity check; otherwise `None` (we don't fail hard — the file
/// may be unusual).
fn sniff_first_page_at_0x100(bytes: &[u8]) -> Option<Section> {
    let hdr_off = 0x100usize;
    if bytes.len() < hdr_off + 0x20 {
        return None;
    }
    let mut hdr = [0u8; 0x20];
    hdr.copy_from_slice(&bytes[hdr_off..hdr_off + 0x20]);
    // Decrypt with the Sec_Mask (XOR against 0x4164536B ^ offset).
    let mask = cipher::section_page_mask(hdr_off as u32);
    for chunk in hdr.chunks_exact_mut(4) {
        let v = LittleEndian::read_u32(chunk);
        LittleEndian::write_u32(chunk, v ^ mask);
    }
    let page_type = LittleEndian::read_u32(&hdr[0..4]);
    // Known type tags (spec §4.3):
    //   system-section page map: 0x41630E3B
    //   system-section section map: 0x4163003B
    //   data-section page:       0x4163043B
    let known = matches!(page_type, 0x4163_0E3B | 0x4163_003B | 0x4163_043B);
    if !known {
        return None;
    }
    let decomp_size = LittleEndian::read_u32(&hdr[4..8]) as u64;
    let comp_size = LittleEndian::read_u32(&hdr[8..12]) as u64;
    let _comp_type = LittleEndian::read_u32(&hdr[12..16]);
    let _checksum = LittleEndian::read_u32(&hdr[16..20]);
    let kind = if page_type == 0x4163_043B {
        SectionKind::Unknown // this is a named-data section; we learn the name later
    } else {
        SectionKind::SystemSection
    };
    Some(Section {
        name: match page_type {
            0x4163_0E3B => "SystemSection(PageMap)".to_string(),
            0x4163_003B => "SystemSection(SectionMap)".to_string(),
            0x4163_043B => format!("DataPage(decomp={})", decomp_size),
            _ => "SystemSection".to_string(),
        },
        kind,
        offset: hdr_off as u64 + 0x20,
        size: comp_size,
        compressed: true,
        encrypted: true,
    })
}

/// Compute the CRC-32 over the R2004+ decrypted header block, setting the
/// stored CRC bytes (0x68-0x6B) to zero first per spec §4.1.
///
/// Returns `(expected, actual)`. A mismatch indicates a corrupt file or
/// a cipher-key error.
pub fn validate_r2004_header_crc(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 0xEC {
        return Err(Error::Truncated {
            offset: 0,
            wanted: 0xEC,
            len: bytes.len() as u64,
        });
    }
    let mut block = [0u8; cipher::MAGIC_LEN];
    block.copy_from_slice(&bytes[0x80..0x80 + cipher::MAGIC_LEN]);
    cipher::xor_in_place(&mut block);
    let expected = LittleEndian::read_u32(&block[0x68..0x6C]);
    for b in &mut block[0x68..0x6C] {
        *b = 0;
    }
    let actual = crc::crc32(0, &block);
    Ok((expected, actual))
}
