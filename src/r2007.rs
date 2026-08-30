//! R2007 (`AC1021`) container — ODA spec §5.1 - §5.4.
//!
//! R2007 is the one release between the R13-R15 flat-locator layout and the
//! R2004 page-map layout that has a container of its own. It keeps the
//! *idea* of pages and sections but replaces every mechanism:
//!
//! | layer | R2004 / R2010+ (§4) | R2007 (§5) |
//! |---|---|---|
//! | file header | 0x6C bytes at 0x80, XOR'd against the magic sequence | 0x400 bytes at 0x80, Reed-Solomon (255, 239) + LZ (§5.10) |
//! | page header | 32-byte `Sec_Mask`-XOR'd header per page | none — pages are bare |
//! | integrity | CRC-8 / CRC-32 | 64-bit CRCs + a 32-bit page checksum (§5.4.1) |
//! | compression | LZ77 §4.7 | a different LZ variant, §5.10 |
//! | system pages | plain | Reed-Solomon (255, 239) interleaved, data repeated |
//! | data pages | plain | Reed-Solomon (255, 251) interleaved when `encoding == 4` |
//!
//! # What this module implements
//!
//! Enough of §5.1 - §5.4 to name every section and hand back its
//! decompressed bytes:
//!
//! 1. [`FileHeader::parse`] — Reed-Solomon-decode the 0x400-byte block at
//!    0x80 (three interleaved codewords, `factor of 3`), then §5.10-decompress
//!    the `ComprLen` bytes at offset 0x20 into the fixed 0x110-byte header.
//! 2. [`PageMap::parse`] — load the system page at `PagesMapOffset + 0x480`
//!    and accumulate its `(size, id)` pairs into page offsets, exactly as the
//!    loop §5.2 prints.
//! 3. [`SectionMap::parse`] — load the system page whose id is
//!    `SectionsMapId` and read the per-section descriptors + page lists.
//! 4. [`read_section`] — concatenate a section's pages, de-interleaving the
//!    Reed-Solomon codewords and decompressing each page as its descriptor
//!    says.
//!
//! Password-protected files are out of scope: a section descriptor with a
//! non-zero `encryption` for its *data* is reported as
//! [`Error::Unsupported`] rather than decoded incorrectly.
//!
//! # Measured against the corpus
//!
//! On `line_2007.dwg`, `arc_2007.dwg` and `circle_2007.dwg` the decoded file
//! header reproduces every constant §5.2 documents — `Header size` `0x70`,
//! the four "normally" fields `0x20` / `0x40` / `0xf800` / `4` / `1`, and
//! `StreamVersion` `0x60100` — and its `File size` field equals the file's
//! actual byte count (66304 / 66304 / 66560). The section map yields
//! thirteen named sections whose `hashcode` matches the §5.2 table for all
//! twelve sections that table documents, plus `AcDb:AppInfoHistory` (which
//! the table does not list — it is written by AutoCAD, not by ODA). See
//! `examples/probe_r2007_container.rs`, which prints all of it.
//!
//! # Reed-Solomon: de-interleave, do not correct
//!
//! §5.13 specifies (255, 239) for system pages and (255, 251) for data
//! pages, interleaved across as many codewords as the data needs. A file
//! that is not corrupt needs no error correction — only the *layout*
//! matters, so [`deinterleave`] simply gathers each codeword's message
//! bytes. [`crate::reed_solomon`] remains the repair path for a file whose
//! codewords do not agree.

use crate::error::{Error, Result};
use crate::lz77::DecompressLimits;
use crate::r21_lz;
use byteorder::{ByteOrder, LittleEndian};

/// Byte offset of the R2007 file header block (§5.2).
pub const FILE_HEADER_OFFSET: usize = 0x80;

/// Size of the R2007 file header block on disk, including its trailing
/// 0x28 bytes of check data (§5.2).
pub const FILE_HEADER_PAGE_SIZE: usize = 0x400;

/// Decompressed size of the R2007 file header (§5.2 — "the decompressed
/// size is a fixed 0x110").
pub const FILE_HEADER_SIZE: usize = 0x110;

/// Reed-Solomon codeword size for both R2007 configurations (§5.13).
pub const RS_CODEWORD: usize = 255;

