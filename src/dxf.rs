//! DXF text writer — tagged group-code emission.
//!
//! DXF is AutoCAD's ASCII companion format. Each record is a pair of
//! lines: a group code (integer, 0-1071) followed by a value (string,
//! integer, or real number). The format is strictly line-based with
//! a two-line pair per field; sections begin with `0 SECTION` + `2
//! <name>` and end with `0 ENDSEC`; the file ends with `0 EOF`.
//!
//! This module is the emitter foundation — `DxfWriter` tracks the
//! file-level state (current section, pending ENDSEC) and exposes
//! typed helpers (`write_int`, `write_string`, `write_double`,
//! `write_point`) that callers use inside sections. Entity / table
//! / block section emitters (L11-02 through L11-06) will be built
//! on top of this writer.
//!
//! # Minimal example
//!
//! ```
//! use dwg::dxf::DxfWriter;
//! let mut w = DxfWriter::new();
//! w.begin_section("HEADER");
//! w.write_string(9, "$ACADVER");
//! w.write_string(1, "AC1032");
//! w.end_section();
//! w.finish();
//! let dxf = w.take_output();
//! assert!(dxf.contains("$ACADVER"));
//! assert!(dxf.ends_with("EOF\n"));
//! ```
//!
//! # Group-code conventions
//!
//! - 0: primary object type (SECTION, ENDSEC, LINE, LAYER, EOF, …)
//! - 1: primary text value (name, string)
//! - 2: section name / table entry name
//! - 5: hexadecimal handle
//! - 10/20/30: first 3D point X/Y/Z
//! - 11/21/31: second 3D point X/Y/Z
//! - 39: thickness
//! - 40: float (radius, length)
//! - 62: color index (ACI)
//! - 70: integer flag
//! - 100: subclass marker (`AcDbEntity`, `AcDbLine`, …)
//! - 999: comment (optional)
//!
//! Full group-code reference lives in the DXF reference manual
//! (Autodesk's publicly-available ASCII-DXF specification).

use crate::entities::Point3D;

/// DXF version targets. Controls the `$ACADVER` value emitted in the
/// HEADER section and, at the emitter layer, which group codes are
/// valid (e.g. 100 `AcDb*` subclass markers were introduced at R13).
///
/// The writer stores the version as configuration only — it does not
/// validate that callers restrict themselves to the subset of group
/// codes allowed for their chosen target. Section emitters that care
/// about the target version read [`DxfWriter::version`] and branch
/// accordingly.
///
/// ACADVER magic values (AutoCAD's published DXF reference):
///
/// | Variant  | `$ACADVER` | Release year | Notes                                             |
/// |----------|------------|--------------|---------------------------------------------------|
/// | `R12`    | `AC1009`   | 1992         | Last pre-subclass release; no `100 AcDb*` markers |
/// | `R14`    | `AC1014`   | 1997         | First R13+ markers in wide use                    |
/// | `R2000`  | `AC1015`   | 1999         |                                                   |
/// | `R2004`  | `AC1018`   | 2003         |                                                   |
/// | `R2007`  | `AC1021`   | 2006         |                                                   |
/// | `R2010`  | `AC1024`   | 2009         |                                                   |
/// | `R2013`  | `AC1027`   | 2012         |                                                   |
/// | `R2018`  | `AC1032`   | 2017         | Current default — widest reader acceptance        |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DxfVersion {
    R12,
    R14,
    R2000,
    R2004,
    R2007,
    R2010,
    R2013,
    #[default]
    R2018,
}

impl DxfVersion {
    /// The `$ACADVER` header string canonical for this target.
    pub fn acadver(self) -> &'static str {
        match self {
            DxfVersion::R12 => "AC1009",
            DxfVersion::R14 => "AC1014",
            DxfVersion::R2000 => "AC1015",
            DxfVersion::R2004 => "AC1018",
            DxfVersion::R2007 => "AC1021",
            DxfVersion::R2010 => "AC1024",
            DxfVersion::R2013 => "AC1027",
            DxfVersion::R2018 => "AC1032",
        }
    }

    /// Whether this target understands `100 AcDb*` subclass markers.
    /// R13 introduced them; R12 predates them and rejects them.
    pub fn supports_subclass_markers(self) -> bool {
        !matches!(self, DxfVersion::R12)
    }

    /// Parse a target from a case-insensitive string accepted by the
    /// `dwg-to-dxf` CLI's `--version` flag. Accepts either the short
    /// release name (`R12`, `R14`, `R2000`, ...) or the `$ACADVER`
    /// magic (`AC1009`, `AC1014`, ...).
    pub fn parse_cli(s: &str) -> Option<Self> {
        let upper = s.trim().to_uppercase();
        match upper.as_str() {
            "R12" | "AC1009" => Some(DxfVersion::R12),
            "R14" | "AC1014" => Some(DxfVersion::R14),
            "R2000" | "AC1015" => Some(DxfVersion::R2000),
            "R2004" | "AC1018" => Some(DxfVersion::R2004),
            "R2007" | "AC1021" => Some(DxfVersion::R2007),
            "R2010" | "AC1024" => Some(DxfVersion::R2010),
            "R2013" | "AC1027" => Some(DxfVersion::R2013),
            "R2018" | "AC1032" => Some(DxfVersion::R2018),
            _ => None,
        }
    }
}

