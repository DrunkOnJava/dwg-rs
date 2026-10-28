//! ASCII DXF reader: every record of a DXF file, keyed by its handle.
//!
//! This is the reading half of the DXF value oracle (#99, #100). AutoCAD's
//! own DXF export of a drawing is the reference its DWG decode is held to:
//! a decoder whose field list closes on its data-stream boundary proves only
//! that the widths add up, and agreement with the export, record by record,
//! is what shows the values are right. The reader therefore keeps everything
//! the file says: every group of every record, in file order, with its value
//! typed by group code, and an index from handle to record.
//!
//! Built behind the `dxf-reader` feature: it is a measuring instrument, not
//! part of the DWG reader's surface.
//!
//! # Grammar
//!
//! From Autodesk's public *DXF Reference*. A file is a sequence of group
//! pairs, a group-code line then a value line. `0 SECTION` / `2 <name>` opens
//! a section, `0 ENDSEC` closes it and `0 EOF` ends the file. Inside HEADER
//! each variable is `9 $NAME` followed by its value groups. Inside every
//! other section a `0 <TYPE>` group starts a record that runs to the next
//! `0`. A record's handle is its group 5, or group 105 on a DIMSTYLE; its
//! owner is the first group 330 outside the `102 {...}` / `102 }` brackets,
//! whose 330 and 360 groups are reactors and the extension dictionary.
//! An XRECORD's groups after its `100 AcDbXrecord` marker are the
//! application's data: the Reference's XRECORD page gives codes 1 to 369
//! (but 5 and 105) to the application "in any way", so a `102` there is a
//! value, not a bracket.
//!
//! Two readings are measured on AutoCAD's own exports rather than taken from
//! the Reference's tables:
//!
//! - On a DIMSTYLE, group 5 is `DIMBLK`, a block name, not a handle: the R12
//!   exports write it empty (`5` / `""`), which is why DIMSTYLE carries its
//!   handle in 105. It is typed as text there.
//! - A `102` bracket closes with `}` or with `<name>}`: the AC1015 to AC1021
//!   exports of `ellipse_*` write XRECORD data as `{ATTRRECORD` ...
//!   `ATTRRECORD}`.
//!
//! Values are typed by the *DXF Reference*'s "Group Code Value Types"
//! table:
//!
//! | Codes | Type |
//! |---|---|
//! | 0-4, 6-9, 100-102, 300-309, 410-419, 430-439, 470-479, 999, 1000-1003, 1006-1009 | text |
//! | 5, 105, 320-369, 390-399, 480-481, 1005 | handle (hexadecimal) |
//! | 10-59, 110-149, 210-239, 460-469, 1010-1059 | real |
//! | 60-79, 170-179, 270-289, 370-389, 400-409, 1060-1070 | 16-bit integer |
//! | 90-99, 420-429, 440-459, 1071 | 32-bit integer |
//! | 160-169 | 64-bit integer |
//! | 290-299 | boolean (0 or 1) |
//! | 310-319, 1004 | binary chunk (hexadecimal) |
//!
//! A code outside the table is an error, not a string.
//!
//! # How it can fail, and what it does
//!
//! Written down before the code, per the crate's rule for isolated
//! components. Every case returns a [`DxfError`]; none guesses.
//!
//! 1. Binary DXF (the `AutoCAD Binary DXF` sentinel), a UTF-8 byte-order
//!    mark or UTF-16 text: refused as an encoding this reader does not read.
//! 2. More bytes, lines, groups or records than [`DxfLimits`] allows, or one
//!    line longer than its cap: refused before the allocation it would need.
//! 3. CRLF and bare LF line endings in one file: refused.
//! 4. A group code with no value line after it: refused (truncated).
//! 5. A group-code line that is not an integer, or a code outside the table
//!    above: refused.
//! 6. A value that does not parse as its code's type: a real that is not a
//!    finite decimal, an integer outside its width, a boolean other than 0 or
//!    1, a handle that is not 1 to 16 hexadecimal digits, a binary chunk of
//!    odd length or with a non-hexadecimal digit: refused.
//! 7. Text that is not UTF-8 from AC1021 on: refused. Before AC1021, text
//!    whose bytes leave 7-bit ASCII is in the `$DWGCODEPAGE` code page (or
//!    one the file does not declare, as in R12): it is kept as its bytes
//!    ([`DxfValue::CodePageText`]) rather than decoded through an assumed
//!    code page.
//! 8. Structure: a group outside any section, `SECTION` not followed by
//!    `2 <name>`, a section opened inside another or never closed, `ENDSEC`,
//!    `ENDTAB` or `ENDBLK` with nothing open, a `TABLE` or `BLOCK` left open
//!    at its section's end, no `EOF`, or anything after it: refused.
//! 9. HEADER: a value group before the first `9 $NAME`, a `9` value that is
//!    not a `$` name, or a variable named twice: refused.
//! 10. A `102 {` with no closing `102` before the record ends, or a closing
//!     `102` with no open bracket, outside an XRECORD's data: refused.
//! 11. From AC1012 on, a record in TABLES, BLOCKS, ENTITIES or OBJECTS with no
//!     handle (other than the `ENDTAB` marker), a handle of 0, or two records
//!     with one handle: refused. Before AC1012 handles are optional (R12 has
//!     `$HANDLING`) but still unique.
//!
//! What the reader accepts and records rather than refusing: `999` comments
//! anywhere (kept as text groups inside a record; elsewhere skipped,
//! including between `9 $NAME` and its value and between `SECTION` and its
//! name, so they never stand in for a value), groups
//! that sit directly after a section's name before its first record (the
//! THUMBNAILIMAGE section is written that way; kept on the section), and
//! sections other than the six named ones (kept, with their records).