/// Reed-Solomon message size for system pages — (255, 239) (§5.13).
pub const RS_SYSTEM_MESSAGE: usize = 239;

/// Reed-Solomon message size for data pages — (255, 251) (§5.13).
pub const RS_DATA_MESSAGE: usize = 251;

/// Every page offset in the page map is relative to the first data page
/// map, which sits immediately after the 0x80-byte meta data and the
/// 0x400-byte file header (§5.2 — "add 0x480 to get stream position").
pub const PAGE_BASE: usize = 0x480;

/// CRC block size the R2007 writer pads data to before encoding (§5.3).
const CRC_BLOCK: usize = 8;

/// Page-start alignment (§5.3 `PageAlignSize`).
const PAGE_ALIGN: usize = 0x20;

/// Defensive cap on the number of pages a page map may declare. A real
/// drawing uses tens; the largest corpus file uses 19.
const MAX_PAGES: usize = 1 << 20;

/// Defensive cap on the number of sections a section map may declare.
const MAX_SECTIONS: usize = 4096;

/// Round `n` up to a multiple of `a` (`a` must be a power of two).
fn align_up(n: usize, a: usize) -> usize {
    n.div_ceil(a) * a
}

/// The system-page size §5.3.1's `GetSystemPageSize` computes from a
/// decompressed data size.
///
/// The page must hold the Reed-Solomon encoding of the aligned data at
/// least twice, with a 0x400-byte floor and 0x20-byte alignment.
pub fn system_page_size(data_size: usize) -> usize {
    let aligned = align_up(data_size, CRC_BLOCK);
    let page = aligned
        .saturating_mul(2)
        .div_ceil(RS_SYSTEM_MESSAGE)
        .saturating_mul(RS_CODEWORD);
    if page < 0x400 {
        0x400
    } else {
        align_up(page, PAGE_ALIGN)
    }
}

/// Gather the message bytes of `blocks` interleaved Reed-Solomon codewords
/// out of `page` (§5.13.2).
///
/// Codeword `j`'s bytes occupy positions `blocks * i + j`; the first
/// `message` of them are data and the rest are parity. Returns
/// `blocks * message` bytes, or [`Error::Truncated`] when `page` does not
/// hold `blocks * 255` bytes.
pub fn deinterleave(page: &[u8], blocks: usize, message: usize) -> Result<Vec<u8>> {
    let needed = blocks.saturating_mul(RS_CODEWORD);
    if page.len() < needed {
        return Err(Error::Truncated {
            offset: 0,
            wanted: needed,
            len: page.len() as u64,
        });
    }
    let mut out = Vec::with_capacity(blocks.saturating_mul(message));
    for j in 0..blocks {
        for i in 0..message {
            out.push(page[i * blocks + j]);
        }
    }
    Ok(out)
}

