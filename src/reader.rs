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
use crate::section_map;
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
            if common.image_seeker != 0 && (common.image_seeker as u64) < bytes.len() as u64 {
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
    /// spec §5 full layout is deferred to Phase C.
    pub fn r2007_common(&self) -> Option<&CommonHeader> {
        self.r2007_common.as_ref()
    }

    /// Read the decompressed bytes of a named section.
    ///
    /// Walks the R2004-family page map + section info to locate the
    /// section by name, then decrypts each data page header, optionally
    /// LZ77-decompresses the payload, and assembles the full content in
    /// page `start_offset` order.
    ///
    /// Returns `None` if this is not an R2004-family file or the section
    /// name is not present; otherwise returns the decompressed bytes
    /// (or an error if a decrypt / decompress step fails).
    pub fn read_section(&self, name: &str) -> Option<Result<Vec<u8>>> {
        let header = self.r2004.as_ref()?;
        Some(self.read_section_r2004(header, name))
    }

    fn read_section_r2004(&self, header: &R2004Header, name: &str) -> Result<Vec<u8>> {
        let page_map = section_map::parse_page_map(&self.bytes, header)?;
        let descriptions = section_map::parse_section_info(&self.bytes, header, &page_map)?;
        let desc = descriptions
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| Error::SectionMap(format!("section {name:?} not found")))?;
        section_map::read_section_payload(&self.bytes, &page_map, desc)
    }

    /// Raw file bytes (useful for downstream tools that want to feed into
    /// decoders without a second read from disk).
    pub fn raw_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Read + parse `AcDb:SummaryInfo` into a structured [`crate::metadata::SummaryInfo`].
    pub fn summary_info(&self) -> Option<Result<crate::metadata::SummaryInfo>> {
        Some(
            self.read_section("AcDb:SummaryInfo")?
                .and_then(|bytes| crate::metadata::SummaryInfo::parse(&bytes)),
        )
    }

    /// Read + parse `AcDb:AppInfo` into a structured [`crate::metadata::AppInfo`].
    pub fn app_info(&self) -> Option<Result<crate::metadata::AppInfo>> {
        Some(
            self.read_section("AcDb:AppInfo")?
                .and_then(|bytes| crate::metadata::AppInfo::parse(&bytes)),
        )
    }

    /// Read + parse `AcDb:Preview` into a structured [`crate::metadata::Preview`]
    /// (with a separable BMP / WMF extract).
    pub fn preview(&self) -> Option<Result<crate::metadata::Preview>> {
        Some(
            self.read_section("AcDb:Preview")?
                .and_then(|bytes| crate::metadata::Preview::parse(&bytes)),
        )
    }

    /// Read + parse `AcDb:FileDepList` into a structured
    /// [`crate::metadata::FileDepList`] listing external font / image /
    /// XREF references.
    pub fn file_dep_list(&self) -> Option<Result<crate::metadata::FileDepList>> {
        Some(
            self.read_section("AcDb:FileDepList")?
                .and_then(|bytes| crate::metadata::FileDepList::parse(&bytes)),
        )
    }

    /// Walk the `AcDb:AcDbObjects` stream and return every object as a
    /// [`crate::object::RawObject`] (typed + handled, bytes preserved).
    ///
    /// This does NOT decode entity-specific fields; callers that want
    /// `Entity::Line { start, end, ... }` pass each `RawObject.raw` to
    /// a per-type decoder. The walker is version-aware and handles the
    /// R2010+ object-type encoding and the pre-section RL prefix.
    pub fn objects(&self) -> Option<Result<Vec<crate::object::RawObject>>> {
        let _ = self.r2004.as_ref()?;
        let bytes = match self.read_section("AcDb:AcDbObjects") {
            Some(Ok(b)) => b,
            Some(Err(e)) => return Some(Err(e)),
            None => return None,
        };
        Some(crate::object::ObjectWalker::new(&bytes, self.version).collect_all())
    }

    /// Read + parse the `AcDb:Handles` object map into a
    /// [`crate::handle_map::HandleMap`] for random-access object lookup
    /// by handle.
    pub fn handle_map(&self) -> Option<Result<crate::handle_map::HandleMap>> {
        Some(
            self.read_section("AcDb:Handles")?
                .and_then(|bytes| crate::handle_map::HandleMap::parse(&bytes)),
        )
    }

    /// Read + parse the `AcDb:Classes` custom class table.
    pub fn class_map(&self) -> Option<Result<crate::classes::ClassMap>> {
        let version = self.version;
        Some(
            self.read_section("AcDb:Classes")?
                .and_then(|bytes| crate::classes::ClassMap::parse(&bytes, version)),
        )
    }

    /// Read + parse the `AcDb:Header` variable table (~200 system
    /// vars). Only the raw bit-stream is captured; targeted
    /// accessors on [`HeaderVars`] can extract individual variables.
    pub fn header_vars(&self) -> Option<Result<crate::header_vars::HeaderVars>> {
        let version = self.version;
        Some(
            self.read_section("AcDb:Header")?
                .and_then(|bytes| crate::header_vars::HeaderVars::parse(&bytes, version)),
        )
    }

    /// Full handle-driven object iteration — uses `AcDb:Handles` to
    /// find every object in the file, not just the first. Returns the
    /// complete list of control objects, table entries, entities, and
    /// dictionaries.
    pub fn all_objects(&self) -> Option<Result<Vec<crate::object::RawObject>>> {
        let _ = self.r2004.as_ref()?;
        let hmap = match self.handle_map()? {
            Ok(m) => m,
            Err(e) => return Some(Err(e)),
        };
        let obj_bytes = match self.read_section("AcDb:AcDbObjects") {
            Some(Ok(b)) => b,
            Some(Err(e)) => return Some(Err(e)),
            None => return None,
        };
        Some(
            crate::object::ObjectWalker::with_handle_map(&obj_bytes, self.version, &hmap)
                .collect_all(),
        )
    }
}