use std::collections::BTreeMap;
use std::fmt;

/// Caps on what one DXF file may claim, checked before the allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DxfLimits {
    /// Largest input accepted, in bytes.
    pub max_bytes: usize,
    /// Largest number of group pairs.
    pub max_groups: usize,
    /// Largest number of `0`-records across all sections.
    pub max_records: usize,
    /// Longest single line, in bytes, line ending excluded.
    pub max_line_len: usize,
}

impl Default for DxfLimits {
    fn default() -> Self {
        Self {
            max_bytes: 512 * 1024 * 1024,
            max_groups: 32 * 1024 * 1024,
            max_records: 4 * 1024 * 1024,
            max_line_len: 1024 * 1024,
        }
    }
}

/// Why a DXF file was refused. `line` is the 1-based line of the group code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DxfError {
    /// The input is binary DXF, UTF-16 or carries a byte-order mark.
    UnsupportedEncoding(&'static str),
    /// A [`DxfLimits`] cap was reached.
    LimitExceeded {
        /// Which cap.
        what: &'static str,
        /// Its value.
        limit: usize,
    },
    /// The file breaks the grammar at `line`.
    Malformed {
        /// 1-based line number of the offending group code.
        line: usize,
        /// What is wrong there.
        reason: String,
    },
}

impl fmt::Display for DxfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedEncoding(what) => write!(f, "unsupported DXF encoding: {what}"),
            Self::LimitExceeded { what, limit } => {
                write!(f, "DXF limit exceeded: {what} (cap {limit})")
            }
            Self::Malformed { line, reason } => write!(f, "malformed DXF at line {line}: {reason}"),
        }
    }
}

impl std::error::Error for DxfError {}

fn malformed(line: usize, reason: impl Into<String>) -> DxfError {
    DxfError::Malformed {
        line,
        reason: reason.into(),
    }
}

/// A group value, typed by its code (see the module table).
#[derive(Debug, Clone, PartialEq)]
pub enum DxfValue {
    /// Text, decoded from ASCII or (AC1021 and later) UTF-8, line ending removed.
    Text(String),
    /// Text before AC1021 with bytes outside 7-bit ASCII, kept undecoded: its
    /// code page is `$DWGCODEPAGE`, or undeclared.
    CodePageText(Vec<u8>),
    /// A real, with the number of significant digits the file printed.
    Real {
        /// The parsed value.
        value: f64,
        /// Significant digits in the printed mantissa, trailing zeros dropped.
        digits: u8,
    },
    /// A 16-bit integer.
    Int16(i16),
    /// A 32-bit integer.
    Int32(i32),
    /// A 64-bit integer.
    Int64(i64),
    /// A boolean (written 0 or 1).
    Bool(bool),
    /// A handle, written as hexadecimal.
    Handle(u64),
    /// A binary chunk, written as hexadecimal.
    Binary(Vec<u8>),
}