/// The 0x110-byte R2007 file header (§5.2), read as 34 little-endian
/// `u64` fields.
///
/// Field names follow the spec's table verbatim. The five `unknown_*`
/// fields carry the spec's own "normally" values on every corpus file and
/// are surfaced so a caller can audit them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileHeader {
    /// 0x00 — header size, normally `0x70`.
    pub header_size: u64,
    /// 0x08 — total file size in bytes.
    pub file_size: u64,
    /// 0x10 — CRC-64 of the compressed page map.
    pub pages_map_crc_compressed: u64,
    /// 0x18 — page-map repeat count used by the Reed-Solomon encoding.
    pub pages_map_correction_factor: u64,
    /// 0x20 — CRC seed for the page map.
    pub pages_map_crc_seed: u64,
    /// 0x28 — offset of the second page-map copy, relative to [`PAGE_BASE`].
    pub pages_map2_offset: u64,
    /// 0x30 — page id of the second page-map copy.
    pub pages_map2_id: u64,
    /// 0x38 — offset of the page map, relative to [`PAGE_BASE`].
    pub pages_map_offset: u64,
    /// 0x40 — page id of the page map.
    pub pages_map_id: u64,
    /// 0x48 — offset of the second file-header copy.
    pub header2_offset: u64,
    /// 0x50 — compressed size of the page map.
    pub pages_map_size_compressed: u64,
    /// 0x58 — decompressed size of the page map.
    pub pages_map_size_uncompressed: u64,
    /// 0x60 — number of pages in the page map.
    pub pages_amount: u64,
    /// 0x68 — largest page id used.
    pub pages_max_id: u64,
    /// 0x70 — unknown, normally `0x20`.
    pub unknown_0x70: u64,
    /// 0x78 — unknown, normally `0x40`.
    pub unknown_0x78: u64,
    /// 0x80 — CRC-64 of the decompressed page map.
    pub pages_map_crc_uncompressed: u64,
    /// 0x88 — unknown, normally `0xF800`.
    pub unknown_0x88: u64,
    /// 0x90 — unknown, normally `4`.
    pub unknown_0x90: u64,
    /// 0x98 — unknown, normally `1`.
    pub unknown_0x98: u64,
    /// 0xA0 — number of sections plus one.
    pub sections_amount: u64,
    /// 0xA8 — CRC-64 of the decompressed section map.
    pub sections_map_crc_uncompressed: u64,
    /// 0xB0 — compressed size of the section map.
    pub sections_map_size_compressed: u64,
    /// 0xB8 — page id of the second section-map copy.
    pub sections_map2_id: u64,
    /// 0xC0 — page id of the section map.
    pub sections_map_id: u64,
    /// 0xC8 — decompressed size of the section map.
    pub sections_map_size_uncompressed: u64,
    /// 0xD0 — CRC-64 of the compressed section map.
    pub sections_map_crc_compressed: u64,
    /// 0xD8 — section-map repeat count used by the Reed-Solomon encoding.
    pub sections_map_correction_factor: u64,
    /// 0xE0 — CRC seed for the section map.
    pub sections_map_crc_seed: u64,
    /// 0xE8 — stream version, normally `0x60100`.
    pub stream_version: u64,
    /// 0xF0 — file-wide CRC seed.
    pub crc_seed: u64,
    /// 0xF8 — the §5.11 random encoding of [`Self::crc_seed`].
    pub crc_seed_encoded: u64,
    /// 0x100 — seed of the §5.11 random encoder.
    pub random_seed: u64,
    /// 0x108 — CRC-64 over the header itself.
    pub header_crc64: u64,
}

impl FileHeader {
    /// Parse the R2007 file header out of a whole-file byte buffer (§5.2).
    ///
    /// Reads the 0x400-byte block at [`FILE_HEADER_OFFSET`], gathers the
    /// three interleaved (255, 239) codewords, then decompresses the
    /// `ComprLen` bytes at offset 0x20 of the result into
    /// [`FILE_HEADER_SIZE`] bytes. A negative `ComprLen` means the payload
    /// is stored, per the spec's note.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let end = FILE_HEADER_OFFSET + FILE_HEADER_PAGE_SIZE;
        if bytes.len() < end {
            return Err(Error::Truncated {
                offset: FILE_HEADER_OFFSET as u64,
                wanted: FILE_HEADER_PAGE_SIZE,
                len: bytes.len() as u64,
            });
        }
        // §5.2: "The first 0x3D8 bytes should be decoded using
        // Reed-Solomon (255, 239) decoding, with a factor of 3."
        let decoded = deinterleave(&bytes[FILE_HEADER_OFFSET..end], 3, RS_SYSTEM_MESSAGE)?;
        if decoded.len() < 0x20 {
            return Err(Error::SectionMap(
                "R2007 file header: Reed-Solomon block shorter than its 0x20-byte prefix".into(),
            ));
        }
        let compr_len = LittleEndian::read_i32(&decoded[0x18..0x1C]);
        let plain = if compr_len < 0 {
            let stored = compr_len.unsigned_abs() as usize;
            decoded
                .get(0x20..0x20 + stored)
                .ok_or_else(|| {
                    Error::SectionMap(
                        "R2007 file header: stored payload runs past the decoded block".into(),
                    )
                })?
                .to_vec()
        } else {
            let compressed = decoded
                .get(0x20..0x20 + compr_len as usize)
                .ok_or_else(|| {
                    Error::SectionMap(
                        "R2007 file header: ComprLen runs past the Reed-Solomon block".into(),
                    )
                })?;
            r21_lz::decompress(compressed, FILE_HEADER_SIZE, DecompressLimits::default())?
        };
        if plain.len() < FILE_HEADER_SIZE {
            return Err(Error::SectionMap(format!(
                "R2007 file header decompressed to {} bytes, expected {FILE_HEADER_SIZE}",
                plain.len()
            )));
        }
        Ok(Self::from_plain(&plain))
    }

    /// Read the 34 little-endian `u64` fields out of a decompressed
    /// 0x110-byte header payload.
    fn from_plain(p: &[u8]) -> Self {
        let q = |i: usize| LittleEndian::read_u64(&p[i * 8..i * 8 + 8]);
        Self {
            header_size: q(0),
            file_size: q(1),
            pages_map_crc_compressed: q(2),
            pages_map_correction_factor: q(3),
            pages_map_crc_seed: q(4),
            pages_map2_offset: q(5),
            pages_map2_id: q(6),
            pages_map_offset: q(7),
            pages_map_id: q(8),
            header2_offset: q(9),
            pages_map_size_compressed: q(10),
            pages_map_size_uncompressed: q(11),
            pages_amount: q(12),
            pages_max_id: q(13),
            unknown_0x70: q(14),
            unknown_0x78: q(15),
            pages_map_crc_uncompressed: q(16),
            unknown_0x88: q(17),
            unknown_0x90: q(18),
            unknown_0x98: q(19),
            sections_amount: q(20),
            sections_map_crc_uncompressed: q(21),
            sections_map_size_compressed: q(22),
            sections_map2_id: q(23),
            sections_map_id: q(24),
            sections_map_size_uncompressed: q(25),
            sections_map_crc_compressed: q(26),
            sections_map_correction_factor: q(27),
            sections_map_crc_seed: q(28),
            stream_version: q(29),
            crc_seed: q(30),
            crc_seed_encoded: q(31),
            random_seed: q(32),
            header_crc64: q(33),
        }
    }
}

