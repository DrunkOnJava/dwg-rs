//! Named sections inside a DWG file.
//!
//! # R13-R15 (spec §3.2.6)
//!
//! A flat list of `(record_number, seeker, size)` triples. Record numbers
//! 0..=6 have fixed meanings; the record itself tells us where to find the
//! payload.
//!
//! # R2004+ (spec §4.5)
//!
//! Sections are named (UTF-16LE strings up to 64 bytes) and looked up via
//! the section info table. A section can be flagged compressed (LZ77) and/or
//! encrypted (Sec_Mask XOR).
//!
//! # Decode diagnostics
//!
//! [`Section`] carries a small set of post-hoc diagnostic fields
//! ([`Section::decode_attempted`], [`Section::decode_succeeded`],
//! [`Section::decompressed_bytes`]) that a caller may populate after
//! running [`crate::reader::DwgFile::read_section`] to remember the
//! outcome alongside the section's locator metadata. The fields are
//! optional and default to "not attempted"; [`Section::diagnostics`]
//! renders them as a printable [`SectionDiagnostics`] view.

use std::fmt;

/// A named section in a DWG file (either version family).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Canonical name — for R13-R15 this is the record slot label
    /// ("Header variables", "Class section", ...); for R2004+ this is the
    /// on-disk string (e.g. "AcDb:Header", "AcDb:Preview").
    pub name: String,
    /// Classification of this section for consumers that want to switch on kind.
    pub kind: SectionKind,
    /// Absolute byte offset into the file where the section payload begins.
    pub offset: u64,
    /// Payload size in bytes. For compressed sections this is the on-disk
    /// compressed size, not the decompressed size.
    pub size: u64,
    /// R2004+: is this section's payload LZ77-compressed?
    pub compressed: bool,
    /// R2004+: is this section's payload XOR-encrypted (Sec_Mask)?
    pub encrypted: bool,
    /// Post-hoc diagnostic: did a caller try to decode / decompress this
    /// section? Populated by the reader via [`Section::mark_decode_attempt`]
    /// after calling [`crate::reader::DwgFile::read_section`]. Defaults to
    /// `false` — the bare section list carries locator metadata only.
    pub decode_attempted: bool,
    /// Post-hoc diagnostic: did the decode / decompress succeed? Only
    /// meaningful when `decode_attempted` is `true`.
    pub decode_succeeded: bool,
    /// Post-hoc diagnostic: number of bytes produced by the decompress /
    /// decrypt pipeline, when one ran. `None` when no attempt has been
    /// made or the attempt failed before producing output.
    pub decompressed_bytes: Option<usize>,
}

impl Default for Section {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: SectionKind::Unknown,
            offset: 0,
            size: 0,
            compressed: false,
            encrypted: false,
            decode_attempted: false,
            decode_succeeded: false,
            decompressed_bytes: None,
        }
    }
}

impl Section {
    /// Record a successful decode attempt with the number of bytes
    /// produced by the decompress / decrypt pipeline. Idempotent —
    /// safe to call twice from different call sites.
    pub fn mark_decode_success(&mut self, decompressed_bytes: usize) {
        self.decode_attempted = true;
        self.decode_succeeded = true;
        self.decompressed_bytes = Some(decompressed_bytes);
    }

    /// Record a failed decode attempt. Leaves `decompressed_bytes`
    /// unset if the failure happened before any output was produced.
    pub fn mark_decode_failure(&mut self) {
        self.decode_attempted = true;
        self.decode_succeeded = false;
    }

    /// Ratio of decompressed to on-disk compressed size. `None` when
    /// no decode has been attempted, when the attempt failed before
    /// producing output, or when the on-disk size is zero (which
    /// would divide by zero).
    ///
    /// A ratio of `1.0` means the section was uncompressed or
    /// decompressed to exactly the on-disk size; larger values
    /// indicate the expected "decompression amplification" of a
    /// compressed section. Useful as a coarse reasonableness check
    /// when enforcing [`crate::limits::OpenLimits::max_section_bytes`].
    pub fn compression_ratio(&self) -> Option<f64> {
        let decomp = self.decompressed_bytes?;
        if self.size == 0 {
            return None;
        }
        Some(decomp as f64 / self.size as f64)
    }

    /// Render the section's post-hoc diagnostic fields as a printable
    /// [`SectionDiagnostics`] view. Used by the `dwg-info` / `dwg-dump`
    /// CLI output so operators can see which sections a given tool
    /// actually touched without grepping for them.
    pub fn diagnostics(&self) -> SectionDiagnostics<'_> {
        SectionDiagnostics {
            name: &self.name,
            kind: self.kind,
            compressed: self.compressed,
            encrypted: self.encrypted,
            on_disk_bytes: self.size,
            decode_attempted: self.decode_attempted,
            decode_succeeded: self.decode_succeeded,
            decompressed_bytes: self.decompressed_bytes,
            compression_ratio: self.compression_ratio(),
        }
    }
}

