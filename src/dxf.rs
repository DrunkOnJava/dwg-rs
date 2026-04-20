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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DxfVersion {
    R12,
    R14,
    R2000,
    R2004,
    R2007,
    R2010,
    R2013,
    R2018,
}

impl Default for DxfVersion {
    fn default() -> Self {
        DxfVersion::R2018
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_finish_is_just_eof() {
        let mut w = DxfWriter::new();
        w.finish();
        assert_eq!(w.take_output(), "  0\nEOF\n");
    }

    #[test]
    fn section_begin_end_wraps_contents() {
        let mut w = DxfWriter::new();
        w.begin_section("HEADER");
        w.write_string(9, "$TEST");
        w.write_int(70, 42);
        w.end_section();
        w.finish();
        let out = w.take_output();
        assert!(out.contains("SECTION"));
        assert!(out.contains("HEADER"));
        assert!(out.contains("$TEST"));
        assert!(out.contains("42"));
        assert!(out.contains("ENDSEC"));
        assert!(out.ends_with("EOF\n"));
    }

    #[test]
    fn pair_format_is_three_digit_padded_code_then_value() {
        let mut w = DxfWriter::new();
        w.write_string(9, "$V");
        let s = w.as_str();
        assert!(s.starts_with("  9\n$V\n"));
    }

    #[test]
    fn double_has_16_decimal_places() {
        let mut w = DxfWriter::new();
        w.write_double(40, std::f64::consts::PI);
        assert!(w.as_str().contains("3.1415926535897931"));
    }

    #[test]
    fn point_emits_three_components_with_offsets() {
        let mut w = DxfWriter::new();
        w.write_point(10, Point3D::new(1.0, 2.0, 3.0));
        let s = w.as_str();
        assert!(s.contains(" 10\n"));
        assert!(s.contains(" 20\n"));
        assert!(s.contains(" 30\n"));
    }

    #[test]
    fn handle_is_hex_uppercase() {
        let mut w = DxfWriter::new();
        w.write_handle(0x1A3F);
        assert!(w.as_str().contains("1A3F"));
    }

    #[test]
    #[should_panic(expected = "section")]
    fn nested_section_panics() {
        let mut w = DxfWriter::new();
        w.begin_section("A");
        w.begin_section("B");
    }

    #[test]
    #[should_panic(expected = "end_section")]
    fn end_section_without_begin_panics() {
        let mut w = DxfWriter::new();
        w.end_section();
    }

    #[test]
    #[should_panic(expected = "section still open")]
    fn finish_with_open_section_panics() {
        let mut w = DxfWriter::new();
        w.begin_section("A");
        w.finish();
    }

    #[test]
    #[should_panic(expected = "twice")]
    fn double_finish_panics() {
        let mut w = DxfWriter::new();
        w.finish();
        w.finish();
    }

    #[test]
    fn entity_header_emits_type_and_handle() {
        let mut w = DxfWriter::new();
        w.begin_section("ENTITIES");
        w.write_entity_header("LINE", Some(0x83));
        w.end_section();
        w.finish();
        let s = w.take_output();
        assert!(s.contains("LINE"));
        assert!(s.contains("83"));
    }

    #[test]
    fn comments_use_group_999() {
        let mut w = DxfWriter::new();
        w.write_comment("generated by dwg-rs");
        assert!(w.as_str().contains("999\n"));
        assert!(w.as_str().contains("generated by dwg-rs"));
    }

    #[test]
    fn take_output_clears_buffer() {
        let mut w = DxfWriter::new();
        w.write_comment("a");
        let _ = w.take_output();
        assert_eq!(w.as_str(), "");
    }

    #[test]
    fn default_version_is_r2018() {
        let w = DxfWriter::new();
        assert_eq!(w.version(), DxfVersion::R2018);
        assert_eq!(w.version().acadver(), "AC1032");
    }

    #[test]
    fn with_version_sets_target() {
        let w = DxfWriter::with_version(DxfVersion::R12);
        assert_eq!(w.version(), DxfVersion::R12);
        assert_eq!(w.version().acadver(), "AC1009");
    }

    #[test]
    fn set_version_overrides_target() {
        let mut w = DxfWriter::new();
        w.set_version(DxfVersion::R2000);
        assert_eq!(w.version(), DxfVersion::R2000);
        assert_eq!(w.version().acadver(), "AC1015");
    }

    #[test]
    fn subclass_markers_gated_by_version() {
        assert!(!DxfVersion::R12.supports_subclass_markers());
        assert!(DxfVersion::R14.supports_subclass_markers());
        assert!(DxfVersion::R2000.supports_subclass_markers());
        assert!(DxfVersion::R2018.supports_subclass_markers());
    }

    #[test]
    fn acadver_magic_strings_match_spec() {
        assert_eq!(DxfVersion::R12.acadver(), "AC1009");
        assert_eq!(DxfVersion::R14.acadver(), "AC1014");
        assert_eq!(DxfVersion::R2000.acadver(), "AC1015");
        assert_eq!(DxfVersion::R2004.acadver(), "AC1018");
        assert_eq!(DxfVersion::R2007.acadver(), "AC1021");
        assert_eq!(DxfVersion::R2010.acadver(), "AC1024");
        assert_eq!(DxfVersion::R2013.acadver(), "AC1027");
        assert_eq!(DxfVersion::R2018.acadver(), "AC1032");
    }

    #[test]
    fn parse_cli_accepts_release_and_acadver() {
        assert_eq!(DxfVersion::parse_cli("R12"), Some(DxfVersion::R12));
        assert_eq!(DxfVersion::parse_cli("r12"), Some(DxfVersion::R12));
        assert_eq!(DxfVersion::parse_cli("AC1009"), Some(DxfVersion::R12));
        assert_eq!(DxfVersion::parse_cli("ac1032"), Some(DxfVersion::R2018));
        assert_eq!(DxfVersion::parse_cli("R2018"), Some(DxfVersion::R2018));
        assert_eq!(DxfVersion::parse_cli(" R2013 "), Some(DxfVersion::R2013));
        assert_eq!(DxfVersion::parse_cli("bogus"), None);
    }
}