/// Load one **system page** (§5.3): Reed-Solomon de-interleave, then
/// decompress if the container says the payload is compressed.
///
/// `offset` is a file offset. `compressed` / `uncompressed` are the sizes
/// the file header records for this page, and `factor` is the repeat count
/// the writer used — the number of Reed-Solomon codewords follows from
/// those three, exactly as §5.3 describes ("the data repeat count is the
/// maximum RS pre-encoded size divided by the resulting (padded) data
/// length" — read backwards, the block count is
/// `ceil(factor * align8(stored) / 239)`).
pub fn load_system_page(
    bytes: &[u8],
    offset: usize,
    compressed: usize,
    uncompressed: usize,
    factor: usize,
    limits: DecompressLimits,
) -> Result<Vec<u8>> {
    let stored = compressed.min(uncompressed);
    if factor == 0 || stored == 0 {
        return Err(Error::SectionMap(
            "R2007 system page: zero repeat factor or zero stored size".into(),
        ));
    }
    let blocks = factor
        .saturating_mul(align_up(stored, CRC_BLOCK))
        .div_ceil(RS_SYSTEM_MESSAGE);
    let span = blocks.saturating_mul(RS_CODEWORD);
    let page = bytes.get(offset..offset + span).ok_or(Error::Truncated {
        offset: offset as u64,
        wanted: span,
        len: bytes.len() as u64,
    })?;
    let decoded = deinterleave(page, blocks, RS_SYSTEM_MESSAGE)?;
    let payload = decoded.get(..stored).ok_or_else(|| {
        Error::SectionMap("R2007 system page: decoded block shorter than the stored size".into())
    })?;
    if compressed < uncompressed {
        r21_lz::decompress(payload, uncompressed, limits)
    } else {
        Ok(payload[..uncompressed.min(payload.len())].to_vec())
    }
}

/// One entry of the R2007 page map (§5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageEntry {
    /// Signed page id as stored; a negative id marks a free page.
    pub id: i64,
    /// On-disk page size in bytes.
    pub size: i64,
    /// Page offset relative to [`PAGE_BASE`], accumulated in map order.
    pub offset: u64,
}

/// The R2007 page map — every page's on-disk offset, keyed by id.
#[derive(Debug, Clone, Default)]
pub struct PageMap {
    /// Entries in map order; `entries[i].id.abs()` is the lookup key.
    pub entries: Vec<PageEntry>,
}