/// Printable diagnostic view over a [`Section`]. Kept as a flat struct
/// of primitives so it can be formatted directly by a CLI or serialized
/// by a future JSON output path without pulling a `serde` feature in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionDiagnostics<'a> {
    /// Borrow of the section's canonical name.
    pub name: &'a str,
    /// Section classification.
    pub kind: SectionKind,
    /// LZ77-compressed on disk.
    pub compressed: bool,
    /// Sec_Mask XOR-encrypted on disk.
    pub encrypted: bool,
    /// On-disk byte size (compressed size when `compressed` is true).
    pub on_disk_bytes: u64,
    /// A caller invoked the decode path on this section.
    pub decode_attempted: bool,
    /// The decode path produced output.
    pub decode_succeeded: bool,
    /// Decompressed byte count, when a successful decode captured it.
    pub decompressed_bytes: Option<usize>,
    /// `decompressed_bytes / on_disk_bytes`, when both are known.
    pub compression_ratio: Option<f64>,
}

impl<'a> fmt::Display for SectionDiagnostics<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{name:<28} kind={kind:<12} on_disk={on_disk:>10} bytes",
            name = self.name,
            kind = self.kind.short_label(),
            on_disk = self.on_disk_bytes,
        )?;
        if self.compressed {
            f.write_str(" [compressed]")?;
        }
        if self.encrypted {
            f.write_str(" [encrypted]")?;
        }
        if !self.decode_attempted {
            f.write_str(" [decode: not attempted]")?;
        } else if self.decode_succeeded {
            match (self.decompressed_bytes, self.compression_ratio) {
                (Some(b), Some(r)) => {
                    write!(f, " [decoded: {b} bytes, ratio {r:.2}x]")?;
                }
                (Some(b), None) => {
                    write!(f, " [decoded: {b} bytes]")?;
                }
                _ => f.write_str(" [decoded: ok]")?,
            }
        } else {
            f.write_str(" [decode: failed]")?;
        }
        Ok(())
    }
}

/// Classification of a section by role.
///
/// The enumeration covers both R13-R15 record numbers and the canonical
/// R2004+ section names. `Unknown` is used for values this crate doesn't
/// recognize yet — the file is still readable, we just don't give the
/// section a friendly classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// R13-R15 record 0 / R2004+ `AcDb:Header` — drawing header variables.
    Header,
    /// R2004+ `AcDb:AuxHeader` — auxiliary header.
    AuxHeader,
    /// R13-R15 record 1 / R2004+ `AcDb:Classes` — class definitions.
    Classes,
    /// R13-R15 record 2 / R2004+ `AcDb:Handles` — object map / handle list.
    Handles,
    /// R2004+ `AcDb:Template` — MEASUREMENT system variable.
    Template,
    /// R2004+ `AcDb:ObjFreeSpace` — object-free-space bookkeeping.
    ObjFreeSpace,
    /// R2004+ `AcDb:AcDbObjects` — database objects.
    Objects,
    /// R2004+ `AcDb:RevHistory` — revision history.
    RevHistory,
    /// R2004+ `AcDb:SummaryInfo` — title/subject/author fields.
    SummaryInfo,
    /// R13-R15 image seeker / R2004+ `AcDb:Preview` — thumbnail bitmap.
    Preview,
    /// R2004+ `AcDb:AppInfo` — author application identification.
    AppInfo,
    /// R2004+ `AcDb:AppInfoHistory` — creator-application trail.
    AppInfoHistory,
    /// R2004+ `AcDb:FileDepList` — referenced fonts & external images.
    FileDepList,
    /// R2004+ `AcDb:Security` — password / encryption metadata.
    Security,
    /// R2004+ `AcDb:VBAProject` — VBA project data.
    VbaProject,
    /// R2004+ `AcDb:Signature` — optional digital signature.
    Signature,
    /// R2004+ `AcDb:AcDsPrototype_1b` — ACIS model data store.
    AcDsData,
    /// R2004+ system sections (SectionMap, SectionPageMap).
    SystemSection,
    /// R13-R15 record 4 — MEASUREMENT / optional data pointer.
    R13MeasurementPtr,
    /// Catch-all for unrecognized records / names.
    Unknown,
}

impl SectionKind {
    /// Map an R13-R15 record number to its kind. Record numbers above 6
    /// are treated as unknown per spec §3.2.6 remark.
    pub fn from_r13_record(n: u8) -> Self {
        match n {
            0 => Self::Header,
            1 => Self::Classes,
            2 => Self::Handles,
            3 => Self::Unknown, // C3+ marker — classified later
            4 => Self::R13MeasurementPtr,
            _ => Self::Unknown,
        }
    }

    /// Map an R2004+ on-disk section name to its kind.
    pub fn from_r2004_name(name: &str) -> Self {
        match name {
            "AcDb:Header" => Self::Header,
            "AcDb:AuxHeader" => Self::AuxHeader,
            "AcDb:Classes" => Self::Classes,
            "AcDb:Handles" => Self::Handles,
            "AcDb:Template" => Self::Template,
            "AcDb:ObjFreeSpace" => Self::ObjFreeSpace,
            "AcDb:AcDbObjects" => Self::Objects,
            "AcDb:RevHistory" => Self::RevHistory,
            "AcDb:SummaryInfo" => Self::SummaryInfo,
            "AcDb:Preview" => Self::Preview,
            "AcDb:AppInfo" => Self::AppInfo,
            "AcDb:AppInfoHistory" => Self::AppInfoHistory,
            "AcDb:FileDepList" => Self::FileDepList,
            "AcDb:Security" => Self::Security,
            "AcDb:VBAProject" => Self::VbaProject,
            "AcDb:Signature" => Self::Signature,
            s if s.starts_with("AcDb:AcDs") => Self::AcDsData,
            _ => Self::Unknown,
        }
    }

