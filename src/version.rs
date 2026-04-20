//! Version identification from the 6-byte ASCII magic at offset 0.
//!
//! The magic bytes are always literal ASCII in the form `AC10xx`, where the
//! two trailing digits identify the format family. Note that these numbers
//! are *not* consecutive — Autodesk skipped several intermediate codes.
//!
//! References:
//! - ODA Open Design Specification for .dwg files v5.4.1, §3 and §4
//! - Autodesk "Drawing version codes for AutoCAD" knowledge base article

use crate::error::{Error, Result};
use std::fmt;

/// DWG file format version, identified by the 6-byte magic at offset 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Version {
    /// AC1014 — AutoCAD R14 (1997)
    R14,
    /// AC1015 — AutoCAD 2000 / 2000i / 2002 (1999–2002)
    R2000,
    /// AC1018 — AutoCAD 2004 / 2005 / 2006 (2003–2005)
    R2004,
    /// AC1021 — AutoCAD 2007 / 2008 / 2009 (2006–2008)
    R2007,
    /// AC1024 — AutoCAD 2010 / 2011 / 2012 (2009–2011)
    R2010,
    /// AC1027 — AutoCAD 2013 / 2014 / 2015 / 2016 / 2017 (2012–2016)
    R2013,
    /// AC1032 — AutoCAD 2018 onward (2017–current)
    R2018,
}

impl Version {
    /// Parse the 6-byte magic into a version.
    ///
    /// Returns `Err(NotDwg)` if the first two bytes aren't `AC` or
    /// `UnsupportedVersion` if the numeric suffix isn't one this crate
    /// understands.
    pub fn from_magic(magic: &[u8; 6]) -> Result<Self> {
        if &magic[..2] != b"AC" {
            return Err(Error::NotDwg { got: *magic });
        }
        Ok(match magic {
            b"AC1014" => Self::R14,
            b"AC1015" => Self::R2000,
            b"AC1018" => Self::R2004,
            b"AC1021" => Self::R2007,
            b"AC1024" => Self::R2010,
            b"AC1027" => Self::R2013,
            b"AC1032" => Self::R2018,
            _ => return Err(Error::UnsupportedVersion(*magic)),
        })
    }

    /// The exact 6-byte magic string as it appears on disk.
    pub fn magic(self) -> [u8; 6] {
        *match self {
            Self::R14 => b"AC1014",
            Self::R2000 => b"AC1015",
            Self::R2004 => b"AC1018",
            Self::R2007 => b"AC1021",
            Self::R2010 => b"AC1024",
            Self::R2013 => b"AC1027",
            Self::R2018 => b"AC1032",
        }
    }