impl DxfValue {
    /// The value as text, if it is a text group.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    /// The value as a real, if it is a real group.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Real { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// The value widened to `i64`, if it is an integer or boolean group.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int16(v) => Some(i64::from(*v)),
            Self::Int32(v) => Some(i64::from(*v)),
            Self::Int64(v) => Some(*v),
            Self::Bool(b) => Some(i64::from(*b)),
            _ => None,
        }
    }

    /// The value as a handle, if it is a handle group.
    pub fn as_handle(&self) -> Option<u64> {
        match self {
            Self::Handle(h) => Some(*h),
            _ => None,
        }
    }
}

impl fmt::Display for DxfValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(s) => write!(f, "{s:?}"),
            Self::CodePageText(b) => write!(f, "<{} bytes of code-page text>", b.len()),
            Self::Real { value, .. } => write!(f, "{value:?}"),
            Self::Int16(v) => write!(f, "{v}"),
            Self::Int32(v) => write!(f, "{v}"),
            Self::Int64(v) => write!(f, "{v}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Handle(h) => write!(f, "#{h:X}"),
            Self::Binary(b) => write!(f, "<{} bytes>", b.len()),
        }
    }
}

/// One group: its code, its typed value and the line its code sits on.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfGroup {
    /// The group code.
    pub code: u16,
    /// The typed value.
    pub value: DxfValue,
    /// 1-based line number of the group code.
    pub line: usize,
}

/// One `0`-record: an entity, object, table, table entry, block marker or class.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfRecord {
    /// The `0` group's value: `LINE`, `LAYER`, `TABLE`, `DICTIONARY`, ...
    pub kind: String,
    /// Index into [`DxfDocument::sections`] of the section holding it.
    pub section: usize,
    /// Group 5 (group 105 on a DIMSTYLE), if present.
    pub handle: Option<u64>,
    /// The first group 330 outside the `102` brackets, if present.
    pub owner: Option<u64>,
    /// Every group after the `0`, in file order.
    pub groups: Vec<DxfGroup>,
    /// 1-based line number of the `0` group.
    pub line: usize,
}

impl DxfRecord {
    /// The first group with `code`, anywhere in the record.
    pub fn first(&self, code: u16) -> Option<&DxfValue> {
        self.groups
            .iter()
            .find(|g| g.code == code)
            .map(|g| &g.value)
    }

    /// Every group with `code`, in file order.
    pub fn all(&self, code: u16) -> impl Iterator<Item = &DxfValue> + '_ {
        self.groups
            .iter()
            .filter(move |g| g.code == code)
            .map(|g| &g.value)
    }

    /// The groups outside the `102 {...}` / `102 }` brackets, the brackets
    /// themselves excluded, then all of an XRECORD's data groups.
    pub fn top_level(&self) -> impl Iterator<Item = &DxfGroup> + '_ {
        let data = self.data_start();
        let mut depth = 0usize;
        self.groups.iter().enumerate().filter_map(move |(i, g)| {
            if i >= data {
                return Some(g);
            }
            if g.code == 102 {
                match g.value.as_str() {
                    Some(s) if s.starts_with('{') => depth += 1,
                    Some(s) if s.ends_with('}') => depth = depth.saturating_sub(1),
                    _ => {}
                }
                return None;
            }
            (depth == 0).then_some(g)
        })
    }

    /// Index of an XRECORD's first data group, the one after `100
    /// AcDbXrecord` (§20.4.104); `groups.len()` on every other record. The
    /// groups before it are structural (`102` brackets, owner, subclass
    /// markers) and the groups from it on are an application's.
    pub fn data_start(&self) -> usize {
        let len = self.groups.len();
        if self.kind != "XRECORD" {
            return len;
        }
        self.groups
            .iter()
            .position(|g| g.code == 100 && g.value.as_str() == Some("AcDbXrecord"))
            .map_or(len, |i| i + 1)
    }
}

/// One section: its name and any groups between its name and first record.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfSection {
    /// `HEADER`, `CLASSES`, `TABLES`, `BLOCKS`, `ENTITIES`, `OBJECTS`, ...
    pub name: String,
    /// Groups written directly under the section name (THUMBNAILIMAGE).
    pub leading_groups: Vec<DxfGroup>,
}

