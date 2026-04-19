//! Structured parsers for the well-known named sections whose format is
//! **byte-oriented** (not bit-packed): SummaryInfo, AppInfo,
//! AppInfoHistory, FileDepList, and Preview.
//!
//! Each of these lives as a named section in the R2004-family section
//! info table; callers extract the decompressed bytes via
//! `DwgFile::read_section(name)` and then hand them to the parser here.
//!
//! For the bit-packed sections (Header, Classes, Handles, AcDbObjects)
//! see `crate::header_vars`, `crate::classes`, `crate::handle_map`, and
//! `crate::object` respectively.

use crate::error::{Error, Result};
use byteorder::{ByteOrder, LittleEndian};

// ================================================================
// SummaryInfo (spec §13)
// ================================================================

/// Drawing-level metadata: title, author, keywords, and custom property
/// key/value pairs (set via `SUMMARYINFO` in AutoCAD).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SummaryInfo {
    pub title: String,
    pub subject: String,
    pub author: String,
    pub keywords: String,
    pub comments: String,
    pub last_saved_by: String,
    pub revision_number: String,
    pub hyperlink_base: String,
    /// Total editing time — written as two zero i32s by ODA; we preserve
    /// the raw bytes in case AutoCAD-produced files carry real data.
    pub total_edit_time_raw: [u8; 8],
    /// Create date-time (Julian date, 8 bytes — 4-byte day + 4-byte ms).
    pub create_date_raw: [u8; 8],
    /// Modified date-time (Julian date, 8 bytes).
    pub modified_date_raw: [u8; 8],
    /// Custom property key/value pairs from the Drawing Properties dialog.
    pub properties: Vec<(String, String)>,
}

impl SummaryInfo {
    /// Parse a decompressed `AcDb:SummaryInfo` payload.
    ///
    /// Auto-detects the string encoding: R18 files use 8-bit strings (u16
    /// length + bytes + NUL), R21+ files use UTF-16LE (u16 char count +
    /// count × 2 bytes where the last code unit is NUL). Heuristic: if a
    /// full Unicode parse consumes the whole buffer without error, accept
    /// it; otherwise fall back to ANSI.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if let Ok(si) = Self::parse_with(bytes, StringEncoding::Unicode) {
            return Ok(si);
        }
        Self::parse_with(bytes, StringEncoding::Ansi)
    }

    fn parse_with(bytes: &[u8], enc: StringEncoding) -> Result<Self> {
        let mut c = ByteCursor::new(bytes);
        let read = |c: &mut ByteCursor<'_>| match enc {
            StringEncoding::Ansi => c.read_lenstr(),
            StringEncoding::Unicode => c.read_unicode_lenstr_inclusive_nul(),
        };
        let title = read(&mut c)?;
        let subject = read(&mut c)?;
        let author = read(&mut c)?;
        let keywords = read(&mut c)?;
        let comments = read(&mut c)?;
        let last_saved_by = read(&mut c)?;
        let revision_number = read(&mut c)?;
        let hyperlink_base = read(&mut c)?;
        let total_edit_time_raw = c.read_array::<8>()?;
        let create_date_raw = c.read_array::<8>()?;
        let modified_date_raw = c.read_array::<8>()?;
        let prop_count = c.read_u16()? as usize;
        let mut properties = Vec::with_capacity(prop_count);
        for _ in 0..prop_count {
            let key = read(&mut c)?;
            let value = read(&mut c)?;
            properties.push((key, value));
        }
        Ok(Self {
            title,
            subject,
            author,
            keywords,
            comments,
            last_saved_by,
            revision_number,
            hyperlink_base,
            total_edit_time_raw,
            create_date_raw,
            modified_date_raw,
            properties,
        })
    }
}

#[derive(Copy, Clone)]
enum StringEncoding {
    Ansi,
    Unicode,
}

// ================================================================
// AppInfo (spec §16)
// ================================================================

/// Identifies the application that wrote the .dwg file.
///
/// The on-disk layout differs by version. R18 uses 8-bit strings; R21
/// and later use UTF-16LE. The parser auto-detects by looking for the
/// version marker.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppInfo {
    /// Always "AppInfoDataList" when ODA-written.
    pub name: String,
    /// XML blob: `<ProductInformation name="Teigha" build_version="..."/>`.
    pub product_xml: String,
    /// Application version, e.g. "2.7.2.0" or AutoCAD's "Z.75.0.0".
    pub version: String,
    /// Free-form comment (R21+ only).
    pub comment: String,
    /// Free-form product string (R21+ only).
    pub product: String,
}