impl PageMap {
    /// Parse the decompressed page map: a run of `(Int64 size, Int64 id)`
    /// pairs whose offsets accumulate from zero, per the loop §5.2 prints.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut entries = Vec::with_capacity(data.len() / 16);
        let mut offset: u64 = 0;
        let mut pos = 0usize;
        while pos + 16 <= data.len() {
            let size = LittleEndian::read_i64(&data[pos..pos + 8]);
            let id = LittleEndian::read_i64(&data[pos + 8..pos + 16]);
            pos += 16;
            if entries.len() >= MAX_PAGES {
                return Err(Error::SectionMap(format!(
                    "R2007 page map declares more than {MAX_PAGES} pages"
                )));
            }
            entries.push(PageEntry { id, size, offset });
            offset = offset.saturating_add(size.unsigned_abs());
        }
        Ok(Self { entries })
    }

    /// File offset of the page with the given (absolute) id.
    pub fn file_offset_of(&self, id: i64) -> Option<usize> {
        self.entries
            .iter()
            .find(|e| e.id.abs() == id.abs())
            .map(|e| PAGE_BASE + e.offset as usize)
    }

    /// Number of pages in the map.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when the map is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One page of a section, as listed in the R2007 section map (§5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionPage {
    /// Offset of this page's data within the assembled section.
    pub data_offset: u64,
    /// On-disk page size.
    pub size: u64,
    /// Page id, resolved through the [`PageMap`].
    pub id: i64,
    /// Decompressed size of this page's payload.
    pub uncompressed: u64,
    /// Stored size of this page's payload.
    pub compressed: u64,
    /// §5.4.1 32-bit data checksum.
    pub checksum: u64,
    /// 64-bit page CRC.
    pub crc: u64,
}

/// One section descriptor from the R2007 section map (§5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionDescriptor {
    /// Section name, e.g. `AcDb:AcDbObjects`.
    pub name: String,
    /// Total decompressed size of the section.
    pub data_size: u64,
    /// Maximum page size for this section.
    pub max_size: u64,
    /// Encryption flag; `0` means the payload is in the clear.
    pub encryption: u64,
    /// The §5.2 hash code that identifies the section by name.
    pub hash_code: u64,
    /// `4` means the pages are Reed-Solomon encoded and interleaved;
    /// `1` means they are stored bare.
    pub encoding: u64,
    /// Pages that make up the section, in data order.
    pub pages: Vec<SectionPage>,
}

/// The R2007 section map — every named section in the file (§5.2).
#[derive(Debug, Clone, Default)]
pub struct SectionMap {
    /// Section descriptors in map order.
    pub sections: Vec<SectionDescriptor>,
}

impl SectionMap {
    /// Parse the decompressed section map.
    ///
    /// Each descriptor is a fixed 0x40-byte header, then
    /// `SectionNameLength` bytes of UTF-16LE name (the length counts the
    /// two-byte terminator — measured on the corpus, where the first
    /// section's page list begins exactly `0x40 + SectionNameLength` bytes
    /// into the record), then `NumPages` 56-byte page entries. The final
    /// descriptor of every corpus file has an empty name and no pages; it
    /// is dropped, which is what makes `SectionsAmount` "number of sections
    /// + 1".
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut sections = Vec::new();
        let mut pos = 0usize;
        while pos + 0x40 <= data.len() {
            let q = |i: usize| LittleEndian::read_u64(&data[pos + i * 8..pos + i * 8 + 8]);
            let data_size = q(0);
            let max_size = q(1);
            let encryption = q(2);
            let hash_code = q(3);
            let name_len = q(4) as usize;
            let encoding = q(6);
            let num_pages = q(7) as usize;
            pos += 0x40;
            let mut name = String::new();
            if name_len > 0 {
                let raw = data.get(pos..pos + name_len).ok_or_else(|| {
                    Error::SectionMap(
                        "R2007 section map: section name runs past the end of the map".into(),
                    )
                })?;
                let units: Vec<u16> = raw
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                name = String::from_utf16(&units)
                    .map_err(|_| {
                        Error::SectionMap("R2007 section map: section name is not UTF-16".into())
                    })?
                    .trim_end_matches('\0')
                    .to_string();
                pos += name_len;
            }
            if num_pages > MAX_PAGES {
                return Err(Error::SectionMap(format!(
                    "R2007 section {name:?} declares {num_pages} pages, above the {MAX_PAGES} cap"
                )));
            }
            let mut pages = Vec::with_capacity(num_pages);
            for _ in 0..num_pages {
                let raw = data.get(pos..pos + 56).ok_or_else(|| {
                    Error::SectionMap(
                        "R2007 section map: page entry runs past the end of the map".into(),
                    )
                })?;
                let p = |i: usize| LittleEndian::read_u64(&raw[i * 8..i * 8 + 8]);
                pages.push(SectionPage {
                    data_offset: p(0),
                    size: p(1),
                    id: LittleEndian::read_i64(&raw[16..24]),
                    uncompressed: p(3),
                    compressed: p(4),
                    checksum: p(5),
                    crc: p(6),
                });
                pos += 56;
            }
            if name.is_empty() {
                continue;
            }
            if sections.len() >= MAX_SECTIONS {
                return Err(Error::SectionMap(format!(
                    "R2007 section map declares more than {MAX_SECTIONS} sections"
                )));
            }
            sections.push(SectionDescriptor {
                name,
                data_size,
                max_size,
                encryption,
                hash_code,
                encoding,
                pages,
            });
        }
        Ok(Self { sections })
    }

    /// Look a section up by its exact on-disk name.
    pub fn by_name(&self, name: &str) -> Option<&SectionDescriptor> {
        self.sections.iter().find(|s| s.name == name)
    }
}