/// One HEADER variable: its `$NAME` and its value groups.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfHeaderVar {
    /// The variable's name, `$` included.
    pub name: String,
    /// Its value groups, in file order (three for a 3D point).
    pub groups: Vec<DxfGroup>,
}

/// A parsed DXF file.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfDocument {
    /// Every section, in file order.
    pub sections: Vec<DxfSection>,
    /// The HEADER variables, in file order.
    pub header: Vec<DxfHeaderVar>,
    /// Every `0`-record of every section other than HEADER, in file order.
    pub records: Vec<DxfRecord>,
    header_index: BTreeMap<String, usize>,
    handle_index: BTreeMap<u64, usize>,
    real_digits: u8,
    real_decimals: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueType {
    Text,
    Handle,
    Real,
    Int16,
    Int32,
    Int64,
    Bool,
    Binary,
}

/// The *DXF Reference* "Group Code Value Types" table; `None` outside it.
fn value_type(code: u16) -> Option<ValueType> {
    use ValueType::*;
    Some(match code {
        5 | 105 => Handle,
        0..=9 => Text,
        10..=59 => Real,
        60..=79 => Int16,
        90..=99 => Int32,
        100..=102 => Text,
        110..=149 => Real,
        160..=169 => Int64,
        170..=179 => Int16,
        210..=239 => Real,
        270..=289 => Int16,
        290..=299 => Bool,
        300..=309 => Text,
        310..=319 => Binary,
        320..=369 => Handle,
        370..=389 => Int16,
        390..=399 => Handle,
        400..=409 => Int16,
        410..=419 => Text,
        420..=429 => Int32,
        430..=439 => Text,
        440..=459 => Int32,
        460..=469 => Real,
        470..=479 => Text,
        480..=481 => Handle,
        999 => Text,
        1004 => Binary,
        1005 => Handle,
        1000..=1009 => Text,
        1010..=1059 => Real,
        1060..=1070 => Int16,
        1071 => Int32,
        _ => return None,
    })
}

/// Significant digits in a printed real: mantissa digits with leading and
/// trailing zeros removed (`50.0` has 1, `0.4242640687119287` has 16).
/// Digits after the decimal point of a real printed without an exponent;
/// 0 for one printed with an exponent.
fn decimal_places(raw: &[u8]) -> u8 {
    if raw.iter().any(|b| matches!(b, b'e' | b'E')) {
        return 0;
    }
    let after = raw.iter().position(|&b| b == b'.').map_or(0, |dot| {
        raw[dot + 1..].iter().filter(|b| b.is_ascii_digit()).count()
    });
    u8::try_from(after).unwrap_or(u8::MAX)
}

fn significant_digits(text: &str) -> u8 {
    let mantissa = text.split(['e', 'E']).next().unwrap_or("");
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let start = digits.iter().position(|&d| d != b'0');
    let end = digits.iter().rposition(|&d| d != b'0');
    match (start, end) {
        (Some(s), Some(e)) => u8::try_from(e - s + 1).unwrap_or(u8::MAX),
        _ => 0,
    }
}

fn parse_real(text: &str, line: usize) -> Result<DxfValue, DxfError> {
    let t = text.trim();
    let ok = !t.is_empty()
        && t.bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'));
    let value = if ok { t.parse::<f64>().ok() } else { None };
    match value {
        Some(v) if v.is_finite() => Ok(DxfValue::Real {
            value: v,
            digits: significant_digits(t),
        }),
        _ => Err(malformed(line, format!("not a finite real: {t:?}"))),
    }
}

fn parse_hex(text: &str, line: usize, what: &str) -> Result<u64, DxfError> {
    let t = text.trim();
    if t.is_empty() || t.len() > 16 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(malformed(line, format!("not a {what}: {t:?}")));
    }
    u64::from_str_radix(t, 16).map_err(|_| malformed(line, format!("not a {what}: {t:?}")))
}