/// Walk the R2004+ Section Page Map → Section Info chain and emit a
/// `Section` list with real named entries.
///
/// Phase B: LZ77 + page-map parse + section-info parse produce the complete
/// AcDb:Header / AcDb:Classes / AcDb:Handles / AcDb:AcDbObjects / etc list.
/// If the parse fails for any reason (e.g. corrupt file, format ahead of
/// this implementation), we fall back to the Phase A stub enumeration so
/// the file still opens with partial metadata.
fn extract_r2004_sections(bytes: &[u8], header: &R2004Header) -> Result<Vec<Section>> {
    // Try the full Section Map walk first.
    if let Ok(full) = extract_r2004_sections_full(bytes, header) {
        if !full.is_empty() {
            return Ok(full);
        }
    }
    // Fall back to stub enumeration.
    extract_r2004_sections_stub(bytes, header)
}

/// Phase B: full named-section enumeration via LZ77-decompressed page map
/// and section info.
fn extract_r2004_sections_full(bytes: &[u8], header: &R2004Header) -> Result<Vec<Section>> {
    let page_map = section_map::parse_page_map(bytes, header)?;
    let descriptions = section_map::parse_section_info(bytes, header, &page_map)?;

    // Build a lookup from page number to file offset.
    let mut page_offset: std::collections::HashMap<i32, u64> =
        std::collections::HashMap::with_capacity(page_map.len());
    for p in &page_map {
        if !p.is_gap {
            page_offset.insert(p.number, p.file_offset);
        }
    }

    let mut out = Vec::with_capacity(descriptions.len());
    for d in descriptions {
        // For the section's "primary" on-disk address we use the first
        // page's file offset. Callers that want to walk the per-section
        // page list can expose that via a dedicated API; for the top-level
        // `Section` we report the first-page offset.
        let first_offset = d
            .pages
            .first()
            .and_then(|p| page_offset.get(&(p.page_number as i32)))
            .copied()
            .unwrap_or(0);
        // Filter out the unnamed "Empty section" (spec §4.5 first entry).
        if d.name.is_empty() {
            continue;
        }
        out.push(Section {
            name: d.name.clone(),
            kind: SectionKind::from_r2004_name(&d.name),
            offset: first_offset,
            size: d.size,
            compressed: d.compressed == 2,
            encrypted: d.encrypted == 1,
        });
    }
    Ok(out)
}

/// Phase A fallback: peek what the raw file header reveals, probe the
/// first page at 0x100 for a known page-type tag.
fn extract_r2004_sections_stub(bytes: &[u8], header: &R2004Header) -> Result<Vec<Section>> {
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
    if header.common.image_seeker != 0 && (header.common.image_seeker as u64) < bytes.len() as u64 {
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
    if header.second_header_addr != 0 && header.second_header_addr < bytes.len() as u64 {
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