impl AppInfo {
    /// Attempt both R18 (ANSI) and R21+ (Unicode) layouts; accept
    /// whichever yields a plausible structure.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        // Heuristic: R21+ begins with a 4-byte "unknown" (ODA writes 2)
        // followed by a UTF-16LE string. R18 begins directly with the
        // 16-bit length of an ANSI string.
        if bytes.len() >= 4 {
            let maybe_unknown = LittleEndian::read_u32(&bytes[0..4]);
            if maybe_unknown == 2 || maybe_unknown == 3 {
                if let Ok(ai) = Self::parse_r21(&bytes[4..]) {
                    return Ok(ai);
                }
            }
        }
        Self::parse_r18(bytes)
    }

    fn parse_r18(bytes: &[u8]) -> Result<Self> {
        let mut c = ByteCursor::new(bytes);
        let name = c.read_lenstr()?;
        let _unknown_u32 = c.read_u32()?;
        let _unknown_str = c.read_lenstr()?;
        let product_xml = c.read_lenstr()?;
        let version = c.read_lenstr().unwrap_or_default();
        Ok(AppInfo {
            name,
            product_xml,
            version,
            ..Default::default()
        })
    }

    fn parse_r21(bytes: &[u8]) -> Result<Self> {
        let mut c = ByteCursor::new(bytes);
        let name = c.read_unicode_lenstr()?;
        let _unknown_u32 = c.read_u32()?;
        // 16 bytes of version data (checksum, ODA writes zeros)
        c.skip(16)?;
        let version = c.read_unicode_lenstr()?;
        c.skip(16)?; // comment data checksum
        let comment = c.read_unicode_lenstr().unwrap_or_default();
        c.skip(16)?; // product data checksum
        let product = c.read_unicode_lenstr().unwrap_or_default();
        // Final ANSI version string.
        let app_version = c.read_lenstr().unwrap_or_default();
        Ok(AppInfo {
            name,
            product_xml: String::new(),
            version: if app_version.is_empty() {
                version
            } else {
                app_version
            },
            comment,
            product,
        })
    }
}

// ================================================================
// Preview (spec §14)
// ================================================================

/// Thumbnail image embedded in the .dwg (BMP or WMF).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// Overall size reported by the preview header (bytes).
    pub overall_size: u32,
    /// Extracted bitmap bytes (always BMP if present).
    pub bmp: Option<Vec<u8>>,
    /// Extracted Windows Metafile bytes (if present; rare).
    pub wmf: Option<Vec<u8>>,
    /// The "header data" blob (per-image preview header, opaque).
    pub header_data: Option<Vec<u8>>,
}

impl Preview {
    /// Start sentinel before the preview body (spec §14.2).
    pub const START_SENTINEL: [u8; 16] = [
        0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D, 0x44, 0x10,
        0x2B,
    ];

    /// End sentinel after the preview body.
    pub const END_SENTINEL: [u8; 16] = [
        0xE0, 0xDA, 0x92, 0xF8, 0x2B, 0xC9, 0xD7, 0xD7, 0x62, 0xA8, 0x35, 0xC0, 0x62, 0xBB, 0xEF,
        0xD4,
    ];

    /// Parse a decompressed `AcDb:Preview` payload.
    ///
    /// AutoCAD versions starting with 2013 emit a PNG thumbnail (code 6)
    /// not documented in the original ODA spec. If the structured parse
    /// fails or produces no image, we scan the payload for BMP (`BM`)
    /// and PNG (`\x89PNG\r\n\x1a\n`) magic bytes and carve the first
    /// match to the end of the sentinel-bounded region.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 20 || bytes[..16] != Self::START_SENTINEL {
            return Err(Error::SectionMap(
                "preview: start sentinel missing".to_string(),
            ));
        }
        let mut c = ByteCursor::new(&bytes[16..]);
        let overall_size = c.read_u32().unwrap_or(0);
        let images_present = c.read_u8().unwrap_or(0);
        let mut header_data: Option<(usize, usize)> = None;
        let mut bmp_range: Option<(usize, usize)> = None;
        let mut wmf_range: Option<(usize, usize)> = None;
        let mut png_range: Option<(usize, usize)> = None;
        for _ in 0..images_present {
            let Ok(code) = c.read_u8() else { break };
            let start = c.read_u32().ok().map(|v| v as usize);
            let size = c.read_u32().ok().map(|v| v as usize);
            let (Some(start), Some(size)) = (start, size) else {
                break;
            };
            match code {
                1 => header_data = Some((start, size)),
                2 => bmp_range = Some((start, size)),
                3 => wmf_range = Some((start, size)),
                6 => png_range = Some((start, size)),
                _ => { /* unknown code — stored but not exposed */ }
            }
        }
        let extract = |range: Option<(usize, usize)>| -> Option<Vec<u8>> {
            let (start, size) = range?;
            if start == 0 || size == 0 {
                return None;
            }
            let end = start.checked_add(size)?;
            if end > bytes.len() {
                return None;
            }
            Some(bytes[start..end].to_vec())
        };
        let mut bmp = extract(bmp_range);
        let wmf = extract(wmf_range);
        let header_data = extract(header_data);
        // PNG masquerades as a bitmap for modern files.
        if bmp.is_none() {
            bmp = extract(png_range);
        }
        // Fallback: scan for image magic if no structured extract worked.
        if bmp.is_none() && wmf.is_none() {
            if let Some(bytes) = carve_image_magic(bytes) {
                bmp = Some(bytes);
            }
        }
        Ok(Self {
            overall_size,
            bmp,
            wmf,
            header_data,
        })
    }
}