fn parse_binary(text: &str, line: usize) -> Result<DxfValue, DxfError> {
    let t = text.trim().as_bytes();
    if t.len() % 2 != 0 {
        return Err(malformed(line, "binary chunk has an odd number of digits"));
    }
    let mut out = Vec::with_capacity(t.len() / 2);
    for pair in t.chunks_exact(2) {
        let hi = (pair[0] as char).to_digit(16);
        let lo = (pair[1] as char).to_digit(16);
        match (hi, lo) {
            (Some(h), Some(l)) => out.push((h * 16 + l) as u8),
            _ => return Err(malformed(line, "binary chunk has a non-hexadecimal digit")),
        }
    }
    Ok(DxfValue::Binary(out))
}

fn parse_int<T: std::str::FromStr>(text: &str, line: usize, what: &str) -> Result<T, DxfError> {
    let t = text.trim();
    t.parse::<T>()
        .map_err(|_| malformed(line, format!("not a {what}: {t:?}")))
}

fn decode_text(raw: &[u8], utf8: bool, line: usize) -> Result<DxfValue, DxfError> {
    if raw.is_ascii() {
        // ASCII is valid UTF-8; the conversion cannot fail.
        return Ok(DxfValue::Text(String::from_utf8_lossy(raw).into_owned()));
    }
    if utf8 {
        return String::from_utf8(raw.to_vec())
            .map(DxfValue::Text)
            .map_err(|_| malformed(line, "text is not valid UTF-8"));
    }
    Ok(DxfValue::CodePageText(raw.to_vec()))
}

fn parse_value(
    code: u16,
    raw: &[u8],
    utf8: bool,
    line: usize,
    in_dimstyle: bool,
) -> Result<DxfValue, DxfError> {
    let ty = match value_type(code) {
        // DIMSTYLE's group 5 is DIMBLK, a block name (module docs).
        Some(ValueType::Handle) if code == 5 && in_dimstyle => ValueType::Text,
        Some(ty) => ty,
        None => {
            return Err(malformed(
                line,
                format!("group code {code} is not in the DXF Reference"),
            ));
        }
    };
    // Every non-text value is ASCII by construction; anything else is malformed.
    let text = match ty {
        ValueType::Text => return decode_text(raw, utf8, line),
        _ if !raw.is_ascii() => {
            return Err(malformed(line, format!("group {code} value is not ASCII")));
        }
        _ => String::from_utf8_lossy(raw),
    };
    let text = text.as_ref();
    match ty {
        ValueType::Text => decode_text(raw, utf8, line),
        ValueType::Handle => parse_hex(text, line, "handle").map(DxfValue::Handle),
        ValueType::Real => parse_real(text, line),
        ValueType::Int16 => parse_int::<i16>(text, line, "16-bit integer").map(DxfValue::Int16),
        ValueType::Int32 => parse_int::<i32>(text, line, "32-bit integer").map(DxfValue::Int32),
        ValueType::Int64 => parse_int::<i64>(text, line, "64-bit integer").map(DxfValue::Int64),
        ValueType::Bool => match text.trim() {
            "0" => Ok(DxfValue::Bool(false)),
            "1" => Ok(DxfValue::Bool(true)),
            other => Err(malformed(line, format!("boolean is not 0 or 1: {other:?}"))),
        },
        ValueType::Binary => parse_binary(text, line),
    }
}

/// A raw group pair: code, value bytes, line of the code.
struct RawPair<'a> {
    code: u16,
    value: &'a [u8],
    line: usize,
}