/// The parsed R2007 container: file header, page map and section map.
#[derive(Debug, Clone)]
pub struct Container {
    /// The 0x110-byte file header (§5.2).
    pub header: FileHeader,
    /// Page id → file offset (§5.2).
    pub page_map: PageMap,
    /// Named sections and their page lists (§5.2).
    pub section_map: SectionMap,
}

impl Container {
    /// Walk the whole R2007 container: file header → page map → section map.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::parse_with_limits(bytes, DecompressLimits::default())
    }

    /// [`Container::parse`] with explicit decompression caps.
    pub fn parse_with_limits(bytes: &[u8], limits: DecompressLimits) -> Result<Self> {
        let header = FileHeader::parse(bytes)?;
        let page_map_offset = PAGE_BASE
            .checked_add(usize::try_from(header.pages_map_offset).map_err(|_| {
                Error::SectionMap("R2007 PagesMapOffset does not fit in a usize".into())
            })?)
            .ok_or_else(|| Error::SectionMap("R2007 page-map offset overflowed".into()))?;
        let page_bytes = load_system_page(
            bytes,
            page_map_offset,
            usize::try_from(header.pages_map_size_compressed).unwrap_or(usize::MAX),
            usize::try_from(header.pages_map_size_uncompressed).unwrap_or(usize::MAX),
            usize::try_from(header.pages_map_correction_factor).unwrap_or(0),
            limits,
        )?;
        let page_map = PageMap::parse(&page_bytes)?;
        let section_map_offset = page_map
            .file_offset_of(header.sections_map_id as i64)
            .ok_or_else(|| {
                Error::SectionMap(format!(
                    "R2007 SectionsMapId {} is not present in the page map",
                    header.sections_map_id
                ))
            })?;
        let section_bytes = load_system_page(
            bytes,
            section_map_offset,
            usize::try_from(header.sections_map_size_compressed).unwrap_or(usize::MAX),
            usize::try_from(header.sections_map_size_uncompressed).unwrap_or(usize::MAX),
            usize::try_from(header.sections_map_correction_factor).unwrap_or(0),
            limits,
        )?;
        let section_map = SectionMap::parse(&section_bytes)?;
        Ok(Self {
            header,
            page_map,
            section_map,
        })
    }

    /// Assemble a named section's decompressed bytes (§5.4).
    ///
    /// Returns [`Error::SectionMap`] when the section is absent and
    /// [`Error::Unsupported`] when its descriptor declares encryption,
    /// which this crate does not implement.
    pub fn read_section(&self, bytes: &[u8], name: &str) -> Result<Vec<u8>> {
        self.read_section_with_limits(bytes, name, DecompressLimits::default())
    }

    /// [`Container::read_section`] with explicit decompression caps.
    pub fn read_section_with_limits(
        &self,
        bytes: &[u8],
        name: &str,
        limits: DecompressLimits,
    ) -> Result<Vec<u8>> {
        let desc = self
            .section_map
            .by_name(name)
            .ok_or_else(|| Error::SectionMap(format!("section {name:?} not found")))?;
        read_section(bytes, &self.page_map, desc, limits)
    }
}