/// Scan a preview payload (including sentinels) for PNG or BMP magic
/// bytes and return everything from the magic to the start of the end
/// sentinel (or end of buffer if no end sentinel is found).
fn carve_image_magic(bytes: &[u8]) -> Option<Vec<u8>> {
    const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    const BMP_MAGIC: &[u8] = b"BM";
    let mut start = None;
    for i in 16..bytes.len().saturating_sub(PNG_MAGIC.len()) {
        if &bytes[i..i + PNG_MAGIC.len()] == PNG_MAGIC {
            start = Some(i);
            break;
        }
        if &bytes[i..i + 2] == BMP_MAGIC && bytes.len() - i > 54 {
            start = Some(i);
            break;
        }
    }
    let start = start?;
    // Truncate before the end sentinel if present.
    let end_sentinel_start = bytes
        .windows(16)
        .rposition(|w| w == Preview::END_SENTINEL)
        .unwrap_or(bytes.len());
    let end = end_sentinel_start.max(start);
    Some(bytes[start..end].to_vec())
}

// ================================================================
// FileDepList (spec §17)
// ================================================================

/// Single entry in the file-dependency list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDependency {
    pub full_filename: String,
    pub found_path: String,
    pub fingerprint_guid: String,
    pub version_guid: String,
    pub feature_index: i32,
    pub timestamp: i32,
    pub filesize: i32,
    pub affects_graphics: bool,
    pub reference_count: i32,
}

/// Aggregated file dependency list (external fonts, images, XREFs, ...).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDepList {
    /// Named features ("Acad:XRef", "Acad:Image", "Acad:Text", ...).
    pub features: Vec<String>,
    /// Per-file dependencies cross-referencing a feature by index.
    pub files: Vec<FileDependency>,
}

impl FileDepList {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut c = ByteCursor::new(bytes);
        let feature_count = c.read_u32()? as usize;
        let mut features = Vec::with_capacity(feature_count);
        for _ in 0..feature_count {
            features.push(c.read_lenstr32()?);
        }
        let file_count = c.read_u32()? as usize;
        let mut files = Vec::with_capacity(file_count);
        for _ in 0..file_count {
            let full_filename = c.read_lenstr32()?;
            let found_path = c.read_lenstr32()?;
            let fingerprint_guid = c.read_lenstr32()?;
            let version_guid = c.read_lenstr32()?;
            let feature_index = c.read_u32()? as i32;
            let timestamp = c.read_u32()? as i32;
            let filesize = c.read_u32()? as i32;
            let affects_graphics = c.read_u16()? != 0;
            let reference_count = c.read_u32()? as i32;
            files.push(FileDependency {
                full_filename,
                found_path,
                fingerprint_guid,
                version_guid,
                feature_index,
                timestamp,
                filesize,
                affects_graphics,
                reference_count,
            });
        }
        Ok(Self { features, files })
    }
}

// ================================================================
// Byte cursor helper (byte-aligned reads; mirrors BitCursor but for
// the fixed-format metadata sections)
// ================================================================

