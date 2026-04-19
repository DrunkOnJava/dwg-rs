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
}