/// DXF file writer. Stateful: tracks which section is open so
/// callers can't accidentally nest or leak. Version-aware: section
/// emitters may branch on [`DxfWriter::version`] to emit the right
/// `$ACADVER` and the correct subset of group codes.
#[derive(Debug, Clone, Default)]
pub struct DxfWriter {
    output: String,
    in_section: bool,
    finished: bool,
    version: DxfVersion,
}

impl DxfWriter {
    /// Start a fresh, empty DXF document targeting [`DxfVersion::R2018`]
    /// (AC1032) — the widest-accepted modern DXF dialect.
    pub fn new() -> Self {
        DxfWriter::default()
    }

    /// Start a fresh, empty DXF document targeting the given version.
    /// Equivalent to `DxfWriter::new()` followed by
    /// [`DxfWriter::set_version`] but avoids the intermediate default.
    pub fn with_version(version: DxfVersion) -> Self {
        DxfWriter {
            output: String::new(),
            in_section: false,
            finished: false,
            version,
        }
    }

    /// Override the target version on an already-constructed writer.
    /// Must be called before the HEADER section is emitted — changing
    /// version mid-stream will make the emitted `$ACADVER` inconsistent
    /// with the rest of the file.
    pub fn set_version(&mut self, version: DxfVersion) {
        self.version = version;
    }

    /// The DXF target version this writer will emit for.
    pub fn version(&self) -> DxfVersion {
        self.version
    }

    /// Begin a named section. Emits `0 SECTION` + `2 <name>` and
    /// tracks the open state. Panics if a section is already open;
    /// callers must balance begin/end.
    pub fn begin_section(&mut self, name: &str) {
        assert!(
            !self.in_section,
            "DXF: section '{name}' begins while another is open"
        );
        assert!(!self.finished, "DXF: cannot begin section after finish()");
        self.write_pair(0, "SECTION");
        self.write_pair(2, name);
        self.in_section = true;
    }

    /// Close the current section with `0 ENDSEC`.
    pub fn end_section(&mut self) {
        assert!(self.in_section, "DXF: end_section with no section open");
        self.write_pair(0, "ENDSEC");
        self.in_section = false;
    }

    /// Emit a group-code + raw-string value pair.
    pub fn write_string(&mut self, code: i32, value: &str) {
        self.write_pair(code, value);
    }

    /// Emit a group-code + integer value.
    pub fn write_int(&mut self, code: i32, value: i64) {
        self.write_pair(code, &value.to_string());
    }

    /// Emit a group-code + float value.
    pub fn write_double(&mut self, code: i32, value: f64) {
        // DXF expects fixed decimal with enough precision. {:.16}
        // matches f64 round-trip precision.
        self.write_pair(code, &format!("{value:.16}"));
    }

    /// Emit a 3D point as three group-code pairs: (code_x, x),
    /// (code_x + 10, y), (code_x + 20, z). For the primary point on
    /// an entity, `code_x = 10` is canonical; secondary = 11; etc.
    pub fn write_point(&mut self, code_x: i32, p: Point3D) {
        self.write_double(code_x, p.x);
        self.write_double(code_x + 10, p.y);
        self.write_double(code_x + 20, p.z);
    }

    /// Emit a handle (hex string) with group code 5.
    pub fn write_handle(&mut self, handle: u64) {
        self.write_pair(5, &format!("{handle:X}"));
    }

    /// Begin an entity header: `0 <type>` then `5 <handle>`.
    /// Callers continue with additional fields + subclass markers.
    pub fn write_entity_header(&mut self, entity_type: &str, handle: Option<u64>) {
        assert!(self.in_section, "DXF: entity header outside of a section");
        self.write_pair(0, entity_type);
        if let Some(h) = handle {
            self.write_handle(h);
        }
    }

    /// Emit an optional comment (group code 999). DXF readers ignore
    /// these; useful for diagnostic diffing between runs.
    pub fn write_comment(&mut self, text: &str) {
        self.write_pair(999, text);
    }

    /// Close the document with `0 EOF`. Further writes panic.
    pub fn finish(&mut self) {
        assert!(
            !self.in_section,
            "DXF: finish() called with section still open"
        );
        assert!(!self.finished, "DXF: finish() called twice");
        self.write_pair(0, "EOF");
        self.finished = true;
    }

    /// Take ownership of the accumulated output; resets the writer.
    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.output)
    }

    /// Borrow the accumulated output without consuming it. Primarily
    /// for testing / diagnostics.
    pub fn as_str(&self) -> &str {
        &self.output
    }

    fn write_pair(&mut self, code: i32, value: &str) {
        // DXF is line-based with CR+LF canonically, but both CR+LF
        // and plain LF are widely tolerated. We emit LF for
        // deterministic output (git-diff-friendly).
        self.output.push_str(&format!("{code:>3}\n"));
        self.output.push_str(value);
        self.output.push('\n');
    }
}