struct ByteCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> ByteCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn need(&self, n: usize) -> Result<()> {
        if self.pos + n > self.bytes.len() {
            Err(Error::Truncated {
                offset: self.pos as u64,
                wanted: n,
                len: self.bytes.len() as u64,
            })
        } else {
            Ok(())
        }
    }

    fn skip(&mut self, n: usize) -> Result<()> {
        self.need(n)?;
        self.pos += n;
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8> {
        self.need(1)?;
        let b = self.bytes[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn read_u16(&mut self) -> Result<u16> {
        self.need(2)?;
        let v = LittleEndian::read_u16(&self.bytes[self.pos..self.pos + 2]);
        self.pos += 2;
        Ok(v)
    }

    fn read_u32(&mut self) -> Result<u32> {
        self.need(4)?;
        let v = LittleEndian::read_u32(&self.bytes[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(v)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.need(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(&self.bytes[self.pos..self.pos + N]);
        self.pos += N;
        Ok(out)
    }

    /// Length-prefixed 8-bit string: u16 length + bytes + NUL terminator.
    /// `length` includes the NUL.
    fn read_lenstr(&mut self) -> Result<String> {
        let len = self.read_u16()? as usize;
        if len == 0 {
            return Ok(String::new());
        }
        self.need(len)?;
        // `len` bytes include the trailing NUL; drop it if present.
        let end = self.pos + len;
        let text_end = if self.bytes[end - 1] == 0 {
            end - 1
        } else {
            end
        };
        let s = String::from_utf8_lossy(&self.bytes[self.pos..text_end]).into_owned();
        self.pos = end;
        Ok(s)
    }

    /// R21+ AppInfo variant: u16 char_count + 2 × char_count bytes + 2-byte NUL
    /// appended AFTER the counted content.
    fn read_unicode_lenstr(&mut self) -> Result<String> {
        let char_count = self.read_u16()? as usize;
        if char_count == 0 {
            return Ok(String::new());
        }
        let byte_count = char_count * 2;
        self.need(byte_count + 2)?;
        let mut u16s = Vec::with_capacity(char_count);
        for i in 0..char_count {
            u16s.push(LittleEndian::read_u16(
                &self.bytes[self.pos + i * 2..self.pos + i * 2 + 2],
            ));
        }
        self.pos += byte_count;
        // Skip the NUL terminator (2 bytes).
        self.pos += 2;
        while let Some(&0) = u16s.last() {
            u16s.pop();
        }
        Ok(String::from_utf16_lossy(&u16s))
    }

    /// R2018 SummaryInfo variant: u16 char_count + 2 × char_count bytes where
    /// the counted content INCLUDES the trailing NUL (no extra NUL follows).
    fn read_unicode_lenstr_inclusive_nul(&mut self) -> Result<String> {
        let char_count = self.read_u16()? as usize;
        if char_count == 0 {
            return Ok(String::new());
        }
        let byte_count = char_count * 2;
        self.need(byte_count)?;
        let mut u16s = Vec::with_capacity(char_count);
        for i in 0..char_count {
            u16s.push(LittleEndian::read_u16(
                &self.bytes[self.pos + i * 2..self.pos + i * 2 + 2],
            ));
        }
        self.pos += byte_count;
        while let Some(&0) = u16s.last() {
            u16s.pop();
        }
        Ok(String::from_utf16_lossy(&u16s))
    }

    /// Length-prefixed 8-bit string with a 32-bit length, NO NUL terminator.
    /// Used by FileDepList (spec §17).
    fn read_lenstr32(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        if len == 0 || len == u32::MAX as usize {
            return Ok(String::new());
        }
        self.need(len)?;
        let s = String::from_utf8_lossy(&self.bytes[self.pos..self.pos + len]).into_owned();
        self.pos += len;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_info_empty_strings() {
        // 8 strings × 2 bytes (length=0) + 8+8+8 date bytes + 2 byte prop count
        let mut data = vec![0u8; 2 * 8 + 24 + 2];
        // all zeros means empty strings and zero properties
        let si = SummaryInfo::parse(&data).unwrap();
        assert_eq!(si.title, "");
        assert_eq!(si.properties.len(), 0);
        // silence unused mut warning
        data[0] = 0;
    }

    #[test]
    fn summary_info_with_title() {
        // title = "X" (1 byte + NUL = 2 bytes with len=2)
        let mut data = Vec::new();
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(b"X\0");
        for _ in 0..7 {
            data.extend_from_slice(&0u16.to_le_bytes());
        }
        data.extend_from_slice(&[0u8; 24]);
        data.extend_from_slice(&0u16.to_le_bytes());
        let si = SummaryInfo::parse(&data).unwrap();
        assert_eq!(si.title, "X");
    }

    #[test]
    fn preview_rejects_bad_sentinel() {
        let bad = [0u8; 32];
        assert!(Preview::parse(&bad).is_err());
    }

    #[test]
    fn preview_empty_after_sentinel_errors_cleanly() {
        let mut data = Vec::new();
        data.extend_from_slice(&Preview::START_SENTINEL);
        // too short, overall_size + image count require 5 more bytes
        let out = Preview::parse(&data);
        assert!(out.is_err());
    }
}