fn split_pairs<'a>(bytes: &'a [u8], limits: &DxfLimits) -> Result<Vec<RawPair<'a>>, DxfError> {
    if bytes.len() > limits.max_bytes {
        return Err(DxfError::LimitExceeded {
            what: "input bytes",
            limit: limits.max_bytes,
        });
    }
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err(DxfError::UnsupportedEncoding("binary DXF"));
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(DxfError::UnsupportedEncoding("UTF-8 byte-order mark"));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(DxfError::UnsupportedEncoding("UTF-16"));
    }
    if bytes.is_empty() {
        return Err(malformed(1, "empty file"));
    }
    let mut lines: Vec<&[u8]> = Vec::new();
    let mut crlf: Option<bool> = None;
    let mut rest = bytes;
    while !rest.is_empty() {
        let (line, next, ending) = match rest.iter().position(|&b| b == b'\n') {
            Some(i) => {
                let (l, r) = rest.split_at(i);
                let r = r.get(1..).unwrap_or(&[]);
                match l.strip_suffix(b"\r") {
                    Some(stripped) => (stripped, r, Some(true)),
                    None => (l, r, Some(false)),
                }
            }
            None => (rest, &[][..], None),
        };
        if let Some(e) = ending {
            match crlf {
                None => crlf = Some(e),
                Some(prev) if prev != e => {
                    return Err(malformed(
                        lines.len() + 1,
                        "CRLF and LF line endings are mixed",
                    ));
                }
                Some(_) => {}
            }
        }
        if line.len() > limits.max_line_len {
            return Err(DxfError::LimitExceeded {
                what: "line length",
                limit: limits.max_line_len,
            });
        }
        if lines.len() / 2 >= limits.max_groups {
            return Err(DxfError::LimitExceeded {
                what: "groups",
                limit: limits.max_groups,
            });
        }
        lines.push(line);
        rest = next;
    }
    if lines.len() % 2 != 0 {
        return Err(malformed(lines.len(), "group code with no value line"));
    }
    let mut pairs = Vec::with_capacity(lines.len() / 2);
    for (i, pair) in lines.chunks_exact(2).enumerate() {
        let line = 2 * i + 1;
        let code_text = std::str::from_utf8(pair[0]).unwrap_or("").trim();
        let code = code_text
            .parse::<u16>()
            .map_err(|_| malformed(line, format!("group code is not an integer: {code_text:?}")))?;
        pairs.push(RawPair {
            code,
            value: pair[1],
            line,
        });
    }
    Ok(pairs)
}

/// The value of HEADER variable `name` in the raw pairs, before typing.
/// `999` comments may sit anywhere, between a `9 $NAME` and its value
/// included, and are skipped as the HEADER parser skips them.
fn raw_header_text<'a>(pairs: &[RawPair<'a>], name: &str) -> Option<&'a [u8]> {
    let i = pairs
        .iter()
        .position(|p| p.code == 9 && p.value == name.as_bytes())?;
    pairs
        .get(i + 1..)?
        .iter()
        .find(|p| p.code != 999)
        .map(|p| p.value)
}

/// AC1012 (R13) and later write a handle on every table entry, block,
/// entity and object.
fn handles_required(acadver: &str) -> bool {
    acadver
        .strip_prefix("AC")
        .and_then(|n| n.parse::<u32>().ok())
        .is_some_and(|n| n >= 1012)
}

fn handled_section(name: &str) -> bool {
    matches!(name, "TABLES" | "BLOCKS" | "ENTITIES" | "OBJECTS")
}

#[derive(Default)]
struct Open {
    table: bool,
    block: bool,
}

impl DxfDocument {
    /// Parses an ASCII DXF file with [`DxfLimits::default`].
    pub fn parse(bytes: &[u8]) -> Result<Self, DxfError> {
        Self::parse_with_limits(bytes, &DxfLimits::default())
    }