/// Assemble one section's decompressed bytes from its pages (§5.4).
///
/// Pages whose data offset is past the end of what has been assembled so
/// far are preceded by a zero run, which is how the spec says a page of
/// zeroes is represented ("A page that contains zeroes only is not written
/// to file").
pub fn read_section(
    bytes: &[u8],
    page_map: &PageMap,
    desc: &SectionDescriptor,
    limits: DecompressLimits,
) -> Result<Vec<u8>> {
    if desc.encryption != 0 && desc.encryption != 2 {
        // encryption == 2 marks "properties encrypted" on sections whose
        // *payload* is still in the clear (AcDb:FileDepList, AcDb:AppInfo).
        // Anything else means a password-protected file.
        return Err(Error::Unsupported {
            feature: format!(
                "R2007 section {:?} declares encryption {} (password-protected files \
                 are not supported)",
                desc.name, desc.encryption
            ),
        });
    }
    let total = usize::try_from(desc.data_size)
        .map_err(|_| Error::SectionMap("R2007 section data size does not fit in a usize".into()))?;
    if total > limits.max_output_bytes {
        return Err(Error::Lz77OutputLimitExceeded {
            limit: limits.max_output_bytes,
        });
    }
    let mut out: Vec<u8> = Vec::with_capacity(total);
    for page in &desc.pages {
        let want = usize::try_from(page.data_offset).unwrap_or(usize::MAX);
        if out.len() < want {
            if want > limits.max_output_bytes {
                return Err(Error::Lz77OutputLimitExceeded {
                    limit: limits.max_output_bytes,
                });
            }
            out.resize(want, 0);
        }
        let base = page_map.file_offset_of(page.id).ok_or_else(|| {
            Error::SectionMap(format!(
                "R2007 section {:?} references page id {} which the page map does not list",
                desc.name, page.id
            ))
        })?;
        let compressed = usize::try_from(page.compressed).unwrap_or(usize::MAX);
        let uncompressed = usize::try_from(page.uncompressed).unwrap_or(usize::MAX);
        let stored: Vec<u8> = if desc.encoding == 4 {
            // §5.13: data pages use (255, 251), interleaved.
            let blocks = compressed.div_ceil(RS_DATA_MESSAGE).max(1);
            let span = blocks.saturating_mul(RS_CODEWORD);
            let raw = bytes.get(base..base + span).ok_or(Error::Truncated {
                offset: base as u64,
                wanted: span,
                len: bytes.len() as u64,
            })?;
            let decoded = deinterleave(raw, blocks, RS_DATA_MESSAGE)?;
            decoded
                .get(..compressed)
                .ok_or_else(|| {
                    Error::SectionMap(format!(
                        "R2007 section {:?}: page {} shorter than its compressed size",
                        desc.name, page.id
                    ))
                })?
                .to_vec()
        } else {
            bytes
                .get(base..base + compressed)
                .ok_or(Error::Truncated {
                    offset: base as u64,
                    wanted: compressed,
                    len: bytes.len() as u64,
                })?
                .to_vec()
        };
        let plain = if compressed < uncompressed {
            r21_lz::decompress(&stored, uncompressed, limits)?
        } else {
            stored
        };
        let take = uncompressed.min(plain.len());
        out.extend_from_slice(&plain[..take]);
    }
    out.truncate(total);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_page_size_matches_the_spec_pseudo_code() {
        // §5.3.1's floor.
        assert_eq!(system_page_size(0), 0x400);
        assert_eq!(system_page_size(304), 0x400);
        // The section map of every corpus R2007 file: 2132 decompressed
        // bytes, which the writer put in a 0x1200-byte page.
        assert_eq!(system_page_size(2132), 0x1200);
    }

    #[test]
    fn deinterleave_gathers_each_codewords_message_bytes() {
        // Two interleaved codewords, message size 3: codeword 0 owns the
        // even positions, codeword 1 the odd ones.
        let mut page = vec![0u8; 2 * RS_CODEWORD];
        for i in 0..RS_CODEWORD {
            page[i * 2] = i as u8;
            page[i * 2 + 1] = 0x80u8.wrapping_add(i as u8);
        }
        let out = deinterleave(&page, 2, 3).unwrap();
        assert_eq!(out, vec![0, 1, 2, 0x80, 0x81, 0x82]);
    }

    #[test]
    fn deinterleave_refuses_a_short_page() {
        let err = deinterleave(&[0u8; 10], 1, RS_SYSTEM_MESSAGE).unwrap_err();
        assert!(matches!(err, Error::Truncated { .. }));
    }

    #[test]
    fn page_map_accumulates_offsets_in_map_order() {
        // Three pages of 0x400 / 0x200 / 0x100 bytes with ids 22, 23, 3 —
        // the shape every corpus file starts with.
        let mut data = Vec::new();
        for (size, id) in [(0x400i64, 22i64), (0x200, 23), (0x100, 3)] {
            data.extend_from_slice(&size.to_le_bytes());
            data.extend_from_slice(&id.to_le_bytes());
        }
        let map = PageMap::parse(&data).unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map.file_offset_of(22), Some(PAGE_BASE));
        assert_eq!(map.file_offset_of(23), Some(PAGE_BASE + 0x400));
        assert_eq!(map.file_offset_of(3), Some(PAGE_BASE + 0x600));
        assert_eq!(map.file_offset_of(99), None);
    }

    #[test]
    fn page_map_keys_on_the_absolute_id_so_free_pages_still_resolve() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x400i64.to_le_bytes());
        data.extend_from_slice(&(-7i64).to_le_bytes());
        let map = PageMap::parse(&data).unwrap();
        assert_eq!(map.file_offset_of(7), Some(PAGE_BASE));
    }

    /// Build one section-map record by hand and read it back. The name
    /// length counts the two-byte terminator, which is the detail that
    /// decides where the page list starts.
    #[test]
    fn section_map_name_length_counts_the_terminator() {
        let name = "AcDb:Handles";
        let mut data = Vec::new();
        let mut header = [0u64; 8];
        header[0] = 591; // data size
        header[1] = 63488; // max size
        header[2] = 0; // encryption
        header[3] = 0x3F6E_0450; // hash code
        header[4] = (name.len() as u64 + 1) * 2; // name length, with terminator
        header[6] = 4; // encoding
        header[7] = 1; // one page
        for v in header {
            data.extend_from_slice(&v.to_le_bytes());
        }
        for unit in name.encode_utf16() {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        data.extend_from_slice(&0u16.to_le_bytes());
        for v in [0u64, 768, 14, 591, 566, 0x0A92_5B96, 0] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let map = SectionMap::parse(&data).unwrap();
        assert_eq!(map.sections.len(), 1);
        let s = &map.sections[0];
        assert_eq!(s.name, "AcDb:Handles");
        assert_eq!(s.hash_code, 0x3F6E_0450);
        assert_eq!(s.encoding, 4);
        assert_eq!(s.pages.len(), 1);
        assert_eq!(s.pages[0].id, 14);
        assert_eq!(s.pages[0].compressed, 566);
        assert_eq!(s.pages[0].uncompressed, 591);
    }

    #[test]
    fn section_map_drops_the_trailing_unnamed_record() {
        let mut data = vec![0u8; 0x40];
        // A zero header: no name, no pages. SectionsAmount counts it, the
        // parsed list does not.
        LittleEndian::write_u64(&mut data[0..8], 0);
        let map = SectionMap::parse(&data).unwrap();
        assert!(map.sections.is_empty());
    }

    #[test]
    fn file_header_parse_refuses_a_short_buffer() {
        let err = FileHeader::parse(&[0u8; 0x100]).unwrap_err();
        assert!(matches!(err, Error::Truncated { .. }));
    }

    #[test]
    fn encrypted_section_is_refused_rather_than_mis_decoded() {
        let desc = SectionDescriptor {
            name: "AcDb:AcDbObjects".into(),
            data_size: 16,
            max_size: 0xF800,
            encryption: 1,
            hash_code: 0x674C_05A9,
            encoding: 4,
            pages: Vec::new(),
        };
        let err =
            read_section(&[], &PageMap::default(), &desc, DecompressLimits::default()).unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }));
    }
}