    /// Human-readable release label.
    pub fn release(self) -> &'static str {
        match self {
            Self::R14 => "R14",
            Self::R2000 => "2000",
            Self::R2004 => "2004",
            Self::R2007 => "2007",
            Self::R2010 => "2010",
            Self::R2013 => "2013",
            Self::R2018 => "2018",
        }
    }

    /// Approximate first-ship year of the first AutoCAD release using this format.
    pub fn year_introduced(self) -> u16 {
        match self {
            Self::R14 => 1997,
            Self::R2000 => 1999,
            Self::R2004 => 2003,
            Self::R2007 => 2006,
            Self::R2010 => 2009,
            Self::R2013 => 2012,
            Self::R2018 => 2017,
        }
    }

    /// True for R2004 and later — any format from AC1018 onward.
    pub fn is_r2004_plus(self) -> bool {
        matches!(
            self,
            Self::R2004 | Self::R2007 | Self::R2010 | Self::R2013 | Self::R2018
        )
    }

    /// True for R13-R15 (pre-R2004) simple header format.
    pub fn is_r13_r15(self) -> bool {
        matches!(self, Self::R14 | Self::R2000)
    }

    /// True for versions that share the R2004 encrypted-header layout.
    ///
    /// R2007 is *not* in this set: spec §5 documents a 33-page delta where
    /// R2007's file header is laid out differently. R2010, R2013, R2018
    /// revert to the R2004 structure with incremental additions.
    pub fn is_r2004_family(self) -> bool {
        matches!(self, Self::R2004 | Self::R2010 | Self::R2013 | Self::R2018)
    }

    /// True for R2007 specifically — format has its own parsing rules in
    /// spec §5. Phase A identifies but does not decrypt R2007 headers.
    pub fn is_r2007(self) -> bool {
        matches!(self, Self::R2007)
    }

    /// True for the set `{R2007, R2010, R2013, R2018}` — the release
    /// line from AC1021 onward.
    ///
    /// The primary meaning callers rely on is **text encoding**: these
    /// versions store `TV` (variable text) and symbol-table entry names
    /// as UTF-16LE bit-streams, whereas R2004 and earlier use 8-bit
    /// bytes. That single observation is load-bearing across
    /// [`crate::tables::read_tv`], `block::decode`, and the attribute
    /// decoders.
    ///
    /// An earlier version of this docstring claimed the predicate
    /// corresponded to "`Sec_Mask` two-layer bitstream obfuscation,"
    /// which is inaccurate — only R2007 uses the two-layer variant; R2010,
    /// R2013, and R2018 revert to the single-layer `Sec_Mask` that R2004
    /// already used. Separate predicates exist for that distinction
    /// ([`Self::is_r2007`] and [`Self::is_r2004_family`]).
    ///
    /// Prefer the semantically-named [`uses_utf16_text`](Self::uses_utf16_text)
    /// at new call sites; `is_r2007_plus` is retained as an alias so
    /// existing callers keep compiling.
    pub fn is_r2007_plus(self) -> bool {
        matches!(self, Self::R2007 | Self::R2010 | Self::R2013 | Self::R2018)
    }

    /// True for formats that store variable-text (`TV`) fields as
    /// UTF-16LE bit-streams rather than 8-bit bytes. This is the
    /// dominant meaning of "R2007-plus" across the rest of the crate.
    ///
    /// Equivalent to [`is_r2007_plus`](Self::is_r2007_plus) at present;
    /// provided as a semantic name for readability at call sites that
    /// are specifically branching on text encoding.
    pub fn uses_utf16_text(self) -> bool {
        self.is_r2007_plus()
    }

    /// True for R2010+ — object-type encoding changed (see spec §2.12).
    pub fn is_r2010_plus(self) -> bool {
        matches!(self, Self::R2010 | Self::R2013 | Self::R2018)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let magic = self.magic();
        write!(
            f,
            "{} ({})",
            std::str::from_utf8(&magic).unwrap_or("?"),
            self.release()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_magics_roundtrip() {
        for v in [
            Version::R14,
            Version::R2000,
            Version::R2004,
            Version::R2007,
            Version::R2010,
            Version::R2013,
            Version::R2018,
        ] {
            assert_eq!(Version::from_magic(&v.magic()).unwrap(), v);
        }
    }

    #[test]
    fn rejects_non_ac_prefix() {
        assert!(matches!(
            Version::from_magic(b"XX1015"),
            Err(Error::NotDwg { .. })
        ));
    }

    #[test]
    fn rejects_unknown_ac_suffix() {
        // AC1009 is R11/R12 (pre-R13), not supported by this Phase A crate.
        assert!(matches!(
            Version::from_magic(b"AC1009"),
            Err(Error::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn family_predicates() {
        assert!(Version::R2018.is_r2004_plus());
        assert!(Version::R2018.is_r2007_plus());
        assert!(Version::R2018.is_r2010_plus());
        assert!(Version::R2018.is_r2004_family());
        assert!(!Version::R14.is_r2004_plus());
        assert!(!Version::R2000.is_r2004_plus());
        assert!(Version::R2000.is_r13_r15());
        assert!(!Version::R2004.is_r13_r15());
        // R2007 is NOT in the R2004 family — it has its own spec chapter.
        assert!(!Version::R2007.is_r2004_family());
        assert!(Version::R2007.is_r2007());
        assert!(Version::R2007.is_r2007_plus());
        // R2010, R2013, R2018 ARE in the R2004 family.
        assert!(Version::R2010.is_r2004_family());
        assert!(Version::R2013.is_r2004_family());
        assert!(Version::R2018.is_r2004_family());
    }
}