    /// Parses an ASCII DXF file, refusing anything past `limits`.
    pub fn parse_with_limits(bytes: &[u8], limits: &DxfLimits) -> Result<Self, DxfError> {
        let pairs = split_pairs(bytes, limits)?;
        let acadver = raw_header_text(&pairs, "$ACADVER")
            .map(|v| String::from_utf8_lossy(v).trim().to_string())
            .unwrap_or_default();
        // AC1021 (R2007) and later DXF is UTF-8.
        let utf8 = acadver
            .strip_prefix("AC")
            .and_then(|n| n.parse::<u32>().ok())
            .is_some_and(|n| n >= 1021);
        let need_handles = handles_required(&acadver);

        let mut doc = DxfDocument {
            sections: Vec::new(),
            header: Vec::new(),
            records: Vec::new(),
            header_index: BTreeMap::new(),
            handle_index: BTreeMap::new(),
            real_digits: 0,
            real_decimals: 0,
        };
        let mut current: Option<usize> = None;
        let mut open = Open::default();
        let mut record: Option<DxfRecord> = None;
        let mut saw_eof = false;
        let mut iter = pairs.iter();

        while let Some(p) = iter.next() {
            if saw_eof {
                return Err(malformed(p.line, "data after EOF"));
            }
            let in_dimstyle = record.as_ref().is_some_and(|r| r.kind == "DIMSTYLE");
            let value = parse_value(p.code, p.value, utf8, p.line, in_dimstyle)?;
            if let DxfValue::Real { digits, .. } = value {
                doc.real_digits = doc.real_digits.max(digits);
                doc.real_decimals = doc.real_decimals.max(decimal_places(p.value));
            }
            if p.code == 0 {
                if let Some(r) = record.take() {
                    doc.finish_record(r, need_handles)?;
                }
                let kind = value.as_str().unwrap_or("").to_string();
                match kind.as_str() {
                    "SECTION" => {
                        if current.is_some() {
                            return Err(malformed(p.line, "SECTION inside an open section"));
                        }
                        // `999` comments may sit between SECTION and its name.
                        let name = match iter.by_ref().find(|n| n.code != 999) {
                            Some(n) if n.code == 2 => match decode_text(n.value, utf8, n.line)? {
                                DxfValue::Text(t) => t,
                                _ => return Err(malformed(n.line, "section name is not ASCII")),
                            },
                            _ => return Err(malformed(p.line, "SECTION not followed by 2 <name>")),
                        };
                        doc.sections.push(DxfSection {
                            name,
                            leading_groups: Vec::new(),
                        });
                        current = Some(doc.sections.len() - 1);
                    }
                    "ENDSEC" => {
                        if current.is_none() {
                            return Err(malformed(p.line, "ENDSEC with no open section"));
                        }
                        if open.table {
                            return Err(malformed(p.line, "TABLE not closed by ENDTAB"));
                        }
                        if open.block {
                            return Err(malformed(p.line, "BLOCK not closed by ENDBLK"));
                        }
                        current = None;
                    }
                    "EOF" => {
                        if current.is_some() {
                            return Err(malformed(p.line, "EOF inside an open section"));
                        }
                        saw_eof = true;
                    }
                    _ => {
                        let Some(sec) = current else {
                            return Err(malformed(p.line, format!("{kind} outside any section")));
                        };
                        let sec_name = doc.sections.get(sec).map(|s| s.name.as_str());
                        if sec_name == Some("HEADER") {
                            return Err(malformed(p.line, format!("{kind} record in HEADER")));
                        }
                        match kind.as_str() {
                            "TABLE" if open.table => {
                                return Err(malformed(p.line, "TABLE inside an open TABLE"));
                            }
                            "TABLE" => open.table = true,
                            "ENDTAB" if !open.table => {
                                return Err(malformed(p.line, "ENDTAB with no open TABLE"));
                            }
                            "ENDTAB" => open.table = false,
                            "BLOCK" if sec_name == Some("BLOCKS") && open.block => {
                                return Err(malformed(p.line, "BLOCK inside an open BLOCK"));
                            }
                            "BLOCK" if sec_name == Some("BLOCKS") => open.block = true,
                            "ENDBLK" if sec_name == Some("BLOCKS") && !open.block => {
                                return Err(malformed(p.line, "ENDBLK with no open BLOCK"));
                            }
                            "ENDBLK" if sec_name == Some("BLOCKS") => open.block = false,
                            _ => {}
                        }
                        if doc.records.len() >= limits.max_records {
                            return Err(DxfError::LimitExceeded {
                                what: "records",
                                limit: limits.max_records,
                            });
                        }
                        record = Some(DxfRecord {
                            kind,
                            section: sec,
                            handle: None,
                            owner: None,
                            groups: Vec::new(),
                            line: p.line,
                        });
                    }
                }
                continue;
            }

            let group = DxfGroup {
                code: p.code,
                value,
                line: p.line,
            };
            if let Some(r) = record.as_mut() {
                r.groups.push(group);
                continue;
            }
            let Some(sec) = current else {
                if p.code == 999 {
                    continue;
                }
                return Err(malformed(p.line, "group outside any section"));
            };
            let in_header = doc.sections.get(sec).is_some_and(|s| s.name == "HEADER");
            if !in_header {
                if let Some(s) = doc.sections.get_mut(sec) {
                    s.leading_groups.push(group);
                }
                continue;
            }
            if group.code == 9 {
                let name = group.value.as_str().unwrap_or("").to_string();
                if !name.starts_with('$') {
                    return Err(malformed(p.line, format!("header name {name:?} has no $")));
                }
                if doc.header_index.contains_key(&name) {
                    return Err(malformed(
                        p.line,
                        format!("header variable {name} repeated"),
                    ));
                }
                doc.header_index.insert(name.clone(), doc.header.len());
                doc.header.push(DxfHeaderVar {
                    name,
                    groups: Vec::new(),
                });
            } else if group.code == 999 {
                continue;
            } else {
                match doc.header.last_mut() {
                    Some(var) => var.groups.push(group),
                    None => return Err(malformed(p.line, "header value before any $NAME")),
                }
            }
        }
        if let Some(r) = record.take() {
            doc.finish_record(r, need_handles)?;
        }
        if !saw_eof {
            let last = pairs.last().map_or(1, |p| p.line);
            return Err(malformed(last, "no EOF"));
        }
        Ok(doc)
    }