    /// Friendly short label — one word each, used in CLI output.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::AuxHeader => "auxheader",
            Self::Classes => "classes",
            Self::Handles => "handles",
            Self::Template => "template",
            Self::ObjFreeSpace => "objfree",
            Self::Objects => "objects",
            Self::RevHistory => "revhist",
            Self::SummaryInfo => "summary",
            Self::Preview => "preview",
            Self::AppInfo => "appinfo",
            Self::AppInfoHistory => "appinfohist",
            Self::FileDepList => "filedep",
            Self::Security => "security",
            Self::VbaProject => "vba",
            Self::Signature => "signature",
            Self::AcDsData => "acds",
            Self::SystemSection => "system",
            Self::R13MeasurementPtr => "measurement",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for SectionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.short_label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r13_record_numbers_map_correctly() {
        assert_eq!(SectionKind::from_r13_record(0), SectionKind::Header);
        assert_eq!(SectionKind::from_r13_record(1), SectionKind::Classes);
        assert_eq!(SectionKind::from_r13_record(2), SectionKind::Handles);
        assert_eq!(
            SectionKind::from_r13_record(4),
            SectionKind::R13MeasurementPtr
        );
        assert_eq!(SectionKind::from_r13_record(99), SectionKind::Unknown);
    }

    #[test]
    fn r2004_names_map_correctly() {
        assert_eq!(
            SectionKind::from_r2004_name("AcDb:Header"),
            SectionKind::Header
        );
        assert_eq!(
            SectionKind::from_r2004_name("AcDb:Preview"),
            SectionKind::Preview
        );
        assert_eq!(
            SectionKind::from_r2004_name("AcDb:AcDsPrototype_1b"),
            SectionKind::AcDsData
        );
        assert_eq!(
            SectionKind::from_r2004_name("made-up-name"),
            SectionKind::Unknown
        );
    }

    #[test]
    fn short_labels_stable_for_cli() {
        // The CLI promises stable short labels for grep-ability; lock them
        // in with a test so accidental renames break this suite loudly.
        assert_eq!(SectionKind::Header.short_label(), "header");
        assert_eq!(SectionKind::Handles.short_label(), "handles");
        assert_eq!(SectionKind::Preview.short_label(), "preview");
    }

    #[test]
    fn default_section_has_no_decode_attempt() {
        let s = Section::default();
        assert!(!s.decode_attempted);
        assert!(!s.decode_succeeded);
        assert_eq!(s.decompressed_bytes, None);
        assert_eq!(s.compression_ratio(), None);
    }

    #[test]
    fn mark_decode_success_sets_all_three() {
        let mut s = Section {
            name: "AcDb:Header".to_string(),
            kind: SectionKind::Header,
            offset: 0x120,
            size: 200,
            compressed: true,
            encrypted: true,
            ..Default::default()
        };
        s.mark_decode_success(800);
        assert!(s.decode_attempted);
        assert!(s.decode_succeeded);
        assert_eq!(s.decompressed_bytes, Some(800));
        // 800 / 200 = 4.0
        assert_eq!(s.compression_ratio(), Some(4.0));
    }

    #[test]
    fn mark_decode_failure_does_not_set_decompressed_bytes() {
        let mut s = Section::default();
        s.mark_decode_failure();
        assert!(s.decode_attempted);
        assert!(!s.decode_succeeded);
        assert_eq!(s.decompressed_bytes, None);
    }

    #[test]
    fn compression_ratio_is_none_when_on_disk_size_is_zero() {
        let mut s = Section {
            size: 0,
            ..Default::default()
        };
        s.mark_decode_success(500);
        assert_eq!(s.compression_ratio(), None);
    }

    #[test]
    fn diagnostics_view_renders_human_readable() {
        let mut s = Section {
            name: "AcDb:Header".to_string(),
            kind: SectionKind::Header,
            offset: 0x120,
            size: 200,
            compressed: true,
            encrypted: false,
            ..Default::default()
        };
        s.mark_decode_success(800);
        let out = format!("{}", s.diagnostics());
        assert!(out.contains("AcDb:Header"));
        assert!(out.contains("header"));
        assert!(out.contains("compressed"));
        assert!(out.contains("decoded"));
        assert!(out.contains("800"));
    }

    #[test]
    fn diagnostics_view_flags_not_attempted() {
        let s = Section {
            name: "AcDb:Preview".to_string(),
            kind: SectionKind::Preview,
            size: 12345,
            ..Default::default()
        };
        let out = format!("{}", s.diagnostics());
        assert!(out.contains("not attempted"));
    }
}
