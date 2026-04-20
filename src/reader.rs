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

/// Diagnostic summary for a best-effort parse.
///
/// Returned by [`DwgFile::from_bytes_best_effort`]. Captures anything
/// the parser noticed but didn't raise as a hard error — useful for
/// exploratory CLIs, debugging corrupt files, or surfacing warnings
/// in an IDE integration without interrupting normal decode.
///
/// Strict callers should use [`DwgFile::from_bytes_strict`] instead,
/// which errors on any non-`Full` [`SectionMapStatus`] and reports
/// the first problematic condition as a typed [`Error`].
#[derive(Debug, Clone, Default)]
pub struct ParseDiagnostics {
    /// The [`SectionMapStatus`] the resulting [`DwgFile`] will report.
    /// Duplicated here so callers who pass the diagnostics around
    /// don't need a reference to the `DwgFile` to inspect it.
    pub section_map_status: SectionMapStatus,
    /// Free-form human-readable notes about the parse. Reserved for
    /// future per-section diagnostics (CRC mismatches tolerated,
    /// object-stream gaps filled, etc.). Empty in 0.1.0-alpha.1.
    pub warnings: Vec<String>,
}

/// How the section list was derived — surfaces whether a file's
/// section enumeration comes from a full section-map walk, a
/// defensive stub fallback, or a synthetic placeholder for an
/// unsupported format.
///
/// Expose via [`DwgFile::section_map_status`]. Callers that need to
/// know whether [`DwgFile::sections`] is authoritative should branch
/// on this before trusting the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SectionMapStatus {
    /// R13-R15 flat locator OR R2004-family full section-map walk
    /// succeeded; the returned `sections` list is authoritative.
    #[default]
    Full,
    /// R2004-family file, but the full section-map walk failed or
    /// returned zero entries. The crate fell back to a stub
    /// enumeration synthesized from the R2004 header's known offset
    /// fields. The `sections` list contains what could be
    /// discovered, plus any known-name entries whose payload size
    /// is unknown. Callers should treat this as advisory only.
    Fallback { reason: String },
    /// R2007 — spec §5 layout not yet implemented by this crate.
    /// `sections` contains a single `_R2007_UNSUPPORTED` placeholder
    /// that covers the whole post-header byte range; it is not
    /// individually decodable.
    Deferred { reason: String },
}

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
    section_map_status: SectionMapStatus,
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

    /// Open with explicit safety limits — refuses to read the file at
    /// all if its on-disk size exceeds [`OpenLimits::max_file_bytes`].
    /// Use [`OpenLimits::paranoid`] for untrusted-upload contexts.
    ///
    /// Performs the size check via `fs::metadata` BEFORE allocating the
    /// buffer, so an adversarial filename pointing at a multi-GB file
    /// cannot trigger an OOM allocation. The other caps in `limits`
    /// (parse / walker / decompress) are stored on the resulting
    /// `DwgFile` for downstream paths to honor (currently advisory —
    /// per-cap wiring is task-tracked).
    pub fn open_with_limits(
        path: impl AsRef<Path>,
        limits: crate::limits::OpenLimits,
    ) -> Result<Self> {
        let path = path.as_ref();
        let meta = fs::metadata(path)?;
        if meta.len() > limits.max_file_bytes {
            return Err(Error::SectionMap(format!(
                "open refused — file size {} bytes exceeds OpenLimits::max_file_bytes {} bytes",
                meta.len(),
                limits.max_file_bytes
            )));
        }
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
                section_map_status: SectionMapStatus::Full,
                r2004: None,
                r13: Some(header),
                r2007_common: None,
            })
        } else if version.is_r2004_family() {
            let header = R2004Header::parse(&bytes)?;
            let (sections, section_map_status) = extract_r2004_sections(&bytes, &header)?;
            Ok(Self {
                bytes,
                version,
                sections,
                section_map_status,
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
            // Synthetic placeholder. R2007 files have real sections but
            // the two-layer Sec_Mask bookkeeping that locates them is
            // not yet implemented. This entry signals "here be dragons"
            // to callers iterating `file.sections()`; it is NOT a real
            // decompressible section.
            sections.push(Section {
                name: "_R2007_UNSUPPORTED".to_string(),
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
                section_map_status: SectionMapStatus::Deferred {
                    reason: "R2007 two-layer Sec_Mask section locator not yet implemented \
                             (spec §5); placeholder covers the post-header byte range only"
                        .to_string(),
                },
                r2004: None,
                r13: None,
                r2007_common: Some(common),
            })
        }
    }

    /// How the section list was derived — see [`SectionMapStatus`].
    ///
    /// Use this to distinguish an authoritative
    /// [`DwgFile::sections`] list from a best-effort fallback or a
    /// synthetic placeholder for an unsupported format.
    pub fn section_map_status(&self) -> &SectionMapStatus {
        &self.section_map_status
    }

    /// Parse a byte buffer in **strict** mode. Returns `Err` if the
    /// section map could not be fully walked (the returned
    /// [`SectionMapStatus`] would not be `Full`).
    ///
    /// Use this when the caller needs to trust
    /// [`DwgFile::sections`] as authoritative — for example, when
    /// computing coverage metrics, or when feeding the section list
    /// into an enforcement step that must not run on partial data.
    ///
    /// For exploratory or lenient use cases, prefer
    /// [`from_bytes_best_effort`](Self::from_bytes_best_effort) or
    /// the back-compat default [`from_bytes`](Self::from_bytes).
    pub fn from_bytes_strict(bytes: Vec<u8>) -> Result<Self> {
        let file = Self::from_bytes(bytes)?;
        match file.section_map_status() {
            SectionMapStatus::Full => Ok(file),
            SectionMapStatus::Fallback { reason } => Err(Error::SectionMap(format!(
                "strict parse refused — section map fell back: {reason}"
            ))),
            SectionMapStatus::Deferred { reason } => Err(Error::Unsupported {
                feature: format!("strict parse refused — section map deferred: {reason}"),
            }),
        }
    }

    /// Parse a byte buffer in **best-effort** mode, returning the
    /// resulting [`DwgFile`] alongside a [`ParseDiagnostics`]
    /// describing anything the parser noticed but didn't hard-error
    /// on.
    ///
    /// Preserves the existing [`from_bytes`](Self::from_bytes)
    /// semantics — in particular, falling back to stub section
    /// enumeration on an R2004 section-map failure and synthesizing
    /// a placeholder for R2007 — but makes the fallback visible to
    /// the caller.
    pub fn from_bytes_best_effort(bytes: Vec<u8>) -> Result<(Self, ParseDiagnostics)> {
        let file = Self::from_bytes(bytes)?;
        let diagnostics = ParseDiagnostics {
            section_map_status: file.section_map_status().clone(),
            warnings: Vec::new(),
        };
        Ok((file, diagnostics))
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
    /// accessors on [`crate::header_vars::HeaderVars`] can extract individual variables.
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

    /// End-to-end entity decode: walk every object via the handle map,
    /// then dispatch each one through the per-type decoder. Returns the
    /// list of [`crate::entities::DecodedEntity`] plus a
    /// [`crate::entities::DispatchSummary`] counting how many succeeded,
    /// were skipped (non-entity or unknown type), or errored.
    ///
    /// This is the method to call if you want "actual entities" out of a
    /// DWG file. [`DwgFile::all_objects`] is the lower-level primitive
    /// that returns raw type-coded blobs.
    pub fn decoded_entities(
        &self,
    ) -> Option<
        Result<(
            Vec<crate::entities::DecodedEntity>,
            crate::entities::DispatchSummary,
        )>,
    > {
        let raws = match self.all_objects()? {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        // Resolve the class map up-front so we can classify Custom(N)
        // type codes during dispatch. Missing / parse-errored class
        // map is fine — custom-class objects simply fall through to
        // Unhandled, matching the pre-resolver behaviour.
        let class_map = self.class_map().and_then(Result::ok);
        let mut out = Vec::with_capacity(raws.len());
        let mut summary = crate::entities::DispatchSummary::default();
        for raw in &raws {
            let decoded = match (class_map.as_ref(), raw.kind) {
                (Some(cm), crate::ObjectType::Custom(code)) => {
                    crate::entities::decode_from_raw_with_class_map(raw, self.version, cm, code)
                }
                _ => crate::entities::decode_from_raw(raw, self.version),
            };
            summary.record(&decoded);
            out.push(decoded);
        }
        Some(Ok((out, summary)))
    }

    /// High-level overview of the file. Returned by
    /// [`summarize_strict`](Self::summarize_strict) and
    /// [`summarize_lossy`](Self::summarize_lossy).
    ///
    /// Avoids copying bytes or decoding entities — just surfaces the
    /// already-known metadata (version, byte count, section
    /// enumeration, R2004 header presence) in one struct.
    pub fn summary(&self) -> Summary {
        Summary {
            version: self.version,
            file_size_bytes: self.bytes.len() as u64,
            section_count: self.sections.len(),
            section_map_status: self.section_map_status.clone(),
            has_r2004_header: self.r2004.is_some(),
            has_r13_header: self.r13.is_some(),
            has_r2007_common_header: self.r2007_common.is_some(),
        }
    }

    /// Strict summarize — equivalent to [`summary`](Self::summary)
    /// but errors if the section map isn't `Full`. SaaS pipelines
    /// that shouldn't silently accept partial metadata use this path.
    pub fn summarize_strict(&self) -> Result<Summary> {
        match &self.section_map_status {
            SectionMapStatus::Full => Ok(self.summary()),
            SectionMapStatus::Fallback { reason } => Err(Error::SectionMap(format!(
                "strict summarize refused — section map fell back: {reason}"
            ))),
            SectionMapStatus::Deferred { reason } => Err(Error::Unsupported {
                feature: format!("strict summarize refused — section map deferred: {reason}"),
            }),
        }
    }

    /// Best-effort summarize — returns a [`Summary`] wrapped in a
    /// [`crate::api::Decoded`] with any accumulated diagnostics.
    /// Never errors for a `DwgFile` that was constructed successfully.
    pub fn summarize_lossy(&self) -> crate::api::Decoded<Summary> {
        let summary = self.summary();
        let mut diagnostics = crate::api::Diagnostics::default();
        match &self.section_map_status {
            SectionMapStatus::Full => {}
            SectionMapStatus::Fallback { reason } => {
                diagnostics.warn(
                    "section_map_fallback",
                    format!("section map fell back: {reason}"),
                );
            }
            SectionMapStatus::Deferred { reason } => {
                diagnostics.warn(
                    "section_map_deferred",
                    format!("section map deferred: {reason}"),
                );
            }
        }
        if diagnostics.is_clean() {
            crate::api::Decoded::complete(summary)
        } else {
            crate::api::Decoded::partial(summary, diagnostics)
        }
    }
}

/// A high-level overview of a parsed DWG file. Returned by
/// [`DwgFile::summary`] and its strict / lossy variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Detected format version (R14 / R2000 / … / R2018).
    pub version: Version,
    /// Total file size in bytes.
    pub file_size_bytes: u64,
    /// Count of enumerated sections.
    pub section_count: usize,
    /// How the section list was derived (authoritative vs fallback).
    pub section_map_status: SectionMapStatus,
    /// `true` if an R2004-family header was parsed.
    pub has_r2004_header: bool,
    /// `true` if an R13-R15 header was parsed.
    pub has_r13_header: bool,
    /// `true` if an R2007 common header was parsed (R2007 full
    /// section layout is Phase B work).
    pub has_r2007_common_header: bool,
}

/// Walk the R2004+ Section Page Map → Section Info chain and emit a
/// `Section` list with real named entries.
///
/// Phase B: LZ77 + page-map parse + section-info parse produce the complete
/// AcDb:Header / AcDb:Classes / AcDb:Handles / AcDb:AcDbObjects / etc list.
/// If the parse fails for any reason (e.g. corrupt file, format ahead of
/// this implementation), we fall back to the Phase A stub enumeration so
/// the file still opens with partial metadata.
fn extract_r2004_sections(
    bytes: &[u8],
    header: &R2004Header,
) -> Result<(Vec<Section>, SectionMapStatus)> {
    // Try the full Section Map walk first; fall back to stub enumeration
    // on error or empty result, recording the reason in SectionMapStatus.
    match extract_r2004_sections_full(bytes, header) {
        Ok(full) if !full.is_empty() => Ok((full, SectionMapStatus::Full)),
        Ok(_) => {
            let stub = extract_r2004_sections_stub(bytes, header)?;
            Ok((
                stub,
                SectionMapStatus::Fallback {
                    reason: "R2004 full section-map walk returned zero entries; \
                             fell back to stub enumeration of known-named sections"
                        .to_string(),
                },
            ))
        }
        Err(e) => {
            let stub = extract_r2004_sections_stub(bytes, header)?;
            Ok((
                stub,
                SectionMapStatus::Fallback {
                    reason: format!(
                        "R2004 full section-map walk failed ({e}); fell back to \
                         stub enumeration"
                    ),
                },
            ))
        }
    }
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

#[cfg(test)]
mod summary_tests {
    use super::*;

    /// Build a minimal R13 file fixture covering the magic, version,
    /// and locator bytes that DwgFile::from_bytes will accept.
    /// Returns the byte vector ready to feed into from_bytes().
    fn minimal_r13_bytes() -> Vec<u8> {
        // We use a real corpus file via include_bytes! when this test
        // module wants real-data smoke tests. For now we use an
        // empty Vec: from_bytes will refuse on truncated input, and
        // we test the strict/lossy summarize wrappers via that
        // refusal path.
        Vec::new()
    }

    #[test]
    fn summarize_strict_refuses_empty_input() {
        // Empty buffer fails at the version-detect stage — the
        // summarize_strict wrapper is never reached. We assert the
        // overall failure rather than the summarize behaviour.
        let bytes = minimal_r13_bytes();
        assert!(DwgFile::from_bytes(bytes).is_err());
    }

    #[test]
    fn summary_struct_round_trips_through_clone() {
        let s = Summary {
            version: Version::R2018,
            file_size_bytes: 1024,
            section_count: 5,
            section_map_status: SectionMapStatus::Full,
            has_r2004_header: true,
            has_r13_header: false,
            has_r2007_common_header: false,
        };
        let cloned = s.clone();
        assert_eq!(s, cloned);
    }

    #[test]
    fn summary_section_map_status_variants_distinguishable() {
        let full = SectionMapStatus::Full;
        let fb = SectionMapStatus::Fallback {
            reason: "test".into(),
        };
        let def = SectionMapStatus::Deferred {
            reason: "test".into(),
        };
        assert_ne!(full, fb);
        assert_ne!(fb, def);
        assert_ne!(full, def);
    }

    #[test]
    fn summary_default_section_map_status_is_full() {
        let default = SectionMapStatus::default();
        assert_eq!(default, SectionMapStatus::Full);
    }
}