    fn finish_record(&mut self, mut r: DxfRecord, need_handles: bool) -> Result<(), DxfError> {
        let mut depth = 0usize;
        let handle_code = if r.kind == "DIMSTYLE" { 105 } else { 5 };
        for g in &r.groups[..r.data_start()] {
            if g.code == 102 {
                match g.value.as_str() {
                    Some(s) if s.starts_with('{') => depth += 1,
                    Some(s) if s.ends_with('}') && depth > 0 => depth -= 1,
                    Some(s) if s.ends_with('}') => {
                        return Err(malformed(g.line, "102 } with no open bracket"));
                    }
                    _ => {}
                }
                continue;
            }
            if depth > 0 {
                continue;
            }
            if g.code == handle_code && r.handle.is_none() {
                r.handle = g.value.as_handle();
            } else if g.code == 330 && r.owner.is_none() {
                r.owner = g.value.as_handle();
            }
        }
        if depth != 0 {
            return Err(malformed(
                r.line,
                format!("{} leaves a 102 bracket open", r.kind),
            ));
        }
        let in_handled = self
            .sections
            .get(r.section)
            .is_some_and(|s| handled_section(&s.name));
        match r.handle {
            Some(0) => return Err(malformed(r.line, format!("{} has handle 0", r.kind))),
            Some(h) => {
                if self.handle_index.contains_key(&h) {
                    return Err(malformed(r.line, format!("handle {h:X} is used twice")));
                }
                self.handle_index.insert(h, self.records.len());
            }
            None if need_handles && in_handled && r.kind != "ENDTAB" => {
                return Err(malformed(r.line, format!("{} has no handle", r.kind)));
            }
            None => {}
        }
        self.records.push(r);
        Ok(())
    }

    /// The record whose handle is `handle`.
    pub fn record_by_handle(&self, handle: u64) -> Option<&DxfRecord> {
        self.handle_index
            .get(&handle)
            .and_then(|&i| self.records.get(i))
    }

    /// Every handle the file defines, in ascending order.
    pub fn handles(&self) -> impl Iterator<Item = u64> + '_ {
        self.handle_index.keys().copied()
    }

    /// The value groups of HEADER variable `name` (`$` included).
    pub fn header_var(&self, name: &str) -> Option<&[DxfGroup]> {
        self.header_index
            .get(name)
            .and_then(|&i| self.header.get(i))
            .map(|v| v.groups.as_slice())
    }

    /// `$ACADVER`, the DXF's release code (`AC1027` for R2013).
    pub fn acad_version(&self) -> Option<&str> {
        self.header_var("$ACADVER")?.first()?.value.as_str()
    }

    /// The name of the section holding `record`.
    pub fn section_name(&self, record: &DxfRecord) -> &str {
        self.sections
            .get(record.section)
            .map_or("", |s| s.name.as_str())
    }

    /// The most significant digits any real in the file was printed with:
    /// the precision AutoCAD wrote the file at (16 for its 2000+ exports).
    pub fn real_precision(&self) -> u8 {
        self.real_digits
    }

    /// The most digits after the decimal point any real printed without an
    /// exponent carries: AutoCAD's exports round to 16 significant digits
    /// but to no more than 16 decimal places (`0.0001000435253656` has 13
    /// significant digits, and `1.2e-20` prints as `0.0`).
    pub fn real_decimals(&self) -> u8 {
        self.real_decimals
    }
}
