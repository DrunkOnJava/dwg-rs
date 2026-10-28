//! Field scorers (#102): each decoded field compared with the DXF group that
//! carries the same value.
//!
//! # Where the mappings come from
//!
//! The DXF group of every field of a type ODA spec v5.4.1 prescribes is the
//! one the spec prints in its DXF column for that field (§20.4.x, cited on
//! each scorer). The types the spec does not prescribe use the group codes of
//! Autodesk's public *DXF Reference*. Where AutoCAD omits a group whose value
//! is the default, the default is the *DXF Reference*'s (thickness 0,
//! extrusion (0,0,1), ...) or, for DIMSTYLE, the imperial value AutoCAD omits
//! (measured: the ISO-25 exports write exactly the groups that differ from
//! it). A match against a default is counted on its own, never as `equal`.
//!
//! # Statuses
//!
//! - `equal`: both files hold the value and they agree.
//! - `default`: the DXF omits the group and the decoded value is its default.
//! - `different`: they disagree (an omitted group counts as its default).
//! - `dwg-only`: the decoder surfaces a field the DXF has no group for, or the
//!   DXF lacks a group the field maps to and the group has no default.
//! - `not-decoded`: a group the DXF holds that no decoded field maps to (owner
//!   and other handles, colour and layer on entities, ...).
//!
//! # What AutoCAD's export writes differently, and how it is scored
//!
//! Each rule below is measured on the corpus and applied where the export
//! departs from the DWG's own representation; none hides a disagreement
//! that has another explanation:
//!
//! - Angles: degrees in the DXF, radians in the DWG.
//! - Strings: the export writes control characters as `^` plus the
//!   character 64 above (`^J` inside an MTEXT), decoded before comparing.
//! - Bit 64 of a table entry's group 70: never written by the export, so
//!   the entry header's 64-flag is `dwg-only`.
//! - LAYER `values` bit 2: the spec glosses it "on"; every corpus layer is
//!   on with the bit clear, so it is scored as "off" (negative 62).
//! - A true-colour LAYER: 62 is the export's nearest index to the 420 RGB,
//!   which the DWG does not store; both are listed as not decoded (#245).
//! - LWPOLYLINE: the DWG flag word is presence bits; its 0x200 is the closed
//!   bit of group 70.
//! - Anonymous blocks: the DWG stores the family (`*U`), AutoCAD numbers it
//!   on export (`*U17`); scored under its own field name. BLOCK_RECORD's
//!   name is the one resolved through its BLOCK entity (#70).
//! - Dictionaries: entries the export adds for its own save-time objects are
//!   set aside and listed; keys are matched by name, order checked apart.
//! - A `101` group starts an embedded object (R2018 MTEXT and multi-line
//!   attributes) whose codes repeat the record's; lookups stop there.
//! - R12 DXF has no subclass markers: the whole record is the scope.
//! - MULTILEADER nests its parts in marker groups (`300 CONTEXT_DATA{`,
//!   `302 LEADER{`, `304 LEADER_LINE{`, closed by `301`, `303`, `305`) whose
//!   fields reuse each other's codes (90, 91, 92, 290, 291, 40, 10, 11,
//!   170, 171, 271 to 273); each part is scored within its own groups, in
//!   the context before and after its leader roots, in each root before and
//!   after its lines, and in the entity's tail after `301`.
//! - A colour before R2004 is a bare index in the DWG, which the export
//!   writes as the matching 32-bit word; a true colour the older file
//!   cannot hold stays `different` (MULTILEADER's text background on R2000,
//!   #381).
//!
//! # Tolerance for reals
//!
//! AutoCAD prints reals at 16 significant digits (its R12 exports at 17)
//! and at no more than 16 decimal places: `-0.0001000435253656` is
//! printed with 13 significant digits and a stored `1.2e-20` as `0.0`.
//! Every export of the corpus measures exactly that
//! ([`DxfDocument::real_precision`], [`DxfDocument::real_decimals`]). The
//! tolerance takes 16 and 16 as its floor and a file's measured digits only
//! when they are finer, since a file whose reals happen to be short (`1.0`,
//! `2.0`, `0.0`) says nothing about how it would print a long one. Printing rounds the stored double to the
//! coarser of the two, and parsing the text back lands within half a unit
//! in the last place of the binary value, so a decoded value `a` agrees with
//! a printed one `b` when
//!
//! ```text
//! |a - b| <= 0.5 * max(10^(e - P + 1), 10^(-D)) + 4 * ulp(max(|a|, |b|))
//! ```
//!
//! where `e` is `b`'s decimal exponent, `P` the significant digits and
//! `D` the decimal places (16 each, or the file's when finer). The first term is the printing; the 4 ulps
//! cover parsing and, for angles, the radian-to-degree conversion both
//! programs perform (DXF writes angles in degrees, DWG stores radians).
//! Nothing looser: an oracle whose job is exactness cannot use the 1e-9 the
//! R2013 value tests use. Integers, text, booleans and flags are compared
//! exactly.

use crate::corpus::DwgRecord;
use dwg::dxf_reader::{DxfDocument, DxfGroup, DxfRecord, DxfValue};
use dwg::entities::{DecodedEntity, Point2D, Point3D};
use dwg::objects::color::ObjectColor;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Equal,
    Default,
    Different,
    DwgOnly,
    NotDecoded,
}

/// A text group held as bytes of the file's code page
/// ([`DxfValue::CodePageText`]): present, but not comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodePage;

/// One field of one record, scored.
#[derive(Debug, Clone)]
pub struct FieldScore {
    pub field: String,
    pub code: String,
    pub status: Status,
    pub dwg: String,
    pub dxf: String,
}

/// The scoring state for one paired record.
pub struct Ctx<'a> {
    doc: &'a DxfDocument,
    rec: &'a DxfRecord,
    /// Groups inside `102` brackets (reactors, extension dictionary).
    bracketed: Vec<bool>,
    used: Vec<bool>,
    /// Group-index range of the subclass being scored.
    scope: (usize, usize),
    prefix: String,
    account: bool,
    pub out: Vec<FieldScore>,
}

/// Index of an XRECORD's first application-data group, the one after
/// `100 AcDbXrecord` (`groups.len()` on other records). The DXF Reference
/// gives codes 1 to 369 there to the application, so a `102` among them is
/// data, not a bracket, as the reader treats it.
fn data_start(rec: &DxfRecord) -> usize {
    let len = rec.groups.len();
    if rec.kind != "XRECORD" {
        return len;
    }
    rec.groups
        .iter()
        .position(|g| g.code == 100 && g.value.as_str() == Some("AcDbXrecord"))
        .map_or(len, |i| i + 1)
}

fn bracket_mask(rec: &DxfRecord) -> Vec<bool> {
    let data = data_start(rec);
    let mut depth = 0usize;
    rec.groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            if i >= data {
                return false;
            }
            if g.code == 102 {
                match g.value.as_str() {
                    Some(s) if s.starts_with('{') => depth += 1,
                    Some(s) if s.ends_with('}') => depth = depth.saturating_sub(1),
                    _ => {}
                }
                return true;
            }
            depth > 0
        })
        .collect()
}

/// AutoCAD's printing of reals, the floor of every file's tolerance.
const EXPORT_DIGITS: u8 = 16;
const EXPORT_DECIMALS: u8 = 16;

/// How finely a file prints reals: significant digits and decimal places.
#[derive(Debug, Clone, Copy)]
pub struct Printing {
    pub digits: u8,
    pub decimals: u8,
}

/// A DXF string with its control characters restored: the export writes
/// a control character as `^` and the character 64 above it (`^J` for the
/// line feed inside an MTEXT), and a literal `^` as `^ ` (DXF Reference).
pub fn caret_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch != '^' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some(' ') => out.push('^'),
            Some(n @ '@'..='_') => out.push(char::from(n as u8 - 0x40)),
            Some(n) => {
                out.push('^');
                out.push(n);
            }
            None => out.push('^'),
        }
    }
    out
}

/// `true` when decoded real `a` agrees with printed real `b` (module docs).
pub fn reals_agree(a: f64, b: f64, p: Printing) -> bool {
    if a == b {
        return true;
    }
    if !a.is_finite() {
        return false;
    }
    let by_digits = if b == 0.0 {
        0.0
    } else {
        10f64.powf(b.abs().log10().floor() - f64::from(p.digits) + 1.0)
    };
    let by_decimals = if p.decimals == 0 {
        0.0
    } else {
        10f64.powi(-i32::from(p.decimals))
    };
    let printing = 0.5 * by_digits.max(by_decimals);
    let ulps = 4.0 * f64::EPSILON * a.abs().max(b.abs());
    (a - b).abs() <= printing + ulps
}

fn fmt_real(v: f64) -> String {
    format!("{v:?}")
}

impl<'a> Ctx<'a> {
    pub fn new(doc: &'a DxfDocument, rec: &'a DxfRecord) -> Self {
        let n = rec.groups.len();
        Self {
            doc,
            rec,
            bracketed: bracket_mask(rec),
            used: vec![false; n],
            scope: (0, n),
            prefix: String::new(),
            account: true,
            out: Vec::new(),
        }
    }

    pub fn doc(&self) -> &'a DxfDocument {
        self.doc
    }

    pub fn record(&self) -> &'a DxfRecord {
        self.rec
    }

    /// Restricts lookups to the groups after `100 <marker>` up to the next
    /// `100`; returns `false` (and scopes to nothing) when the marker is absent.
    pub fn subclass(&mut self, marker: &str) -> bool {
        let groups = &self.rec.groups;
        // R12 DXF has no subclass markers: the whole record is the scope.
        if !groups.iter().any(|g| g.code == 100) {
            self.scope = (0, groups.len());
            return true;
        }
        let start = groups
            .iter()
            .position(|g| g.code == 100 && g.value.as_str() == Some(marker));
        match start {
            Some(s) => {
                // A 101 starts an embedded object (R2018 MTEXT, ATTRIB),
                // whose groups reuse the codes of the record's own.
                let end = groups
                    .iter()
                    .enumerate()
                    .skip(s + 1)
                    .find(|(_, g)| matches!(g.code, 100 | 101))
                    .map_or(groups.len(), |(i, _)| i);
                self.scope = (s + 1, end);
                true
            }
            None => {
                self.scope = (0, 0);
                false
            }
        }
    }

    /// Lookups over the whole record again, up to an embedded object (101).
    pub fn whole(&mut self) {
        let end = self.rec.groups.iter().position(|g| g.code == 101);
        self.scope = (0, end.unwrap_or(self.rec.groups.len()));
    }

    /// Takes the next unused top-level group with `code` in scope.
    pub fn take(&mut self, code: u16) -> Option<&'a DxfValue> {
        let (s, e) = self.scope;
        let groups = &self.rec.groups;
        let i = (s..e.min(groups.len()))
            .find(|&i| groups[i].code == code && !self.used[i] && !self.bracketed[i])?;
        self.used[i] = true;
        Some(&groups[i].value)
    }

    /// The unused top-level groups in scope, with their indices.
    pub fn free(&self) -> Vec<(usize, u16, &'a DxfValue)> {
        let (s, e) = self.scope;
        let groups = &self.rec.groups;
        (s..e.min(groups.len()))
            .filter(|&i| !self.used[i] && !self.bracketed[i])
            .map(|i| (i, groups[i].code, &groups[i].value))
            .collect()
    }

    /// Marks group `i` as taken.
    pub fn take_at(&mut self, i: usize) {
        if let Some(u) = self.used.get_mut(i) {
            *u = true;
        }
    }

    /// Looks at the next unused group with `code` without taking it.
    pub fn peek(&self, code: u16) -> Option<&'a DxfValue> {
        let (s, e) = self.scope;
        let groups = &self.rec.groups;
        (s..e.min(groups.len()))
            .find(|&i| groups[i].code == code && !self.used[i] && !self.bracketed[i])
            .map(|i| &groups[i].value)
    }

    fn push(&mut self, field: &str, code: String, status: Status, dwg: String, dxf: String) {
        self.out.push(FieldScore {
            field: format!("{}{field}", self.prefix),
            code: format!("{}{code}", self.prefix),
            status,
            dwg,
            dxf,
        });
    }

    fn precision(&self) -> Printing {
        Printing {
            digits: self.doc.real_precision().max(EXPORT_DIGITS),
            decimals: self.doc.real_decimals().max(EXPORT_DECIMALS),
        }
    }

    /// A real carried in group `code`; `default` is the value an omitted
    /// group stands for, if the DXF Reference gives one.
    pub fn real(&mut self, field: &str, code: u16, dwg: f64, default: Option<f64>) {
        let p = self.precision();
        match (self.take(code).and_then(DxfValue::as_f64), default) {
            (Some(x), _) => {
                let s = if reals_agree(dwg, x, p) {
                    Status::Equal
                } else {
                    Status::Different
                };
                self.push(field, code.to_string(), s, fmt_real(dwg), fmt_real(x));
            }
            (None, Some(d)) => {
                let s = if reals_agree(dwg, d, p) {
                    Status::Default
                } else {
                    Status::Different
                };
                self.push(
                    field,
                    code.to_string(),
                    s,
                    fmt_real(dwg),
                    format!("(omitted) {d:?}"),
                );
            }
            (None, None) => self.push(
                field,
                code.to_string(),
                Status::DwgOnly,
                fmt_real(dwg),
                String::new(),
            ),
        }
    }

    /// An angle: radians in the DWG, degrees in the DXF.
    pub fn angle(&mut self, field: &str, code: u16, dwg_rad: f64, default_deg: Option<f64>) {
        let deg = dwg_rad.to_degrees();
        let p = self.precision();
        match (self.take(code).and_then(DxfValue::as_f64), default_deg) {
            (Some(x), _) => {
                let s = if reals_agree(deg, x, p) {
                    Status::Equal
                } else {
                    Status::Different
                };
                self.push(
                    field,
                    code.to_string(),
                    s,
                    format!("{deg:?} deg"),
                    fmt_real(x),
                );
            }
            (None, Some(d)) => {
                let s = if reals_agree(deg, d, p) {
                    Status::Default
                } else {
                    Status::Different
                };
                self.push(
                    field,
                    code.to_string(),
                    s,
                    format!("{deg:?} deg"),
                    format!("(omitted) {d:?}"),
                );
            }
            (None, None) => self.push(
                field,
                code.to_string(),
                Status::DwgOnly,
                format!("{deg:?} deg"),
                String::new(),
            ),
        }
    }

    /// A 3D point in groups `code`, `code + 10`, `code + 20`.
    pub fn point3(&mut self, field: &str, code: u16, p: Point3D, default: Option<Point3D>) {
        let d = default;
        self.real(&format!("{field}.x"), code, p.x, d.map(|d| d.x));
        self.real(&format!("{field}.y"), code + 10, p.y, d.map(|d| d.y));
        self.real(&format!("{field}.z"), code + 20, p.z, d.map(|d| d.z));
    }

    /// A 2D point in groups `code`, `code + 10`.
    pub fn point2(&mut self, field: &str, code: u16, p: Point2D) {
        self.real(&format!("{field}.x"), code, p.x, None);
        self.real(&format!("{field}.y"), code + 10, p.y, None);
    }

    /// An integer in group `code` (any integer or boolean group type).
    pub fn int(&mut self, field: &str, code: u16, dwg: i64, default: Option<i64>) {
        match (self.take(code).and_then(DxfValue::as_i64), default) {
            (Some(x), _) => {
                let s = if dwg == x {
                    Status::Equal
                } else {
                    Status::Different
                };
                self.push(field, code.to_string(), s, dwg.to_string(), x.to_string());
            }
            (None, Some(d)) => {
                let s = if dwg == d {
                    Status::Default
                } else {
                    Status::Different
                };
                self.push(
                    field,
                    code.to_string(),
                    s,
                    dwg.to_string(),
                    format!("(omitted) {d}"),
                );
            }
            (None, None) => self.push(
                field,
                code.to_string(),
                Status::DwgOnly,
                dwg.to_string(),
                String::new(),
            ),
        }
    }

    /// Bit `mask` of integer group `code`, compared with a decoded boolean.
    /// Does not take the group: call [`Ctx::done`] on it after its bits.
    pub fn bit(&mut self, field: &str, code: u16, mask: i64, dwg: bool) {
        let label = format!("{code}&{mask}");
        match self.peek(code).and_then(DxfValue::as_i64) {
            Some(x) => {
                let v = x & mask != 0;
                let s = if v == dwg {
                    Status::Equal
                } else {
                    Status::Different
                };
                self.push(field, label, s, dwg.to_string(), v.to_string());
            }
            None => self.push(
                field,
                label,
                Status::DwgOnly,
                dwg.to_string(),
                String::new(),
            ),
        }
    }

    /// The next group `code` as text, as [`take`](Self::take) would give it,
    /// except that a pre-AC1021 group whose bytes need the file's code page
    /// ([`DxfValue::CodePageText`]) is left untaken, so it stays among the
    /// record's not-decoded groups, and comes back as `Err`: it holds a value
    /// this oracle cannot compare, which is not a group the export omitted.
    pub fn take_text(&mut self, code: u16) -> Result<Option<&'a str>, CodePage> {
        let (s, e) = self.scope;
        let groups = &self.rec.groups;
        let Some(i) = (s..e.min(groups.len()))
            .find(|&i| groups[i].code == code && !self.used[i] && !self.bracketed[i])
        else {
            return Ok(None);
        };
        if matches!(groups[i].value, DxfValue::CodePageText(_)) {
            return Err(CodePage);
        }
        self.used[i] = true;
        Ok(groups[i].value.as_str())
    }

    /// Marks the next group `code` as accounted for (its bits were scored).
    pub fn done(&mut self, code: u16) {
        self.take(code);
    }

    /// Text in group `code`; `default` stands for an omitted group.
    pub fn text(&mut self, field: &str, code: u16, dwg: &str, default: Option<&str>) {
        let Ok(got) = self.take_text(code) else {
            self.push(
                field,
                code.to_string(),
                Status::DwgOnly,
                format!("{dwg:?}"),
                "(code-page text, not compared)".to_string(),
            );
            return;
        };
        match (got, default) {
            (Some(x), _) => {
                let s = if dwg == caret_decode(x) {
                    Status::Equal
                } else {
                    Status::Different
                };
                self.push(
                    field,
                    code.to_string(),
                    s,
                    format!("{dwg:?}"),
                    format!("{x:?}"),
                );
            }
            (None, Some(d)) => {
                let s = if dwg == d {
                    Status::Default
                } else {
                    Status::Different
                };
                self.push(
                    field,
                    code.to_string(),
                    s,
                    format!("{dwg:?}"),
                    format!("(omitted) {d:?}"),
                );
            }
            (None, None) => self.push(
                field,
                code.to_string(),
                Status::DwgOnly,
                format!("{dwg:?}"),
                String::new(),
            ),
        }
    }

    /// A decoded value the DXF has no group for.
    pub fn dwg_only(&mut self, field: &str, dwg: impl std::fmt::Debug) {
        self.push(
            field,
            "-".into(),
            Status::DwgOnly,
            format!("{dwg:?}"),
            String::new(),
        );
    }

    /// DXF groups no decoded field maps to, labelled by the caller.
    pub fn not_decoded(&mut self, code: &str, dxf: String) {
        self.push("", code.to_string(), Status::NotDecoded, String::new(), dxf);
    }

    /// A comparison computed by the caller (counts, derived values).
    pub fn derived(&mut self, field: &str, code: &str, agree: bool, dwg: String, dxf: String) {
        let s = if agree {
            Status::Equal
        } else {
            Status::Different
        };
        self.push(field, code.to_string(), s, dwg, dxf);
    }

    /// Scores fields held on another record (a BLOCK_HEADER's BLOCK entity),
    /// prefixing their names with `prefix`; that record's other groups are
    /// not accounted for here.
    pub fn on_other(&mut self, prefix: &str, other: &'a DxfRecord, f: impl FnOnce(&mut Ctx<'a>)) {
        let mut sub = Ctx::new(self.doc, other);
        sub.prefix = format!("{prefix}.");
        sub.account = false;
        f(&mut sub);
        self.out.append(&mut sub.out);
    }

    /// Every group in the record no field took, as `not-decoded`: by code,
    /// with the bracket contents counted once per bracket and extended data
    /// once per application.
    pub fn finish(mut self) -> Vec<FieldScore> {
        if !self.account {
            return self.out;
        }
        let groups = &self.rec.groups;
        let data = data_start(self.rec);
        let mut i = 0;
        while i < groups.len() {
            let g = &groups[i];
            if self.used[i]
                || (matches!(g.code, 5 | 100) && i < data)
                || (g.code == 105 && self.rec.kind == "DIMSTYLE")
            {
                i += 1;
                continue;
            }
            if g.code == 102 && i < data {
                if let Some(name) = g.value.as_str().filter(|s| s.starts_with('{')) {
                    let mut j = i + 1;
                    while j < groups.len() && !(groups[j].code == 102 && !self.bracketed_open(j)) {
                        j += 1;
                    }
                    self.out.push(FieldScore {
                        field: String::new(),
                        code: format!("102{name}}}"),
                        status: Status::NotDecoded,
                        dwg: String::new(),
                        dxf: format!("{} groups", j.saturating_sub(i + 1)),
                    });
                    i = j + 1;
                    continue;
                }
                i += 1;
                continue;
            }
            if g.code == 1001 {
                let app = g.value.as_str().unwrap_or("").to_string();
                let mut j = i + 1;
                while j < groups.len() && groups[j].code > 1001 {
                    j += 1;
                }
                self.out.push(FieldScore {
                    field: String::new(),
                    code: format!("1001 {app}"),
                    status: Status::NotDecoded,
                    dwg: String::new(),
                    dxf: format!("{} groups", j - i - 1),
                });
                i = j;
                continue;
            }
            if self.bracketed[i] {
                i += 1;
                continue;
            }
            self.out.push(FieldScore {
                field: String::new(),
                code: g.code.to_string(),
                status: Status::NotDecoded,
                dwg: String::new(),
                dxf: g.value.to_string(),
            });
            i += 1;
        }
        self.out
    }

    /// `true` when group `j` (a `102`) opens a bracket rather than closing one.
    fn bracketed_open(&self, j: usize) -> bool {
        self.rec.groups[j]
            .value
            .as_str()
            .is_some_and(|s| s.starts_with('{'))
    }
}

/// The DXF 62 colour number a decoded `CMC`/`ENC` colour exports as: 256
/// ByLayer, 0 ByBlock, the index for an ACI colour.
pub fn aci_of(c: &ObjectColor) -> i64 {
    match c.rgb >> 24 {
        0xC0 => 256,
        0xC1 => 0,
        0xC3 => i64::from(c.rgb & 0xFF),
        _ => i64::from(c.index),
    }
}

/// DXF 370 lineweight for a DWG lineweight index (0-23 the DXF Reference's
/// lineweights in hundredths of a millimetre, 29 ByLayer, 30 ByBlock, 31
/// Default).
pub fn lineweight_of(index: i64) -> Option<i64> {
    const TABLE: [i64; 24] = [
        0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
        200, 211,
    ];
    match index {
        0..=23 => TABLE.get(index as usize).copied(),
        29 => Some(-1),
        30 => Some(-2),
        31 => Some(-3),
        _ => None,
    }
}

const Z: Point3D = Point3D {
    x: 0.0,
    y: 0.0,
    z: 1.0,
};

/// The ODA spec section a DWG type is prescribed by, `-` for the classes the
/// spec does not prescribe.
pub fn section_of(label: &str) -> &'static str {
    match label {
        "SORTENTSTABLE" => "§20.4.93",
        "BLOCK" => "§20.4.6",
        "ENDBLK" => "§20.4.7",
        "ARC" => "§20.4.18",
        "CIRCLE" => "§20.4.20",
        "TOLERANCE" => "§20.4.49",
        "TEXT" => "§20.4.3",
        "ATTRIB" => "§20.4.4",
        "ATTDEF" => "§20.4.5",
        "SEQEND" => "§20.4.8",
        "INSERT" => "§20.4.9",
        "DIMENSION_ORDINATE" => "§20.4.23",
        "DIMENSION_LINEAR" => "§20.4.24",
        "DIMENSION_ALIGNED" => "§20.4.25",
        "DIMENSION_ANG_3PT" => "§20.4.26",
        "DIMENSION_ANG_2LN" => "§20.4.27",
        "DIMENSION_RADIUS" => "§20.4.28",
        "DIMENSION_DIAMETER" => "§20.4.29",
        "VERTEX_2D" => "§20.4.11",
        "VERTEX_3D" => "§20.4.12",
        "VERTEX_MESH" => "§20.4.13",
        "VERTEX_PFACE" => "§20.4.14",
        "VERTEX_PFACE_FACE" => "§20.4.15",
        "POLYLINE_2D" => "§20.4.16",
        "POLYLINE_3D" => "§20.4.17",
        "3DFACE" => "§20.4.32",
        "SPLINE" => "§20.4.40",
        "POLYLINE_PFACE" => "§20.4.33",
        "SOLID" => "§20.4.35",
        "RAY" => "§20.4.42",
        "XLINE" => "§20.4.43",
        "MTEXT" => "§20.4.46",
        "MULTILEADER" => "§20.4.48",
        "LINE" => "§20.4.21",
        "POINT" => "§20.4.31",
        "VIEWPORT" => "§20.4.38",
        "ELLIPSE" => "§20.4.39",
        "HATCH" => "§20.4.75",
        "DICTIONARY" => "§20.4.44",
        "ACDBDICTIONARYWDFLT" => "§20.4.45",
        "BLOCK_CONTROL" => "§20.4.51",
        "BLOCK_HEADER" => "§20.4.52",
        "LAYER_CONTROL" => "§20.4.53",
        "LAYER" => "§20.4.54",
        "STYLE_CONTROL" => "§20.4.55",
        "STYLE" => "§20.4.56",
        "LTYPE_CONTROL" => "§20.4.57",
        "LTYPE" => "§20.4.58",
        "VIEW_CONTROL" => "§20.4.59",
        "VIEW" => "§20.4.60",
        "UCS_CONTROL" => "§20.4.61",
        "VPORT_CONTROL" => "§20.4.63",
        "VPORT" => "§20.4.64",
        "APPID_CONTROL" => "§20.4.65",
        "APPID" => "§20.4.66",
        "DIMSTYLE_CONTROL" => "§20.4.67",
        "DIMSTYLE" => "§20.4.68",
        "MLINESTYLE" => "§20.4.73",
        "MLINE" => "§20.4.50",
        "MLEADERSTYLE" => "§20.4.87",
        "DICTIONARYVAR" => "§20.4.74",
        "LAYOUT" => "§20.4.84",
        "LWPOLYLINE" => "§20.4.85",
        "SCALE" => "§20.4.92",
        "TABLESTYLE" => "§20.4.101",
        "IMAGEDEF_REACTOR" => "§20.4.82",
        "RASTERVARIABLES" => "§20.4.91",
        "IMAGE" => "§20.4.80",
        "IMAGEDEF" => "§20.4.81",
        "XRECORD" => "§20.4.104",
        _ => "-",
    }
}

/// Every DWG record of the drawing, by handle.
pub type DwgIndex<'a> = BTreeMap<u64, &'a DwgRecord>;

/// Scores one paired record; `Err` with the reason when no scorer covers
/// its decode (the record is then counted as unscored).
pub fn score(
    rec: &DwgRecord,
    dwg: &DwgIndex<'_>,
    doc: &DxfDocument,
    dxf: &DxfRecord,
) -> Result<Vec<FieldScore>, String> {
    let mut c = Ctx::new(doc, dxf);
    use DecodedEntity as D;
    match &rec.decoded {
        // §20.4.21: 10/20/30, 11/21/31, 39, 210.
        D::Line(l) => {
            c.subclass("AcDbLine");
            c.point3("start", 10, l.start, None);
            c.point3("end", 11, l.end, None);
            c.real("thickness", 39, l.thickness, Some(0.0));
            c.point3("extrusion", 210, l.extrusion, Some(Z));
            c.dwg_only("is_2d", l.is_2d);
        }
        // §20.4.31: 10, 39, 210, 50.
        D::Point(p) => {
            c.subclass("AcDbPoint");
            c.point3("position", 10, p.position, None);
            c.real("thickness", 39, p.thickness, Some(0.0));
            c.point3("extrusion", 210, p.extrusion, Some(Z));
            c.angle("x_axis_angle", 50, p.x_axis_angle, Some(0.0));
        }
        // §20.4.20: 10, 40, 39, 210.
        D::Circle(k) => {
            c.subclass("AcDbCircle");
            c.point3("center", 10, k.center, None);
            c.real("radius", 40, k.radius, None);
            c.real("thickness", 39, k.thickness, Some(0.0));
            c.point3("extrusion", 210, k.extrusion, Some(Z));
        }
        // §20.4.18: the CIRCLE fields, then 50/51 under AcDbArc.
        D::Arc(a) => {
            c.subclass("AcDbCircle");
            c.point3("center", 10, a.center, None);
            c.real("radius", 40, a.radius, None);
            c.real("thickness", 39, a.thickness, Some(0.0));
            c.point3("extrusion", 210, a.extrusion, Some(Z));
            c.subclass("AcDbArc");
            c.angle("start_angle", 50, a.start_angle, None);
            c.angle("end_angle", 51, a.end_angle, None);
        }
        // §20.4.39: 10, 11, 210, 40, 41, 42 (41/42 in radians in both files).
        D::Ellipse(e) => {
            c.subclass("AcDbEllipse");
            c.point3("center", 10, e.center, None);
            c.point3("major_axis", 11, e.major_axis, None);
            c.point3("extrusion", 210, e.extrusion, Some(Z));
            c.real("axis_ratio", 40, e.axis_ratio, None);
            c.real("start_param", 41, e.start_param, None);
            c.real("end_param", 42, e.end_param, None);
        }
        D::LwPolyline(p) => score_lwpolyline(&mut c, p),
        D::Spline(sp) => score_spline(&mut c, sp),
        // §20.4.6: the name, 2 and 3 in the DXF.
        D::Block(b) => {
            c.subclass("AcDbBlockBegin");
            block_name(&mut c, "name", &b.name);
        }
        D::EndBlk(_) | D::SeqEnd(_) => {}
        D::Mesh(m) => score_mesh(&mut c, m),
        // §20.4.3: TEXT writes AcDbText twice; 73 is in the second.
        D::Text(t) => {
            c.whole();
            score_text(&mut c, t, Some(73));
        }
        // §20.4.4 / §20.4.5: the TEXT fields under AcDbText, then the
        // attribute's own under AcDbAttribute / AcDbAttributeDefinition,
        // where the vertical alignment is 74 (73 is the field length).
        D::Attrib(a) => {
            c.subclass("AcDbText");
            score_text(&mut c, &a.text, None);
            c.subclass("AcDbAttribute");
            score_attribute(
                &mut c,
                &a.text,
                &a.tag,
                a.flags,
                a.field_length,
                a.lock_position,
                a.attribute_type,
            );
        }
        D::AttDef(a) => {
            c.subclass("AcDbText");
            score_text(&mut c, &a.text, None);
            c.subclass("AcDbAttributeDefinition");
            c.text("prompt", 3, &a.prompt, None);
            score_attribute(
                &mut c,
                &a.text,
                &a.tag,
                a.flags,
                a.field_length,
                a.lock_position,
                a.attribute_type,
            );
        }
        // §20.4.9: 10, 41/42/43, 50, 210, 66.
        D::Insert(i) => {
            c.subclass("AcDbBlockReference");
            c.point3("insertion_point", 10, i.insertion_point, None);
            c.real("scale.x", 41, i.scale.x, Some(1.0));
            c.real("scale.y", 42, i.scale.y, Some(1.0));
            c.real("scale.z", 43, i.scale.z, Some(1.0));
            c.angle("rotation", 50, i.rotation, Some(0.0));
            c.point3("extrusion", 210, i.extrusion, Some(Z));
            c.int("has_attribs", 66, i64::from(i.has_attribs), Some(0));
            c.dwg_only("owned_object_count", i.owned_object_count);
        }
        // §20.4.35: 39, the elevation as the corners' Z, 10-13, 210.
        D::Solid(k) => {
            c.subclass("AcDbTrace");
            for (n, p) in k.corners.iter().enumerate() {
                let code = 10 + n as u16;
                c.real(&format!("corners[{n}].x"), code, p.x, None);
                c.real(&format!("corners[{n}].y"), code + 10, p.y, None);
                c.real(
                    &format!("elevation (corners[{n}].z)"),
                    code + 20,
                    k.elevation,
                    None,
                );
            }
            c.real("thickness", 39, k.thickness, Some(0.0));
            c.point3("extrusion", 210, k.extrusion, Some(Z));
        }
        // §20.4.42 / §20.4.43: 10, 11.
        D::Ray(r) => {
            c.subclass("AcDbRay");
            c.point3("start", 10, r.start, None);
            c.point3("direction", 11, r.direction, None);
        }
        D::XLine(x) => {
            c.subclass("AcDbXline");
            c.point3("point", 10, x.point, None);
            c.point3("direction", 11, x.direction, None);
        }
        // §20.4.32: 10-13, 70.
        D::ThreeDFace(f) => {
            c.subclass("AcDbFace");
            for (n, p) in f.corners.iter().enumerate() {
                c.point3(&format!("corners[{n}]"), 10 + n as u16, *p, None);
            }
            c.int("invisible_edges", 70, i64::from(f.invisible_edges), Some(0));
            c.dwg_only("is_triangle", f.is_triangle);
        }
        // §20.4.11: 70, 10, 40, 41, 42, 91, 50.
        D::Vertex(v) => {
            c.whole();
            c.int("flag", 70, i64::from(v.flag), Some(0));
            c.point3("location", 10, v.location, None);
            c.real("start_width", 40, v.start_width, Some(0.0));
            c.real("end_width", 41, v.end_width, Some(0.0));
            c.real("bulge", 42, v.bulge, Some(0.0));
            if let Some(id) = v.vertex_id {
                c.int("vertex_id", 91, i64::from(id), Some(0));
            }
            if let Some(t) = v.tangent_direction {
                c.angle("tangent_direction", 50, t, None);
            }
        }
        // §20.4.12 to §20.4.14: 70, 10.
        D::VertexPoint(v) => {
            c.whole();
            c.int("flag", 70, i64::from(v.flag), Some(0));
            c.point3("location", 10, v.location, None);
        }
        // §20.4.15: 71-74.
        D::VertexPfaceFace(v) => {
            c.whole();
            for (n, i) in v.vertex_indices.iter().enumerate() {
                c.int(
                    &format!("vertex_indices[{n}]"),
                    71 + n as u16,
                    i64::from(*i),
                    Some(0),
                );
            }
        }
        // §20.4.16: 70, 75, 40, 41, 39, the elevation as 30, 210.
        D::Polyline(p) => {
            c.subclass("AcDb2dPolyline");
            c.int("flag", 70, i64::from(p.flag), Some(0));
            c.int("curve_type", 75, i64::from(p.curve_type), Some(0));
            c.real("default_start_width", 40, p.default_start_width, Some(0.0));
            c.real("default_end_width", 41, p.default_end_width, Some(0.0));
            c.real("thickness", 39, p.thickness, Some(0.0));
            c.real("elevation (10 is (0,0,elev))", 30, p.elevation, None);
            c.point3("extrusion", 210, p.extrusion, Some(Z));
        }
        // §20.4.17: the first RC's bits 0/1 give 75 = 5 / 6, the second's bit
        // 0 the closed bit of 70; the export also sets 70 bit 8 (3D).
        D::Polyline3d(p) => {
            c.subclass("AcDb3dPolyline");
            let curve = match p.flags[0] & 3 {
                1 => 5,
                2 => 6,
                _ => 0,
            };
            c.int("flags[0] (75: 5 or 6 when splined)", 75, curve, Some(0));
            c.bit("flags[1] & 1 (closed)", 70, 1, p.flags[1] & 1 != 0);
            c.bit("3D polyline (always)", 70, 8, true);
            c.bit("flags[0] & 3 (splined)", 70, 4, curve != 0);
            c.done(70);
            if let Some(n) = p.num_owned_objects {
                c.dwg_only("num_owned_objects", n);
            }
        }
        // §20.4.33: 71, 72; the export sets 70 bit 64 (polyface).
        D::PolyfaceMesh(p) => {
            c.subclass("AcDbPolyFaceMesh");
            c.int("vertex_count", 71, i64::from(p.vertex_count), None);
            c.int("face_count", 72, i64::from(p.face_count), None);
            if let Some(n) = p.num_owned_objects {
                c.dwg_only("num_owned_objects", n);
            }
        }
        D::MText(m) => score_mtext(&mut c, m),
        D::Dimension(d) => score_dimension(&mut c, d),
        D::Layer(l) => score_layer(&mut c, l),
        D::Ltype(l) => score_ltype(&mut c, l),
        D::Hatch(h) => score_hatch(&mut c, h),
        D::Style(s) => score_style(&mut c, s),
        D::Tolerance(t) => score_tolerance(&mut c, t),
        // §20.4.66: the entry header, then the RC unknown in 71.
        D::AppId(a) => {
            c.subclass("AcDbRegAppTableRecord");
            entry_header(&mut c, &a.header);
            c.int("unknown", 71, i64::from(a.unknown), Some(0));
        }
        D::DimStyle(d) => score_dimstyle(&mut c, d),
        D::VPort(v) => score_vport(&mut c, v),
        D::View(v) => score_view(&mut c, v),
        D::BlockRecord(b) => score_block_record(&mut c, b, rec.handle, dwg),
        // §20.4.51 and the other controls: BL num_entries; the TABLE's 70 is
        // the DXF Reference's "maximum number of entries".
        D::Control { control, .. } => {
            c.subclass("AcDbSymbolTable");
            c.int("num_entries", 70, i64::from(control.num_entries), None);
            // The RC the DIMSTYLE control carries past its entry count has no
            // §20.4.67 row; AcDbDimStyleTable's 71 is the one DXF candidate.
            if let Some(rc) = control.dimstyle_trailing_rc {
                c.subclass("AcDbDimStyleTable");
                c.int("dimstyle_trailing_rc", 71, i64::from(rc), None);
            }
        }
        D::Dictionary(d) => score_dictionary(&mut c, d, dwg),
        D::Viewport(v) => score_viewport(&mut c, v),
        // §20.4.74: 280 schema, 1 value.
        D::DictionaryVar(v) => {
            c.subclass("DictionaryVariables");
            c.int("schema", 280, i64::from(v.schema), None);
            c.text("value", 1, &v.value, None);
        }
        // §20.4.92: 70, 300, 140, 141, 290.
        D::Scale(s) => {
            c.subclass("AcDbScale");
            c.int("version", 70, i64::from(s.version), None);
            c.text("scale_name", 300, &s.scale_name, None);
            c.real("paper_units", 140, s.paper_units, None);
            c.real("drawing_units", 141, s.drawing_units, None);
            c.int("is_unit_scale", 290, i64::from(s.is_unit_scale), None);
        }
        // §20.4.104: the cloning flag in 280; the data stream stays raw.
        D::SortEntsTable(t) => score_sortents(&mut c, t),
        D::XRecord(x) => {
            c.subclass("AcDbXrecord");
            c.int("cloning_flags", 280, i64::from(x.cloning_flags), None);
            c.dwg_only("data (raw bytes)", x.data.len());
        }
        // The reactor count is the number of 330s in {ACAD_REACTORS.
        D::Placeholder(p) => {
            let n = reactor_count(dxf);
            c.derived(
                "num_reactors",
                "102{ACAD_REACTORS",
                n == p.num_reactors as usize,
                p.num_reactors.to_string(),
                n.to_string(),
            );
        }
        D::MLineStyle(m) => score_mlinestyle(&mut c, m),
        D::MLine(m) => score_mline(&mut c, m),
        // §20.4.82: the class version in 90; the IMAGE it serves is a handle.
        D::ImageDefReactor(r) => {
            c.subclass("AcDbRasterImageDefReactor");
            c.int("class_version", 90, i64::from(r.class_version), None);
        }
        // §20.4.91: 90, 70 display frame, 71 display quality, 72 units.
        D::RasterVariables(r) => {
            c.subclass("AcDbRasterVariables");
            c.int("class_version", 90, i64::from(r.class_version), None);
            c.int("display_frame", 70, i64::from(r.display_frame), None);
            c.int("display_quality", 71, i64::from(r.display_quality), None);
            c.int("units", 72, i64::from(r.units), None);
        }
        // No spec section: every number is a group 90, taken in the order
        // the DWG holds them (ARCHITECTURE.md §7aq).
        D::PersSubentManager(m) => {
            c.subclass("AcDbPersSubentManager");
            c.int("version", 90, i64::from(m.version), None);
            score_pers_arrays(&mut c, "arrays", &m.arrays);
        }
        // The same, then the final `B` as group 290.
        D::AssocPersSubentManager(m) => {
            c.subclass("AcDbAssocPersSubentManager");
            c.int("version", 90, i64::from(m.version), None);
            c.int("ids.count", 90, m.ids.len() as i64, None);
            for (i, id) in m.ids.iter().enumerate() {
                c.int(&format!("ids[{i}]"), 90, i64::from(*id), None);
            }
            score_pers_arrays(&mut c, "arrays", &m.arrays);
            c.int("flag", 290, i64::from(m.flag), None);
        }
        D::VisualStyle(v) => score_visualstyle(&mut c, v),
        D::Image(i) => score_image(&mut c, i, "AcDbRasterImage"),
        D::Wipeout(i) => score_image(&mut c, i, "AcDbWipeout"),
        D::ImageDef(d) => score_imagedef(&mut c, d),
        D::MLeaderStyle(m) => score_mleaderstyle(&mut c, m),
        D::TableStyle(t) => score_tablestyle(&mut c, t),
        D::Layout(l) => score_layout(&mut c, l),
        D::MLeader(m) => score_mleader(&mut c, m),
        D::Unhandled { .. } => return Err("no decoder".into()),
        D::Error { message, .. } => return Err(format!("decode error: {message}")),
        _ => return Err("no scorer yet".into()),
    }
    Ok(c.finish())
}

/// The arrays of a persistent-subentity manager: each one's count, then its
/// values, every one a group 90.
fn score_pers_arrays(c: &mut Ctx<'_>, name: &str, arrays: &[Vec<u32>]) {
    for (k, array) in arrays.iter().enumerate() {
        c.int(&format!("{name}[{k}].count"), 90, array.len() as i64, None);
        for (i, v) in array.iter().enumerate() {
            c.int(&format!("{name}[{k}][{i}]"), 90, i64::from(*v), None);
        }
    }
}

// MESH, which v5.4.1 does not prescribe, against the DXF Reference's
// AcDbSubDMesh: 71 the version, 72 the crease blend, 91 the subdivision
// level, 92 the vertex count and 10 the vertices, 93 the face list's entry
// count and the faces as groups 90 (each face's vertex count, then its
// indices), 94 the edge count and each edge's two vertex indices as groups
// 90, 95 the crease count and 140 the creases. The record ends with a 90
// the decoder checks to be 0 and does not surface. Groups of one code come
// in the order the decoder holds them, so each is taken as the next of its
// code; vertices, faces, edges and creases are scored under one field name
// each.
fn score_mesh(c: &mut Ctx<'_>, m: &dwg::entities::mesh::Mesh) {
    if !c.subclass("AcDbSubDMesh") {
        c.dwg_only("(the export has no AcDbSubDMesh subclass)", ());
        return;
    }
    c.int("version", 71, i64::from(m.version), None);
    c.int("blend_crease", 72, i64::from(m.blend_crease), None);
    c.int(
        "subdivision_level",
        91,
        i64::from(m.subdivision_level),
        None,
    );
    c.int("vertex count", 92, m.vertices.len() as i64, None);
    for v in &m.vertices {
        c.point3("vertices", 10, *v, None);
    }
    let entries: usize = m.faces.iter().map(|f| 1 + f.len()).sum();
    c.int("face list entries", 93, entries as i64, None);
    for f in &m.faces {
        c.int("faces.vertex count", 90, f.len() as i64, None);
        for x in f {
            c.int("faces.indices", 90, i64::from(*x), None);
        }
    }
    c.int("edge count", 94, m.edges.len() as i64, None);
    for (a, b) in &m.edges {
        c.int("edges.start", 90, i64::from(*a), None);
        c.int("edges.end", 90, i64::from(*b), None);
    }
    c.int("crease count", 95, m.creases.len() as i64, None);
    for x in &m.creases {
        c.real("creases", 140, *x, None);
    }
}

// §20.4.3: 10/20 with the elevation as 30, 11/21/31 when stored, 210, 39,
// 51, 50, 40, 41, 1, 71, 72 and the vertical alignment (73 on TEXT, 74 on
// ATTRIB and ATTDEF, whose 73 is the field length).
fn score_text(c: &mut Ctx<'_>, t: &dwg::entities::text::Text, v_align_code: Option<u16>) {
    c.real("insertion_point.x", 10, t.insertion_point.x, None);
    c.real("insertion_point.y", 20, t.insertion_point.y, None);
    c.real("elevation (30)", 30, t.elevation, None);
    match t.alignment_point {
        Some(p) => {
            c.real("alignment_point.x", 11, p.x, None);
            c.real("alignment_point.y", 21, p.y, None);
            c.real("elevation (31)", 31, t.elevation, None);
        }
        None => {
            // Taken so that `finish` does not count it a second time.
            if let Some(v) = c.take(11) {
                c.not_decoded("11 (alignment point the DWG does not store)", v.to_string());
            }
        }
    }
    c.point3("extrusion", 210, t.extrusion, Some(Z));
    c.real("thickness", 39, t.thickness, Some(0.0));
    c.angle("oblique_angle", 51, t.oblique_angle, Some(0.0));
    c.angle("rotation_angle", 50, t.rotation_angle, Some(0.0));
    c.real("height", 40, t.height, None);
    c.real("width_factor", 41, t.width_factor, Some(1.0));
    c.text("text", 1, &t.text, None);
    c.int("generation", 71, i64::from(t.generation), Some(0));
    c.int("h_align", 72, i64::from(t.h_align), Some(0));
    if let Some(code) = v_align_code {
        c.int("v_align", code, i64::from(t.v_align), Some(0));
    }
}

/// The attribute fields of ATTRIB and ATTDEF (DXF Reference AcDbAttribute):
/// 2 tag, 70 flags, 73 field length, 74 vertical alignment, the last 280
/// the lock position (R2010+; the first is a version), 71 the attribute
/// (MTEXT) type when written.
fn score_attribute(
    c: &mut Ctx<'_>,
    t: &dwg::entities::text::Text,
    tag: &str,
    flags: u8,
    field_length: i16,
    lock_position: bool,
    attribute_type: u8,
) {
    c.text("tag", 2, tag, None);
    c.int("flags", 70, i64::from(flags), Some(0));
    c.int("field_length", 73, i64::from(field_length), Some(0));
    c.int("v_align", 74, i64::from(t.v_align), Some(0));
    let two_80: Vec<usize> = c
        .free()
        .iter()
        .filter(|g| g.1 == 280)
        .map(|g| g.0)
        .collect();
    match two_80.as_slice() {
        [version, .., lock] => {
            c.take_at(*version);
            let x = c.record().groups[*lock].value.as_i64().unwrap_or(-1);
            c.take_at(*lock);
            c.derived(
                "lock_position",
                "280 (last)",
                x == i64::from(lock_position),
                lock_position.to_string(),
                x.to_string(),
            );
        }
        _ => c.dwg_only("lock_position", lock_position),
    }
    match c.take(71).and_then(DxfValue::as_i64) {
        Some(x) => c.derived(
            "attribute_type",
            "71",
            x == i64::from(attribute_type),
            attribute_type.to_string(),
            x.to_string(),
        ),
        None => c.dwg_only("attribute_type", attribute_type),
    }
}

// §20.4.46 with the DXF Reference's MTEXT groups: 10, 210, 11, 41, 46, 40,
// 71, 72, the text in 3-group chunks and a final 1, 73, 44. The extents
// (42, 43) are compared too; the spec says they are not in the DXF, but
// AutoCAD's exports write them.
fn score_mtext(c: &mut Ctx<'_>, m: &dwg::entities::mtext::MText) {
    c.subclass("AcDbMText");
    c.point3("insertion_point", 10, m.insertion_point, None);
    c.point3("extrusion", 210, m.extrusion, Some(Z));
    c.point3("x_axis_direction", 11, m.x_axis_direction, None);
    c.real("rect_width", 41, m.rect_width, None);
    if let Some(h) = m.rect_height {
        c.real("rect_height", 46, h, Some(0.0));
    }
    c.real("nominal_text_height", 40, m.nominal_text_height, None);
    c.int("attachment_point", 71, i64::from(m.attachment_point), None);
    c.int(
        "drawing_direction",
        72,
        i64::from(m.drawing_direction),
        None,
    );
    c.real("extents_width", 42, m.extents_width, None);
    c.real("extents_height", 43, m.extents_height, None);
    let mut chunks = String::new();
    let mut code_page = false;
    loop {
        match c.take_text(3) {
            Ok(Some(t)) => chunks.push_str(t),
            Ok(None) => break,
            Err(CodePage) => {
                code_page = true;
                break;
            }
        }
    }
    let last = if code_page {
        Err(CodePage)
    } else {
        c.take_text(1)
    };
    match last {
        Ok(Some(last)) => {
            chunks.push_str(last);
            let chunks = caret_decode(&chunks);
            c.derived(
                "text (3-group chunks + 1)",
                "1+3",
                m.text == chunks,
                format!("{} chars", m.text.chars().count()),
                format!("{} chars", chunks.chars().count()),
            );
        }
        Ok(None) | Err(CodePage) => c.dwg_only("text", m.text.chars().count()),
    }
    c.int("linespace_style", 73, i64::from(m.linespace_style), None);
    c.real("linespace_factor", 44, m.linespace_factor, None);
}

// §20.4.22 with §20.4.23 to §20.4.29, against the DXF Reference's
// AcDbDimension groups and each subtype's own subclass. The record is
// scored as a whole: every code is written once, except 10, whose
// subtype-specific point lives in AcDbDimension and whose z the DWG stores
// in full. Angles are degrees in the DXF (53, 51, 54, 52, 50) and radians
// in the DWG. The text midpoint, the clone insertion point and the angular
// 16-point are 2D points whose z is the common elevation (§20.4.22). The
// DWG's `flags` byte is not group 70: the type the subtype implies, 32 for
// its bit 1 and 128 for the absence of its bit 0, and 64 for an ordinate
// dimension's second flag byte make the group (§20.4.22, §20.4.23); that
// derived value is the one compared. Omitted groups stand for the DXF
// Reference's defaults (line spacing style 1 and factor 1, rotations 0,
// empty text). The handles (the dimension style in 3, the anonymous block
// in 2) are not decoded; 73 is the spec's "unknown" and not surfaced.
fn score_dimension(c: &mut Ctx<'_>, d: &dwg::entities::dimension::Dimension) {
    use dwg::entities::dimension::Dimension as Dm;
    let (common, kind, flag2) = match d {
        Dm::Linear(x) => (&x.common, 0, 0),
        Dm::Aligned(x) => (&x.common, 1, 0),
        Dm::Angular2Line(x) => (&x.common, 2, 0),
        Dm::Diameter(x) => (&x.common, 3, 0),
        Dm::Radius(x) => (&x.common, 4, 0),
        Dm::Angular3Pt(x) => (&x.common, 5, 0),
        Dm::Ordinate(x) => (&x.common, 6, i64::from(x.flag_2 & 1) * 64),
    };
    c.whole();
    if c.peek(280).is_some() {
        c.int("version_flag", 280, i64::from(common.version_flag), None);
    }
    c.point3("extrusion", 210, common.extrusion, Some(Z));
    c.point2("text_midpoint", 11, common.text_midpoint);
    c.real("elevation (z of 11)", 31, common.elevation, None);
    let flags = i64::from(common.flags);
    let type70 =
        kind | if flags & 2 != 0 { 32 } else { 0 } | if flags & 1 == 0 { 128 } else { 0 } | flag2;
    c.int(
        "flags (the subtype and flag bits, as group 70)",
        70,
        type70,
        None,
    );
    c.text("user_text", 1, &common.user_text, Some(""));
    c.angle("text_rotation", 53, common.text_rotation, Some(0.0));
    c.angle("horiz_dir", 51, common.horiz_dir, Some(0.0));
    c.dwg_only(
        "ins_scale.x (41 is the line spacing factor)",
        common.ins_scale.x,
    );
    c.dwg_only("ins_scale.y (42 is the measurement)", common.ins_scale.y);
    c.real("ins_scale.z", 43, common.ins_scale.z, Some(1.0));
    c.angle("ins_rotation", 54, common.ins_rotation, Some(0.0));
    c.int("attachment", 71, i64::from(common.attachment), None);
    c.int(
        "line_spacing_style",
        72,
        i64::from(common.line_spacing_style),
        Some(1),
    );
    c.real(
        "line_spacing_factor",
        41,
        common.line_spacing_factor,
        Some(1.0),
    );
    c.real("actual_measurement", 42, common.actual_measurement, None);
    if c.peek(74).is_some() {
        c.int("flip_arrow_1", 74, i64::from(common.flip_arrow_1), None);
        c.int("flip_arrow_2", 75, i64::from(common.flip_arrow_2), None);
    }
    c.point2("def_point_12", 12, common.def_point_12);
    match d {
        Dm::Linear(x) => {
            c.point3("def_point_13", 13, x.def_point_13, None);
            c.point3("def_point_14", 14, x.def_point_14, None);
            c.point3("def_point_10", 10, x.def_point_10, None);
            c.angle(
                "extension_line_rotation",
                52,
                x.extension_line_rotation,
                Some(0.0),
            );
            c.angle("dim_rotation", 50, x.dim_rotation, Some(0.0));
        }
        Dm::Aligned(x) => {
            c.point3("def_point_13", 13, x.def_point_13, None);
            c.point3("def_point_14", 14, x.def_point_14, None);
            c.point3("def_point_10", 10, x.def_point_10, None);
            c.angle(
                "extension_line_rotation",
                52,
                x.extension_line_rotation,
                Some(0.0),
            );
        }
        Dm::Ordinate(x) => {
            c.point3("def_point_10", 10, x.def_point_10, None);
            c.point3("feature_location_13", 13, x.feature_location_13, None);
            c.point3("leader_endpoint_14", 14, x.leader_endpoint_14, None);
        }
        Dm::Angular3Pt(x) => {
            c.point3("def_point_10", 10, x.def_point_10, None);
            c.point3("def_point_13", 13, x.def_point_13, None);
            c.point3("def_point_14", 14, x.def_point_14, None);
            c.point3("def_point_15", 15, x.def_point_15, None);
        }
        Dm::Angular2Line(x) => {
            c.point2("def_point_16", 16, x.def_point_16);
            c.real("elevation (z of 16)", 36, common.elevation, None);
            c.point3("def_point_13", 13, x.def_point_13, None);
            c.point3("def_point_14", 14, x.def_point_14, None);
            c.point3("def_point_15", 15, x.def_point_15, None);
            c.point3("def_point_10", 10, x.def_point_10, None);
        }
        Dm::Radius(x) => {
            c.point3("def_point_10", 10, x.def_point_10, None);
            c.point3("def_point_15", 15, x.def_point_15, None);
            c.real("leader_length", 40, x.leader_length, None);
        }
        Dm::Diameter(x) => {
            c.point3("def_point_15", 15, x.def_point_15, None);
            c.point3("def_point_10", 10, x.def_point_10, None);
            c.real("leader_length", 40, x.leader_length, None);
        }
    }
}

fn reactor_count(dxf: &DxfRecord) -> usize {
    let mut inside = false;
    let mut n = 0;
    for g in &dxf.groups {
        if g.code == 102 {
            inside = g.value.as_str() == Some("{ACAD_REACTORS");
            continue;
        }
        if inside && g.code == 330 {
            n += 1;
        }
    }
    n
}

// §20.4.49 against the DXF Reference's AcDbFcf: 10 the insertion point, 11
// the x direction, 210 the extrusion and 1 the text. The R13-R14 unknown
// short, height and dimgap are not in an R2000+ record (the decoder leaves
// them 0) and R14 has no DXF twin; the dimension style is a handle.
fn score_tolerance(c: &mut Ctx<'_>, t: &dwg::entities::tolerance::Tolerance) {
    if !c.subclass("AcDbFcf") {
        c.dwg_only("(the export has no AcDbFcf subclass)", ());
        return;
    }
    c.point3("insertion_point", 10, t.insertion_point, None);
    c.point3("x_axis_direction", 11, t.x_axis_direction, None);
    c.point3("extrusion", 210, t.extrusion, Some(Z));
    c.text("text_string", 1, &t.text_string, None);
}

/// The entry header every table record starts with (§20.4.54 and the other
/// table sections): the name in 2, then the `64-flag` and `Xdep`, which the
/// spec places in bits 64 and 16 of group 70. AutoCAD's export never writes
/// bit 64 (measured: clear in all 571 table records of the 42 exports,
/// while every DWG entry sets the flag), so the 64-flag is `dwg-only`. The
/// xref index has no DXF group.
fn entry_header(c: &mut Ctx<'_>, h: &dwg::tables::TableEntryHeader) {
    c.text("name", 2, &h.name, None);
    flags_64_xdep(c, h);
    c.done(70);
}

/// The 64-flag and Xdep of an entry header (see [`entry_header`]); the
/// caller takes group 70 once all its bits are scored.
fn flags_64_xdep(c: &mut Ctx<'_>, h: &dwg::tables::TableEntryHeader) {
    c.dwg_only("flag_64 (the export never writes bit 64)", h.flag_64);
    c.bit("is_xref_dependent (Xdep)", 70, 16, h.is_xref_dependent);
    c.dwg_only("xref_index_plus_1", h.xref_index_plus_1);
}

// §20.4.54: 70 (frozen 1, frozen-in-new 2, locked 4), 62 (negative when off),
// 290 plot flag, 370 lineweight. The DWG packs frozen 1, 2, frozen-in-new 4,
// locked 8, plot 16 and the lineweight index (mask 0x3E0) into one BS. The
// spec glosses bit 2 "on"; every corpus layer is on with the bit clear, so it
// is scored, as the decoder names it, as "off".
fn score_layer(c: &mut Ctx<'_>, l: &dwg::tables::layer::Layer) {
    c.subclass("AcDbLayerTableRecord");
    c.text("name", 2, &l.header.name, None);
    c.bit("frozen (flags & 1)", 70, 1, l.flags & 1 != 0);
    c.bit(
        "frozen in new viewports (flags & 4)",
        70,
        2,
        l.flags & 4 != 0,
    );
    c.bit("locked (flags & 8)", 70, 4, l.flags & 8 != 0);
    flags_64_xdep(c, &l.header);
    c.done(70);
    // A true-colour layer exports 420 with the RGB and a 62 AutoCAD derives
    // from it (the nearest index); the DWG stores no index for it (the CMC's
    // index word is 0) and the decoder does not surface the RGB.
    if let Some(rgb) = c.take(420).and_then(DxfValue::as_i64) {
        let sign = c.peek(62).and_then(DxfValue::as_i64).unwrap_or(0);
        let off = l.flags & 2 != 0;
        c.take(62);
        c.derived(
            "off (flags & 2)",
            "62 sign",
            off == (sign < 0),
            off.to_string(),
            (sign < 0).to_string(),
        );
        c.not_decoded(
            "420 (true colour; 62 its nearest index)",
            format!("{rgb:#08X}"),
        );
    }
    match c.take(62).and_then(DxfValue::as_i64) {
        Some(x) => {
            let off = l.flags & 2 != 0;
            c.derived(
                "off (flags & 2)",
                "62 sign",
                off == (x < 0),
                off.to_string(),
                (x < 0).to_string(),
            );
            let idx = i64::from(l.color_index);
            c.derived(
                "color_index",
                "62",
                idx == x.abs(),
                idx.to_string(),
                x.abs().to_string(),
            );
        }
        None => c.dwg_only("color_index", l.color_index),
    }
    c.int("plot_flag", 290, i64::from(l.plot_flag), Some(1));
    match lineweight_of(i64::from(l.lineweight)) {
        Some(lw) => c.int("lineweight", 370, lw, None),
        None => c.dwg_only("lineweight (index outside the table)", l.lineweight),
    }
}

// §20.4.58: 2, 70, 3, 40, 72, 73, then per dash 49, 74, 75, 44, 45, 46, 50.
fn score_ltype(c: &mut Ctx<'_>, l: &dwg::tables::ltype::LtypeEntry) {
    c.subclass("AcDbLinetypeTableRecord");
    entry_header(c, &l.header);
    c.text("description", 3, &l.description, None);
    c.int("alignment", 72, i64::from(l.alignment), None);
    c.int("dash count", 73, l.dashes.len() as i64, None);
    c.real("pattern_length", 40, l.pattern_length, None);
    for (i, d) in l.dashes.iter().enumerate() {
        c.real(&format!("dashes[{i}].length"), 49, d.length, None);
        c.int(
            &format!("dashes[{i}].shape_flag"),
            74,
            i64::from(d.shape_flag),
            None,
        );
        if d.shape_flag != 0 {
            c.int(
                &format!("dashes[{i}].shape_number"),
                75,
                i64::from(d.shape_number),
                None,
            );
            c.real(&format!("dashes[{i}].scale"), 46, d.scale, None);
            c.angle(&format!("dashes[{i}].rotation"), 50, d.rotation, None);
            c.real(&format!("dashes[{i}].x_offset"), 44, d.x_offset, None);
            c.real(&format!("dashes[{i}].y_offset"), 45, d.y_offset, None);
        }
    }
}

// §20.4.56: 70 (shape file 1, vertical 4), 40, 41, 50, 71, 42, 3, 4.
fn score_style(c: &mut Ctx<'_>, s: &dwg::tables::style::StyleEntry) {
    c.subclass("AcDbTextStyleTableRecord");
    c.text("name", 2, &s.header.name, None);
    c.bit("shape file (flags & 1)", 70, 1, s.flags & 1 != 0);
    c.bit("vertical (flags & 4)", 70, 4, s.flags & 4 != 0);
    flags_64_xdep(c, &s.header);
    c.done(70);
    c.real("fixed_height", 40, s.fixed_height, None);
    c.real("width_factor", 41, s.width_factor, None);
    c.angle("oblique_angle", 50, s.oblique_angle, None);
    c.int("generation", 71, i64::from(s.generation), None);
    c.real("last_height", 42, s.last_height, None);
    c.text("font_filename", 3, &s.font_filename, None);
    c.text("bigfont_filename", 4, &s.bigfont_filename, Some(""));
}

// §20.4.68, for the fields the decoder surfaces. AutoCAD omits a DIMSTYLE
// group whose value is the imperial default; those defaults are given here.
fn score_dimstyle(c: &mut Ctx<'_>, d: &dwg::tables::dimstyle::DimStyleEntry) {
    c.subclass("AcDbDimStyleTableRecord");
    entry_header(c, &d.header);
    c.real("dimscale", 40, d.dimscale, Some(1.0));
    c.real("dimasz", 41, d.dimasz, Some(0.18));
    c.real("dimexo", 42, d.dimexo, Some(0.0625));
    c.real("dimexe", 44, d.dimexe, Some(0.18));
    c.real("dimtxt", 140, d.dimtxt, Some(0.18));
    c.real("dimcen", 141, d.dimcen, Some(0.09));
    c.real("dimtfac", 146, d.dimtfac, Some(1.0));
    c.real("dimlfac", 144, d.dimlfac, Some(1.0));
    c.int("dimtih", 73, i64::from(d.dimtih), Some(1));
    c.int("dimtoh", 74, i64::from(d.dimtoh), Some(1));
    c.int("dimtad", 77, i64::from(d.dimtad), Some(0));
    c.int("dimtolj", 283, i64::from(d.dimtolj), Some(1));
    c.real("dimaltf", 143, d.dimaltf, Some(25.4));
    c.real("dimaltrnd", 148, d.dimaltrnd, Some(0.0));
    c.int("dimupt", 288, i64::from(d.dimupt), Some(0));
}

// §20.4.64. The DWG's 41 is the aspect ratio times the view height (spec:
// "this number must be divided by the 40-value to produce the aspect ratio").
fn score_vport(c: &mut Ctx<'_>, v: &dwg::tables::vport::VportEntry) {
    c.subclass("AcDbViewportTableRecord");
    entry_header(c, &v.header);
    c.real("view_height", 40, v.view_height, None);
    match v.aspect_ratio() {
        Some(r) => c.real("aspect_ratio() (view_width / view_height)", 41, r, None),
        None => c.dwg_only("aspect_ratio() (view height 0)", v.view_width),
    }
    c.point2("view_center", 12, v.view_center);
    c.point3("view_target", 17, v.view_target, None);
    c.point3("view_direction", 16, v.view_direction, None);
    c.angle("view_twist", 51, v.view_twist, None);
    c.real("lens_length", 42, v.lens_length, None);
    c.real("front_clip", 43, v.front_clip, None);
    c.real("back_clip", 44, v.back_clip, None);
    c.int("view_mode", 71, i64::from(v.view_mode), None);
    c.int("render_mode", 281, i64::from(v.render_mode), None);
    c.point2("lower_left", 10, v.lower_left);
    c.point2("upper_right", 11, v.upper_right);
    c.bit("ucs_at_origin", 74, 2, v.ucs_at_origin);
    c.done(74);
    c.int("ucs_per_vport", 65, i64::from(v.ucs_per_vport), None);
    c.point2("snap_base", 13, v.snap_base);
    c.point2("snap_spacing", 14, v.snap_spacing);
    c.point2("grid_spacing", 15, v.grid_spacing);
    c.angle("snap_rotation", 50, v.snap_rotation, None);
}

/// AutoCAD numbers anonymous blocks itself: the DWG stores the family
/// (`*U`, `*D`, `*T`, `*X`, `*A`, `*E`) and the export appends a number.
/// `Some(family)` when `stored` is a bare family and `exported` is that
/// family plus digits.
fn anonymous_family<'s>(stored: &'s str, exported: &str) -> Option<&'s str> {
    let bare = stored.len() == 2 && stored.starts_with('*');
    let rest = exported.strip_prefix(stored)?;
    (bare && !rest.is_empty() && rest.bytes().all(|d| d.is_ascii_digit())).then_some(stored)
}

/// Scores a block name in group 2, allowing for AutoCAD's numbering of
/// anonymous blocks (counted under its own field name).
fn block_name(c: &mut Ctx<'_>, field: &str, name: &str) {
    let exported = c.peek(2).and_then(DxfValue::as_str).unwrap_or("");
    if anonymous_family(name, exported).is_some() {
        c.take(2);
        c.derived(
            &format!("{field} (anonymous; AutoCAD numbers it)"),
            "2",
            true,
            format!("{name:?}"),
            format!("{exported:?}"),
        );
    } else {
        c.text(field, 2, name, None);
    }
}

// §20.4.50 against the DXF Reference's AcDbMline: 40 the scale, 70 the
// justification, 71 the flags, 72 and 73 the vertex and element counts, 10
// the base point and 210 the extrusion, then per vertex 11, 12 and 13 (its
// position, segment direction and miter direction) and per element 74 with
// that many 41 segment parameters and 75 with that many 42 area-fill
// parameters. The groups of the vertices come in the order the decoder
// holds them, so each is taken as the next group of its code. The style (2
// and 340) is a name and a handle.
fn score_mline(c: &mut Ctx<'_>, m: &dwg::entities::mline::MLine) {
    use dwg::entities::mline::Justification;
    if !c.subclass("AcDbMline") {
        c.dwg_only("(the export has no AcDbMline subclass)", ());
        return;
    }
    c.real("scale_factor", 40, m.scale_factor, None);
    let just = match m.justification {
        Justification::Top => 0,
        Justification::Zero => 1,
        Justification::Bottom => 2,
        Justification::Unknown(x) => i64::from(x),
    };
    c.int("justification", 70, just, None);
    c.int(
        "open_closed_flags",
        71,
        i64::from(m.open_closed_flags),
        None,
    );
    c.int("vertex count", 72, m.vertices.len() as i64, None);
    c.int("num_lines", 73, i64::from(m.num_lines), None);
    c.point3("scale_point", 10, m.scale_point, None);
    c.point3("extrusion", 210, m.extrusion, Some(Z));
    for v in &m.vertices {
        c.point3("vertices.vertex_point", 11, v.vertex_point, None);
        c.point3("vertices.segment_direction", 12, v.segment_direction, None);
        c.point3("vertices.miter_direction", 13, v.miter_direction, None);
        for r in &v.sub_records {
            c.int(
                "vertices.elements.segment parameter count",
                74,
                r.segment_parameters.len() as i64,
                None,
            );
            for p in &r.segment_parameters {
                c.real("vertices.elements.segment_parameters", 41, *p, None);
            }
            c.int(
                "vertices.elements.area fill count",
                75,
                r.area_fill_parameters.len() as i64,
                None,
            );
            for p in &r.area_fill_parameters {
                c.real("vertices.elements.area_fill_parameters", 42, *p, None);
            }
        }
    }
}

// §20.4.52. BLOCK_RECORD's own DXF record holds the name; the flags, base
// point, xref path and description are on its BLOCK entity, the record the
// export owns by this BLOCK_RECORD (its 330), in that entity's 70, 10/20/30,
// 1 and 4. The DWG stores an anonymous family's name without its index
// (`*Paper_Space` for `*Paper_Space0`); the name scored is the one resolved
// through the BLOCK entity (#70), which carries the index.
fn score_block_record(
    c: &mut Ctx<'_>,
    b: &dwg::tables::block_record::BlockRecord,
    handle: u64,
    dwg: &DwgIndex<'_>,
) {
    c.subclass("AcDbBlockTableRecord");
    let block = c
        .doc()
        .records
        .iter()
        .find(|r| r.kind == "BLOCK" && r.owner == Some(handle));
    let resolved = block
        .and_then(|r| r.handle)
        .and_then(|h| dwg.get(&h))
        .and_then(|r| match &r.decoded {
            DecodedEntity::Block(blk) => Some(blk.name.clone()),
            _ => None,
        })
        .filter(|n| {
            n.strip_prefix(b.header.name.as_str())
                .is_some_and(|rest| rest.bytes().all(|d| d.is_ascii_digit()))
        })
        .unwrap_or_else(|| b.header.name.clone());
    block_name(c, "name (resolved through BLOCK, #70)", &resolved);
    c.dwg_only("xref_index_plus_1", b.header.xref_index_plus_1);
    if let Some(n) = b.num_owned_objects {
        c.dwg_only("num_owned_objects", n);
    }
    let Some(block) = block else {
        c.dwg_only("BLOCK fields (the export owns no BLOCK by this record)", ());
        return;
    };
    match b.block_sentinel_handle {
        Some(h) => c.derived(
            "block_sentinel_handle",
            "BLOCK.5",
            Some(h) == block.handle,
            format!("{h:X}"),
            block.handle.map_or_else(String::new, |x| format!("{x:X}")),
        ),
        None => c.dwg_only("block_sentinel_handle (not decoded on this release)", ()),
    }
    let h = b.header.clone();
    let (anon, atts, xref, overlay) = (b.is_anonymous, b.has_attribs, b.is_xref, b.xref_overlay);
    let (base, path, desc) = (b.base_point, b.xref_path.clone(), b.description.clone());
    c.on_other("BLOCK", block, |c| {
        c.subclass("AcDbBlockBegin");
        c.bit("is_anonymous", 70, 1, anon);
        c.bit("has_attribs", 70, 2, atts);
        c.bit("is_xref", 70, 4, xref);
        c.bit("xref_overlay", 70, 8, overlay);
        c.bit("is_xref_dependent (Xdep)", 70, 16, h.is_xref_dependent);
        c.done(70);
        c.point3("base_point", 10, base, None);
        c.text("xref_path", 1, &path, Some(""));
        c.text("description", 4, &desc, Some(""));
    });
}

// §20.4.44: 281 cloning, 280 hard owner (omitted when 0), then a 3 / 350 (or
// 360) pair per entry. An entry the export added for a save-time object (its
// target is not in the DWG, its key not among the DWG's) is set aside and
// listed, not compared: the pairing report explains save-time objects.
fn score_dictionary(c: &mut Ctx<'_>, d: &dwg::objects::dictionary::Dictionary, dwg: &DwgIndex<'_>) {
    c.subclass("AcDbDictionary");
    c.int("cloning_flag", 281, i64::from(d.cloning_flag), Some(0));
    c.int("hard_owner", 280, i64::from(d.hard_owner), Some(0));
    let free = c.free();
    let mut entries: Vec<(usize, String, Option<u64>)> = Vec::new();
    // A key held as code-page text cannot be compared: it is left untaken,
    // so it stays not-decoded, and the decoded keys it could match are
    // reported as not compared rather than as absent from the export.
    let mut code_page = false;
    for (k, &(i, code, v)) in free.iter().enumerate() {
        if code != 3 {
            continue;
        }
        if matches!(v, DxfValue::CodePageText(_)) {
            code_page = true;
            continue;
        }
        let target = free
            .get(k + 1)
            .filter(|(_, c2, _)| matches!(c2, 350 | 360))
            .and_then(|(_, _, h)| h.as_handle());
        entries.push((i, caret_decode(v.as_str().unwrap_or("")), target));
    }
    let mut keys = Vec::new();
    for (i, key, target) in entries {
        let save_time = target.is_some_and(|h| !dwg.contains_key(&h)) && !d.keys.contains(&key);
        c.take_at(i);
        if save_time {
            c.not_decoded("3 (entry for a save-time object)", key);
        } else {
            keys.push(key);
        }
    }
    // Keys are matched by name: a key one file has and the other lacks is
    // listed on its own instead of shifting every key after it.
    for k in &d.keys {
        if keys.iter().any(|x| x == k) {
            c.derived("key", "3", true, format!("{k:?}"), format!("{k:?}"));
        } else {
            let field = if code_page {
                "key (not compared: the export has code-page keys)"
            } else {
                "key (absent from the export)"
            };
            c.push(
                field,
                "3".into(),
                Status::DwgOnly,
                format!("{k:?}"),
                String::new(),
            );
        }
    }
    for x in keys.iter().filter(|x| !d.keys.iter().any(|k| k == *x)) {
        c.derived(
            "key (absent from the decode)",
            "3",
            false,
            String::new(),
            format!("{x:?}"),
        );
    }
    let common_dwg: Vec<&String> = d
        .keys
        .iter()
        .filter(|k| keys.iter().any(|x| x == *k))
        .collect();
    let common_dxf: Vec<&str> = keys
        .iter()
        .map(String::as_str)
        .filter(|x| d.keys.iter().any(|k| k == x))
        .collect();
    let same_order = common_dwg
        .iter()
        .map(|k| k.as_str())
        .eq(common_dxf.iter().copied());
    c.derived(
        "key order",
        "3",
        same_order,
        format!("{} keys", common_dwg.len()),
        format!("{} keys", common_dxf.len()),
    );
}

// §20.4.101.1 (the layout "until R21", R2000 to R2007): 3, 70, 71, 40, 41,
// 280, 281, then each row style in the order data, title, header: 7 (the
// text style, a handle the decoder does not read), 140, 170, 62, 63, 283,
// R2007+ 90, 91 and 1, and per border 274-279, 284-289 and 64-69. The
// export writes the borders grouped by code (all six 27x, then 28x, then
// 6x), so each is taken by its own code. Colours are TC; their DXF number
// is the index (0 ByBlock, 256 ByLayer).
fn score_tablestyle(c: &mut Ctx<'_>, t: &dwg::objects::acad_table_style::AcadTableStyle) {
    c.subclass("AcDbTableStyle");
    c.text("description", 3, &t.description, None);
    c.int("flow_direction", 70, i64::from(t.flow_direction), None);
    c.int("bit_flags", 71, i64::from(t.bit_flags), None);
    c.real("horizontal_margin", 40, t.horizontal_margin, None);
    c.real("vertical_margin", 41, t.vertical_margin, None);
    // R2010+ exports write two 280s before the first row style; the
    // suppress-title flag is the second (examples/probe_tablestyle.rs).
    let groups = &c.record().groups;
    let head = groups
        .iter()
        .position(|g| g.code == 100 && g.value.as_str() == Some("AcDbTableStyle"))
        .map_or(&groups[..0], |s| {
            let rest = &groups[s + 1..];
            &rest[..rest.iter().position(|g| g.code == 7).unwrap_or(rest.len())]
        });
    if head.iter().filter(|g| g.code == 280).count() > 1 {
        if let Some(v) = c.take(280) {
            c.not_decoded(
                "280 (the first of two, not the suppress flag)",
                v.to_string(),
            );
        }
    }
    c.int("suppress_title", 280, i64::from(t.suppress_title), None);
    c.int("suppress_header", 281, i64::from(t.suppress_header), None);
    for (name, r) in [
        ("data", &t.data),
        ("title", &t.title),
        ("header", &t.header),
    ] {
        c.real(&format!("{name}.text_height"), 140, r.text_height, None);
        c.int(
            &format!("{name}.text_alignment"),
            170,
            i64::from(r.text_alignment),
            None,
        );
        c.int(
            &format!("{name}.text_color"),
            62,
            aci_of(&r.text_color),
            None,
        );
        c.int(
            &format!("{name}.fill_color"),
            63,
            aci_of(&r.fill_color),
            None,
        );
        c.int(
            &format!("{name}.background_enabled"),
            283,
            i64::from(r.background_enabled),
            None,
        );
        if let Some(v) = r.data_type {
            c.int(&format!("{name}.data_type"), 90, i64::from(v), None);
        }
        if let Some(v) = r.data_unit_type {
            c.int(&format!("{name}.data_unit_type"), 91, i64::from(v), None);
        }
        if let Some(v) = &r.format_string {
            c.text(&format!("{name}.format_string"), 1, v, None);
        }
        for (k, b) in (0u16..).zip(r.borders.iter()) {
            c.int(
                &format!("{name}.borders[{k}].line_weight"),
                274 + k,
                i64::from(b.line_weight),
                None,
            );
            c.int(
                &format!("{name}.borders[{k}].visible"),
                284 + k,
                i64::from(b.visible),
                None,
            );
            c.int(
                &format!("{name}.borders[{k}].color"),
                64 + k,
                aci_of(&b.color),
                None,
            );
        }
    }
}

// §20.4.73 with the DXF Reference's MLINESTYLE groups: 2, 70, 3, 62, 51, 52,
// 71, then 49 / 62 / 6 per line. The flag bits are remapped per the spec's
// table (DWG 1 -> DXF 2, 2 -> 1, 32 -> 64, 64 -> 32, 512 -> 1024, 1024 -> 512).
fn score_mlinestyle(c: &mut Ctx<'_>, m: &dwg::objects::acad_mlinestyle::AcadMlinestyle) {
    c.subclass("AcDbMlineStyle");
    c.text("name", 2, &m.name, None);
    let f = i64::from(m.flags);
    let mut dxf_flags = f & (16 | 256);
    for (dwg_bit, dxf_bit) in [(1, 2), (2, 1), (32, 64), (64, 32), (512, 1024), (1024, 512)] {
        if f & dwg_bit != 0 {
            dxf_flags |= dxf_bit;
        }
    }
    c.int("flags (remapped)", 70, dxf_flags, None);
    c.text("description", 3, &m.description, None);
    c.int("fill_color", 62, aci_of(&m.fill_color), None);
    c.angle("start_angle", 51, m.start_angle, None);
    c.angle("end_angle", 52, m.end_angle, None);
    c.int("line count", 71, m.lines.len() as i64, None);
    for (i, l) in m.lines.iter().enumerate() {
        c.real(&format!("lines[{i}].offset"), 49, l.offset, None);
        c.int(&format!("lines[{i}].color"), 62, aci_of(&l.color), None);
        c.dwg_only(&format!("lines[{i}].linetype_index"), l.linetype_index);
    }
}

/// Scores the groups one after another: each step scopes the lookups to
/// the next `n` groups of the subclass, runs the scorers there, and moves
/// on past what they took, so a group order the spec fixes is checked as
/// well as its values.
struct Seq {
    pos: usize,
    end: usize,
}

impl Seq {
    fn new(c: &Ctx<'_>) -> Self {
        Self {
            pos: c.scope.0,
            end: c.scope.1,
        }
    }

    fn step(&mut self, c: &mut Ctx<'_>, n: usize, f: impl FnOnce(&mut Ctx<'_>)) {
        let to = (self.pos + n).min(self.end);
        c.scope = (self.pos, to);
        f(c);
        self.pos += (self.pos..to).filter(|&i| c.used[i]).count();
    }

    /// Skips `n` groups the decoder does not surface, leaving them among
    /// the record's not-decoded groups.
    fn skip(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.end);
    }
}

// §20.4.75 against the DXF Reference's AcDbHatch groups, which the export
// writes in the order the spec prints them: the dummy elevation point (10
// and 20 zero, 30 the elevation), the extrusion in 210/220/230, 2, 70, 71,
// 91 and per path 92, its vertices or edges (72 the edge type) and 97 with
// that many 330 handles, then 75, 76, the pattern block when the fill is
// not solid (52, 41, 77, 78 and per line 53, 43/44, 45/46, 79 and 49 per
// dash), 47 when a path is derived, 98 with the seed points and, for a
// gradient, 450 to 470. The groups are scored in that order, each within
// the next few groups of the record, so an out-of-order group shows as a
// missing one; the paths, edges and pattern lines are scored under one
// field name each. The DWG's angles are radians, the DXF's 50 to 53
// degrees. A gradient colour is a CMC word: the DWG keeps the method byte
// 0xC2 and the RGB, which is group 421; group 63 is the ACI AutoCAD
// derives from it and the DWG does not store. Only line edges and
// polylines occur in the corpus; another edge type ends the scoring of its
// record, since its groups are not walked.
fn score_hatch(c: &mut Ctx<'_>, h: &dwg::entities::hatch::Hatch) {
    use dwg::entities::hatch::{HatchEdge, HatchPathSegments};
    if !c.subclass("AcDbHatch") {
        c.dwg_only("(the export has no AcDbHatch subclass)", ());
        return;
    }
    let mut q = Seq::new(c);
    q.skip(2);
    q.step(c, 1, |c| {
        c.real("elevation (z of 10)", 30, h.elevation, None)
    });
    q.step(c, 3, |c| c.point3("extrusion", 210, h.extrusion, Some(Z)));
    q.step(c, 1, |c| c.text("pattern_name", 2, &h.pattern_name, None));
    q.step(c, 1, |c| {
        c.int("solid_fill", 70, i64::from(h.solid_fill), None)
    });
    q.step(c, 1, |c| {
        c.int("associative", 71, i64::from(h.associative), None)
    });
    q.step(c, 1, |c| {
        c.int("path count", 91, h.paths.len() as i64, None)
    });
    for p in &h.paths {
        q.step(c, 1, |c| c.int("paths.flags", 92, i64::from(p.flags), None));
        match &p.segments {
            HatchPathSegments::Polyline {
                has_bulge,
                is_closed,
                vertices,
            } => {
                q.step(c, 1, |c| {
                    c.int("paths.has_bulge", 72, i64::from(*has_bulge), None)
                });
                q.step(c, 1, |c| {
                    c.int("paths.is_closed", 73, i64::from(*is_closed), None)
                });
                q.step(c, 1, |c| {
                    c.int("paths.vertex count", 93, vertices.len() as i64, None)
                });
                for (v, b) in vertices {
                    q.step(c, 2, |c| c.point2("paths.vertices", 10, *v));
                    if let Some(b) = b {
                        q.step(c, 1, |c| c.real("paths.bulges", 42, *b, None));
                    }
                }
            }
            HatchPathSegments::Edges(edges) => {
                q.step(c, 1, |c| {
                    c.int("paths.edge count", 93, edges.len() as i64, None)
                });
                for e in edges {
                    match e {
                        HatchEdge::Line { start, end } => {
                            q.step(c, 1, |c| c.int("paths.edges.type", 72, 1, None));
                            q.step(c, 2, |c| c.point2("paths.edges.start", 10, *start));
                            q.step(c, 2, |c| c.point2("paths.edges.end", 11, *end));
                        }
                        _ => {
                            // Its groups are not walked, so no later group could be paired by
                            // position: the rest of the record stays unscored.
                            c.dwg_only("paths.edges (type not scored; the rest of the record is not scored)", ());
                            return;
                        }
                    }
                }
            }
        }
        q.step(c, 1, |c| {
            c.int(
                "paths.boundary handle count",
                97,
                i64::from(p.num_boundary_handles),
                None,
            );
        });
        q.skip(p.num_boundary_handles as usize);
    }
    q.step(c, 1, |c| {
        c.int("pattern_style", 75, i64::from(h.pattern_style), None)
    });
    q.step(c, 1, |c| {
        c.int("pattern_type", 76, i64::from(h.pattern_type), None)
    });
    if !h.solid_fill {
        q.step(c, 1, |c| {
            c.angle("pattern_angle", 52, h.pattern_angle, None)
        });
        q.step(c, 1, |c| c.real("pattern_scale", 41, h.pattern_scale, None));
        q.step(c, 1, |c| {
            c.int("pattern_double", 77, i64::from(h.pattern_double), None)
        });
        q.step(c, 1, |c| {
            c.int("pattern line count", 78, h.pattern_lines.len() as i64, None);
        });
        for l in &h.pattern_lines {
            q.step(c, 1, |c| c.angle("pattern_lines.angle", 53, l.angle, None));
            q.step(c, 2, |c| {
                c.real("pattern_lines.origin.x", 43, l.origin.x, None);
                c.real("pattern_lines.origin.y", 44, l.origin.y, None);
            });
            q.step(c, 2, |c| {
                c.real("pattern_lines.offset.x", 45, l.offset.x, None);
                c.real("pattern_lines.offset.y", 46, l.offset.y, None);
            });
            q.step(c, 1, |c| {
                c.int("pattern_lines.dash count", 79, l.dashes.len() as i64, None);
            });
            for d in &l.dashes {
                q.step(c, 1, |c| c.real("pattern_lines.dashes", 49, *d, None));
            }
        }
    }
    if h.paths.iter().any(|p| p.flags & 4 != 0) {
        q.step(c, 1, |c| c.real("pixel_size", 47, h.pixel_size, None));
    }
    q.step(c, 1, |c| {
        c.int("seed point count", 98, h.seed_points.len() as i64, None)
    });
    for (x, y) in &h.seed_points {
        q.step(c, 2, |c| {
            c.real("seed_points.x", 10, *x, None);
            c.real("seed_points.y", 20, *y, None);
        });
    }
    if let Some(g) = &h.gradient {
        q.step(c, 1, |c| c.int("gradient flag", 450, 1, None));
        q.skip(1);
        q.step(c, 1, |c| c.real("gradient.angle", 460, g.angle, None));
        q.step(c, 1, |c| c.real("gradient.shift", 461, g.shift, None));
        q.step(c, 1, |c| {
            c.int(
                "gradient.is_single_color",
                452,
                i64::from(g.is_single_color),
                None,
            );
        });
        q.step(c, 1, |c| c.real("gradient.tint", 462, g.tint, None));
        q.step(c, 1, |c| {
            c.int("gradient.color count", 453, g.colors.len() as i64, None)
        });
        for col in &g.colors {
            q.step(c, 1, |c| {
                c.real(
                    "gradient.colors.unknown_double",
                    463,
                    col.unknown_double,
                    None,
                );
            });
            q.step(c, 2, |c| {
                c.dwg_only("gradient.colors.unknown_short", col.unknown_short);
                if let Some(v) = c.take(63) {
                    c.not_decoded(
                        "63 (the ACI the export derives from the RGB)",
                        v.to_string(),
                    );
                }
                if col.rgb >> 24 == 0xC2 {
                    c.int(
                        "gradient.colors.rgb (CMC word 0xC2, payload)",
                        421,
                        i64::from(col.rgb & 0x00FF_FFFF),
                        None,
                    );
                } else {
                    c.dwg_only("gradient.colors.rgb (not a true colour)", col.rgb);
                }
            });
        }
        q.step(c, 1, |c| c.text("gradient.name", 470, &g.name, None));
    }
}

// §20.4.84: the plot settings under AcDbPlotSettings, then LAYOUT's own under
// AcDbLayout.
fn score_layout(c: &mut Ctx<'_>, l: &dwg::objects::acad_layout::AcadLayout) {
    let p = &l.plot_settings;
    if c.subclass("AcDbPlotSettings") {
        c.text("page_setup_name", 1, &p.page_setup_name, None);
        c.text("printer_config_name", 2, &p.printer_config_name, None);
        c.int(
            "plot_layout_flags",
            70,
            i64::from(p.plot_layout_flags),
            None,
        );
        c.real("margin_left", 40, p.margin_left, None);
        c.real("margin_bottom", 41, p.margin_bottom, None);
        c.real("margin_right", 42, p.margin_right, None);
        c.real("margin_top", 43, p.margin_top, None);
        c.real("paper_width", 44, p.paper_width, None);
        c.real("paper_height", 45, p.paper_height, None);
        c.text("paper_size", 4, &p.paper_size, None);
        c.real("plot_origin.x", 46, p.plot_origin.x, None);
        c.real("plot_origin.y", 47, p.plot_origin.y, None);
        c.int("paper_units", 72, i64::from(p.paper_units), None);
        c.int("plot_rotation", 73, i64::from(p.plot_rotation), None);
        c.int("plot_type", 74, i64::from(p.plot_type), None);
        c.real("window_min.x", 48, p.window_min.x, None);
        c.real("window_min.y", 49, p.window_min.y, None);
        c.real("window_max.x", 140, p.window_max.x, None);
        c.real("window_max.y", 141, p.window_max.y, None);
        c.real("real_world_units", 142, p.real_world_units, None);
        c.real("drawing_units", 143, p.drawing_units, None);
        c.text("current_style_sheet", 7, &p.current_style_sheet, None);
        c.int(
            "standard_scale_type",
            75,
            i64::from(p.standard_scale_type),
            None,
        );
        c.real("scale_factor", 147, p.scale_factor, None);
        c.real("paper_image_origin.x", 148, p.paper_image_origin.x, None);
        c.real("paper_image_origin.y", 149, p.paper_image_origin.y, None);
        c.int("shade_plot_mode", 76, i64::from(p.shade_plot_mode), None);
        c.int(
            "shade_plot_resolution_level",
            77,
            i64::from(p.shade_plot_resolution_level),
            None,
        );
        c.int(
            "shade_plot_custom_dpi",
            78,
            i64::from(p.shade_plot_custom_dpi),
            None,
        );
    }
    if c.subclass("AcDbLayout") {
        c.text("layout_name", 1, &l.layout_name, None);
        c.int("flags", 70, i64::from(l.flags), None);
        c.int("tab_order", 71, i64::from(l.tab_order), None);
        c.real("limmin.x", 10, l.limmin.x, None);
        c.real("limmin.y", 20, l.limmin.y, None);
        c.real("limmax.x", 11, l.limmax.x, None);
        c.real("limmax.y", 21, l.limmax.y, None);
        let ip = &l.insertion_point;
        c.real("insertion_point.x", 12, ip.x, None);
        c.real("insertion_point.y", 22, ip.y, None);
        c.real("insertion_point.z", 32, ip.z, None);
        let (mn, mx) = (&l.extent_min, &l.extent_max);
        c.real("extent_min.x", 14, mn.x, None);
        c.real("extent_min.y", 24, mn.y, None);
        c.real("extent_min.z", 34, mn.z, None);
        c.real("extent_max.x", 15, mx.x, None);
        c.real("extent_max.y", 25, mx.y, None);
        c.real("extent_max.z", 35, mx.z, None);
        c.real("elevation", 146, l.elevation, None);
        let o = &l.ucs_origin;
        c.real("ucs_origin.x", 13, o.x, None);
        c.real("ucs_origin.y", 23, o.y, None);
        c.real("ucs_origin.z", 33, o.z, None);
        let x = &l.ucs_x_axis;
        c.real("ucs_x_axis.x", 16, x.x, None);
        c.real("ucs_x_axis.y", 26, x.y, None);
        c.real("ucs_x_axis.z", 36, x.z, None);
        let y = &l.ucs_y_axis;
        c.real("ucs_y_axis.x", 17, y.x, None);
        c.real("ucs_y_axis.y", 27, y.y, None);
        c.real("ucs_y_axis.z", 37, y.z, None);
        c.int(
            "ucs_ortho_view_type",
            76,
            i64::from(l.ucs_ortho_view_type),
            None,
        );
        c.dwg_only("viewport_count", l.viewport_count);
    }
    c.whole();
}

// §20.4.40 against the DXF Reference's AcDbSpline: 70 the flags (closed 1,
// periodic 2, rational 4, planar 8 and others the decoder does not
// surface), 71 the degree, 72, 73 and 74 the knot, control point and fit
// point counts, 42, 43 and 44 the knot, control point and fit tolerances,
// 12 and 13 the fit tangents, then 40 the knots, 41 the weights of a
// rational spline, 10 the control points and 11 the fit points. The
// control form has no fit points (74 is 0); only it occurs in a file with
// a DXF twin. The scenario, the R2013+ flags and the knot parameter have
// no DXF group.
fn score_spline(c: &mut Ctx<'_>, s: &dwg::entities::spline::Spline) {
    if !c.subclass("AcDbSpline") {
        c.dwg_only("(the export has no AcDbSpline subclass)", ());
        return;
    }
    c.int("degree", 71, i64::from(s.degree), None);
    if let Some(k) = &s.control {
        c.bit("closed", 70, 1, k.closed);
        c.bit("periodic", 70, 2, k.periodic);
        c.bit("rational", 70, 4, k.rational);
        if let Some(x) = c.peek(70).and_then(DxfValue::as_i64) {
            if x & !7 != 0 {
                c.not_decoded(
                    "70 (bits other than closed, periodic and rational)",
                    (x & !7).to_string(),
                );
            }
        }
        c.done(70);
        c.int("knot count", 72, k.knots.len() as i64, None);
        c.int(
            "control point count",
            73,
            k.control_points.len() as i64,
            None,
        );
        c.real("knot_tolerance", 42, k.knot_tolerance, None);
        c.real("control_tolerance", 43, k.control_tolerance, None);
        for x in &k.knots {
            c.real("knots", 40, *x, None);
        }
        if k.rational {
            for w in &k.weights {
                c.real("weights", 41, *w, None);
            }
        }
        for p in &k.control_points {
            c.point3("control_points", 10, *p, None);
        }
    }
    match &s.fit {
        Some(f) => {
            c.int("fit point count", 74, f.fit_points.len() as i64, None);
            c.real("fit tolerance", 44, f.tolerance, None);
            c.point3("begin_tangent", 12, f.begin_tangent, None);
            c.point3("end_tangent", 13, f.end_tangent, None);
            for p in &f.fit_points {
                c.point3("fit_points", 11, *p, None);
            }
        }
        None => c.int("fit point count", 74, 0, None),
    }
    c.dwg_only("scenario", s.scenario);
    if let Some(f) = s.flag1 {
        c.dwg_only("flag1 (R2013+)", f);
    }
    if let Some(k) = s.knot_param {
        c.dwg_only("knot_param (R2013+)", k);
    }
}

// §20.4.85: 90 count, 70 flag, 43 constant width, 38 elevation, 39
// thickness, 210 normal; per vertex 10/20, then 40/41 widths, 42 bulge and
// 91 vertex id when present. Vertex groups are read vertex by vertex: a
// vertex runs from its 10 to the next 10.
fn score_lwpolyline(c: &mut Ctx<'_>, p: &dwg::entities::lwpolyline::LwPolyline) {
    c.subclass("AcDbPolyline");
    c.int("vertex count", 90, p.vertices.len() as i64, None);
    // The DWG flag word is mostly presence bits (§20.4.85); its 0x200 is the
    // closed flag, DXF 70 bit 1 (measured: 0x200 and 0x210 export as 1,
    // 0x30 as 0).
    c.bit("closed (flag & 0x200)", 70, 1, p.closed);
    if let Some(x) = c.peek(70).and_then(DxfValue::as_i64) {
        if x & !1 != 0 {
            c.not_decoded("70 (bits other than closed)", (x & !1).to_string());
        }
    }
    c.done(70);
    c.dwg_only("flag (presence bits)", p.flag);
    match p.constant_width {
        Some(w) => c.real("constant_width", 43, w, Some(0.0)),
        None => c.real("constant_width (absent: 0)", 43, 0.0, Some(0.0)),
    }
    c.real("elevation", 38, p.elevation.unwrap_or(0.0), Some(0.0));
    c.real("thickness", 39, p.thickness.unwrap_or(0.0), Some(0.0));
    c.point3("extrusion", 210, p.extrusion.unwrap_or(Z), Some(Z));
    let rec = c.record();
    let (s, e) = c.scope;
    let starts: Vec<usize> = (s..e).filter(|&i| rec.groups[i].code == 10).collect();
    for (i, v) in p.vertices.iter().enumerate() {
        let Some(&start) = starts.get(i) else {
            c.dwg_only(&format!("vertices[{i}]"), (v.x, v.y));
            continue;
        };
        let end = starts.get(i + 1).copied().unwrap_or(e);
        c.scope = (start, end);
        c.real(&format!("vertices[{i}].x"), 10, v.x, None);
        c.real(&format!("vertices[{i}].y"), 20, v.y, None);
        if let Some(&(sw, ew)) = p.widths.get(i) {
            c.real(&format!("widths[{i}].start"), 40, sw, Some(0.0));
            c.real(&format!("widths[{i}].end"), 41, ew, Some(0.0));
        }
        if let Some(&b) = p.bulges.get(i) {
            c.real(&format!("bulges[{i}]"), 42, b, Some(0.0));
        }
        if let Some(&id) = p.vertex_ids.get(i) {
            c.int(&format!("vertex_ids[{i}]"), 91, i64::from(id), None);
        }
    }
    c.scope = (s, e);
}

/// The DXF 90-range word a MULTILEADER colour (`CMC`) exports as: the
/// 32-bit true-colour word as a signed integer. Before R2004 the DWG holds
/// the bare index, which the export writes as the matching word (see
/// [`color_word`]).
fn cmc_word(c: &dwg::entities::mleader::Cmc) -> i64 {
    let word = if c.rgb != 0 {
        c.rgb
    } else {
        match c.index {
            0 => 0xC100_0000,
            256 => 0xC000_0000,
            257 => 0xC800_0000,
            n => 0xC300_0000 | u32::from(n as u16),
        }
    };
    i64::from(word as i32)
}

/// Where the parts of an `AcDbMLeader` subclass sit among its groups. The
/// export nests the embedded context in `300 CONTEXT_DATA{` ... `301 }`,
/// each leader root in `302 LEADER{` ... `303 }` and each leader line in
/// `304 LEADER_LINE{` ... `305 }`; the context's own fields come before
/// the first root and after the last, a root's before its first line and
/// after its last.
struct MLeaderLayout {
    head: (usize, usize),
    context_before: (usize, usize),
    roots: Vec<MLeaderRootLayout>,
    context_after: (usize, usize),
    tail: (usize, usize),
    markers: Vec<usize>,
}

struct MLeaderRootLayout {
    before: (usize, usize),
    lines: Vec<(usize, usize)>,
    after: (usize, usize),
}

fn marker_at(g: &DxfGroup, code: u16, text: &str) -> bool {
    g.code == code && g.value.as_str() == Some(text)
}

fn mleader_layout(groups: &[DxfGroup], (s, e): (usize, usize)) -> Option<MLeaderLayout> {
    let open = (s..e).find(|&i| marker_at(&groups[i], 300, "CONTEXT_DATA{"))?;
    let mut markers = vec![open];
    let mut roots: Vec<MLeaderRootLayout> = Vec::new();
    let mut ctx_end = None;
    let mut i = open + 1;
    while i < e {
        let g = &groups[i];
        if marker_at(g, 302, "LEADER{") {
            markers.push(i);
            let mut root = MLeaderRootLayout {
                before: (i + 1, i + 1),
                lines: Vec::new(),
                after: (i + 1, i + 1),
            };
            let mut j = i + 1;
            let mut last_end = i + 1;
            while j < e && !marker_at(&groups[j], 303, "}") {
                if marker_at(&groups[j], 304, "LEADER_LINE{") {
                    markers.push(j);
                    if root.lines.is_empty() {
                        root.before = (i + 1, j);
                    }
                    let start = j + 1;
                    while j < e && !marker_at(&groups[j], 305, "}") {
                        j += 1;
                    }
                    if j >= e {
                        return None;
                    }
                    markers.push(j);
                    root.lines.push((start, j));
                    last_end = j + 1;
                }
                j += 1;
            }
            if j >= e {
                return None;
            }
            if root.lines.is_empty() {
                root.before = (i + 1, j);
            }
            root.after = (last_end, j);
            markers.push(j);
            roots.push(root);
            i = j;
        } else if marker_at(g, 301, "}") {
            markers.push(i);
            ctx_end = Some(i);
            break;
        }
        i += 1;
    }
    let ctx_end = ctx_end?;
    let first_root = roots.first().map_or(ctx_end, |r| r.before.0 - 1);
    // Without roots the context's interior is one range, for both lookups.
    let after_roots = roots.last().map_or(open + 1, |r| r.after.1 + 1);
    Some(MLeaderLayout {
        head: (s, open),
        context_before: (open + 1, first_root),
        roots,
        context_after: (after_roots, ctx_end),
        tail: (ctx_end + 1, e),
        markers,
    })
}

// §20.4.48 with §20.4.86's embedded MLeaderAnnotContext, against the DXF
// Reference's AcDbMLeader groups, which the export nests: the context in
// 300 CONTEXT_DATA{ .. 301 }, each leader root in 302 LEADER{ .. 303 } and
// each leader line in 304 LEADER_LINE{ .. 305 }. Angles are radians on both
// sides; colours are the CMC's 32-bit word (before R2004 the DWG holds the
// bare index and the export writes the matching word). The handles
// (340-344) are not decoded. The break points of a leader line are read and
// dropped by the decoder.
fn score_mleader(c: &mut Ctx<'_>, m: &dwg::entities::mleader::MLeader) {
    if !c.subclass("AcDbMLeader") {
        c.dwg_only("(the export has no AcDbMLeader subclass)", ());
        return;
    }
    let Some(lay) = mleader_layout(&c.record().groups, c.scope) else {
        c.dwg_only("(the export has no complete CONTEXT_DATA block)", ());
        return;
    };
    for &i in &lay.markers {
        c.take_at(i);
    }
    let ctx = &m.context;
    c.scope = lay.head;
    if m.legacy.is_none() {
        c.int("class_version", 270, i64::from(m.class_version), None);
    }

    c.scope = lay.context_before;
    c.real("context.overall_scale", 40, ctx.overall_scale, None);
    c.point3(
        "context.content_base_point",
        10,
        ctx.content_base_point,
        None,
    );
    c.real("context.text_height", 41, ctx.text_height, None);
    c.real("context.arrow_head_size", 140, ctx.arrow_head_size, None);
    c.real("context.landing_gap", 145, ctx.landing_gap, None);
    c.int(
        "context.left_attachment",
        174,
        i64::from(ctx.left_attachment),
        None,
    );
    c.int(
        "context.right_attachment",
        175,
        i64::from(ctx.right_attachment),
        None,
    );
    c.int(
        "context.text_align_type",
        176,
        i64::from(ctx.text_align_type),
        None,
    );
    c.int(
        "context.attachment_type",
        177,
        i64::from(ctx.attachment_type),
        None,
    );
    c.int("context.has_text", 290, i64::from(ctx.text.is_some()), None);
    if let Some(t) = &ctx.text {
        c.text("text.label", 304, &t.label, None);
        c.point3("text.normal", 11, t.normal, None);
        c.point3("text.location", 12, t.location, None);
        c.point3("text.direction", 13, t.direction, None);
        c.real("text.rotation", 42, t.rotation, None);
        c.real("text.boundary_width", 43, t.boundary_width, None);
        c.real("text.boundary_height", 44, t.boundary_height, None);
        c.real("text.line_spacing_factor", 45, t.line_spacing_factor, None);
        c.int(
            "text.line_spacing_style",
            170,
            i64::from(t.line_spacing_style),
            None,
        );
        c.int("text.color", 90, cmc_word(&t.color), None);
        c.int("text.alignment", 171, i64::from(t.alignment), None);
        c.int(
            "text.flow_direction",
            172,
            i64::from(t.flow_direction),
            None,
        );
        c.int(
            "text.background_color",
            91,
            cmc_word(&t.background_color),
            None,
        );
        c.real("text.background_scale", 141, t.background_scale, None);
        c.int(
            "text.background_transparency",
            92,
            i64::from(t.background_transparency),
            None,
        );
        c.int(
            "text.background_fill_enabled",
            291,
            i64::from(t.background_fill_enabled),
            None,
        );
        c.int(
            "text.background_mask_fill_on",
            292,
            i64::from(t.background_mask_fill_on),
            None,
        );
        c.int("text.column_type", 173, i64::from(t.column_type), None);
        c.int(
            "text.text_height_automatic",
            293,
            i64::from(t.text_height_automatic),
            None,
        );
        c.real("text.column_width", 142, t.column_width, None);
        c.real("text.column_gutter", 143, t.column_gutter, None);
        c.int(
            "text.column_flow_reversed",
            294,
            i64::from(t.column_flow_reversed),
            None,
        );
        for (i, w) in t.column_sizes.iter().enumerate() {
            c.real(&format!("text.column_sizes[{i}]"), 144, *w, None);
        }
        c.int("text.word_break", 295, i64::from(t.word_break), None);
        c.dwg_only("text.unknown_flag", t.unknown_flag);
    }
    // §20.4.86 reads the has-block bit only when there is no text, so on a
    // text multileader the DWG does not hold the export's 296.
    if ctx.text.is_some() {
        if let Some(v) = c.take(296) {
            c.not_decoded(
                "296 (has block; the DWG stores it only without text)",
                v.to_string(),
            );
        }
    } else {
        c.int(
            "context.has_block",
            296,
            i64::from(ctx.block.is_some()),
            None,
        );
    }
    if let Some(b) = &ctx.block {
        c.point3("block.normal", 14, b.normal, None);
        c.point3("block.location", 15, b.location, None);
        c.point3("block.scale", 16, b.scale, None);
        c.real("block.rotation", 46, b.rotation, None);
        c.int("block.color", 93, cmc_word(&b.color), None);
        for (i, v) in b.transform.iter().enumerate() {
            c.real(&format!("block.transform[{i}]"), 47, *v, None);
        }
    }
    c.point3("context.base_point", 110, ctx.base_point, None);
    c.point3("context.base_direction", 111, ctx.base_direction, None);
    c.point3("context.base_vertical", 112, ctx.base_vertical, None);
    c.int(
        "context.normal_reversed",
        297,
        i64::from(ctx.normal_reversed),
        None,
    );

    c.derived(
        "context.leader_root_count (302 LEADER{ blocks)",
        "302",
        ctx.leader_roots.len() == lay.roots.len(),
        ctx.leader_roots.len().to_string(),
        lay.roots.len().to_string(),
    );
    for (k, (root, rl)) in ctx.leader_roots.iter().zip(&lay.roots).enumerate() {
        let n = format!("roots[{k}]");
        c.scope = rl.before;
        c.int(
            &format!("{n}.content_valid"),
            290,
            i64::from(root.content_valid),
            None,
        );
        c.int(
            &format!("{n}.unknown_flag"),
            291,
            i64::from(root.unknown_flag),
            None,
        );
        c.point3(
            &format!("{n}.connection_point"),
            10,
            root.connection_point,
            None,
        );
        c.point3(&format!("{n}.direction"), 11, root.direction, None);
        for (j, (a, b)) in root.break_points.iter().enumerate() {
            c.point3(&format!("{n}.break_points[{j}].start"), 12, *a, None);
            c.point3(&format!("{n}.break_points[{j}].end"), 13, *b, None);
        }
        c.int(&format!("{n}.index"), 90, i64::from(root.index), None);
        c.real(
            &format!("{n}.landing_distance"),
            40,
            root.landing_distance,
            None,
        );
        c.derived(
            &format!("{n}.line_count (304 LEADER_LINE{{ blocks)"),
            "304",
            root.lines.len() == rl.lines.len(),
            root.lines.len().to_string(),
            rl.lines.len().to_string(),
        );
        for (l, (line, range)) in root.lines.iter().zip(&rl.lines).enumerate() {
            let n = format!("{n}.lines[{l}]");
            c.scope = *range;
            for (p, pt) in line.points.iter().enumerate() {
                c.point3(&format!("{n}.points[{p}]"), 10, *pt, None);
            }
            c.int(&format!("{n}.index"), 91, i64::from(line.index), None);
            if m.legacy.is_none() {
                c.int(
                    &format!("{n}.leader_type"),
                    170,
                    i64::from(line.leader_type),
                    None,
                );
                c.int(&format!("{n}.color"), 92, cmc_word(&line.color), None);
                c.int(
                    &format!("{n}.line_weight"),
                    171,
                    i64::from(line.line_weight),
                    None,
                );
                c.real(&format!("{n}.arrow_size"), 40, line.arrow_size, None);
                c.int(
                    &format!("{n}.override_flags"),
                    93,
                    i64::from(line.override_flags),
                    None,
                );
            }
        }
        if m.legacy.is_none() {
            c.scope = rl.after;
            c.int(
                &format!("{n}.attachment_direction"),
                271,
                i64::from(root.attachment_direction),
                None,
            );
        }
    }

    if m.legacy.is_none() {
        c.scope = lay.context_after;
        c.int(
            "context.bottom_attachment",
            272,
            i64::from(ctx.bottom_attachment),
            None,
        );
        c.int(
            "context.top_attachment",
            273,
            i64::from(ctx.top_attachment),
            None,
        );
    }

    c.scope = lay.tail;
    c.int("override_flags", 90, i64::from(m.override_flags), None);
    c.int("leader_type", 170, i64::from(m.leader_type), None);
    c.int("leader_color", 91, cmc_word(&m.leader_color), None);
    c.int("line_weight", 171, i64::from(m.line_weight), None);
    c.int("landing_enabled", 290, i64::from(m.landing_enabled), None);
    c.int("dogleg_enabled", 291, i64::from(m.dogleg_enabled), None);
    c.real("landing_distance", 41, m.landing_distance, None);
    c.real("arrow_head_size", 42, m.arrow_head_size, None);
    c.int("content_type", 172, i64::from(m.content_type), None);
    c.int("left_attachment", 173, i64::from(m.left_attachment), None);
    c.int("right_attachment", 95, i64::from(m.right_attachment), None);
    c.int("text_angle_type", 174, i64::from(m.text_angle_type), None);
    c.int("unknown_175", 175, i64::from(m.unknown_175), None);
    c.int("text_color", 92, cmc_word(&m.text_color), None);
    c.int(
        "text_frame_enabled",
        292,
        i64::from(m.text_frame_enabled),
        None,
    );
    c.int("block_color", 93, cmc_word(&m.block_color), None);
    c.point3("block_scale", 10, m.block_scale, None);
    c.real("block_rotation", 43, m.block_rotation, None);
    c.int(
        "block_attachment_type",
        176,
        i64::from(m.block_attachment_type),
        None,
    );
    c.int("is_annotative", 293, i64::from(m.is_annotative), None);
    if let Some(l) = &m.legacy {
        c.int(
            "legacy.text_direction_negative",
            294,
            i64::from(l.text_direction_negative),
            None,
        );
        c.int("legacy.ipe_align", 178, i64::from(l.ipe_align), None);
        c.int(
            "legacy.justification",
            179,
            i64::from(l.justification),
            None,
        );
        c.real("legacy.scale_factor", 45, l.scale_factor, None);
        c.dwg_only("legacy.arrow_heads_default", &l.arrow_heads_default);
        c.dwg_only("legacy.block_labels", &l.block_labels);
    } else {
        c.int(
            "attachment_direction",
            271,
            i64::from(m.attachment_direction),
            None,
        );
        c.int(
            "bottom_attachment",
            272,
            i64::from(m.bottom_attachment),
            None,
        );
        c.int("top_attachment", 273, i64::from(m.top_attachment), None);
        c.dwg_only("undocumented_mc", m.undocumented_mc);
        c.dwg_only("undocumented_flag", m.undocumented_flag);
        // R2013+ only: the DWG and the export both lack it on R2010.
        if c.peek(295).is_some() {
            c.int(
                "leader_extended_to_text",
                295,
                i64::from(m.leader_extended_to_text),
                None,
            );
        }
    }
}

use dwg::objects::acad_visual_style::{AcadVisualStyle, VisualStyleColor, VisualStyleValue};

/// The DXF colour number of a VISUALSTYLE colour, `None` for an RGB one
/// (whose 62/63-67 the export derives; the RGB goes in 420/421).
fn vs_aci(c: &VisualStyleColor) -> Option<i64> {
    match c.method() {
        0xC0 => Some(256),
        0xC1 => Some(0),
        0xC3 => Some(i64::from(c.payload())),
        0xC8 => Some(257),
        0xC2 => None,
        // R14 and R2000 write the bare index (§2.11).
        _ => Some(i64::from(c.index)),
    }
}

/// VISUALSTYLE, which v5.4.1 does not prescribe, against the DXF
/// Reference's AcDbVisualStyle groups. Up to R2010 every property has its
/// own group (71 face lighting model ... 173 shadow type), and from R2010
/// each is followed by its 176 flag; R2013 and R2018 write the 58
/// properties as a counted list (70 = 58) of value groups and 176 flags,
/// compared by position. The decoder names its slots after the
/// conventional property order and says only the face group is
/// corroborated; the named groups test every name.
fn score_visualstyle(c: &mut Ctx<'_>, v: &AcadVisualStyle) {
    c.subclass("AcDbVisualStyle");
    c.text("description", 2, &v.description, None);
    c.int(
        "internal_style_type",
        70,
        i64::from(v.internal_style_type),
        None,
    );
    if v.format_version != 0 {
        c.int("format_version", 177, i64::from(v.format_version), None);
    }
    c.int(
        "is_internal_use_only",
        291,
        i64::from(v.is_internal_use_only),
        Some(0),
    );
    use VisualStyleValue::{Bool, Color, Double, Short};
    let named: [(&str, u16, VisualStyleValue); 28] = [
        ("face_lighting_model", 71, Short(v.face_lighting_model)),
        ("face_lighting_quality", 72, Short(v.face_lighting_quality)),
        ("face_color_mode", 73, Short(v.face_color_mode)),
        ("face_modifier", 90, Short(v.face_modifier)),
        ("face_opacity", 40, Double(v.face_opacity)),
        ("face_specular", 41, Double(v.face_specular)),
        ("face_mono_color", 63, Color(v.face_mono_color)),
        ("edge_model", 74, Short(v.edge_model)),
        ("edge_style", 91, Short(v.edge_style)),
        (
            "edge_intersection_color",
            64,
            Color(v.edge_intersection_color),
        ),
        ("edge_obscured_color", 65, Color(v.edge_obscured_color)),
        (
            "edge_obscured_linetype",
            75,
            Short(v.edge_obscured_linetype),
        ),
        (
            "edge_intersection_linetype",
            175,
            Short(v.edge_intersection_linetype),
        ),
        ("edge_crease_angle", 42, Double(v.edge_crease_angle)),
        ("edge_modifier", 92, Short(v.edge_modifier)),
        ("edge_color", 66, Color(v.edge_color)),
        ("edge_opacity", 43, Double(v.edge_opacity)),
        ("edge_width", 76, Short(v.edge_width)),
        ("edge_overhang", 77, Short(v.edge_overhang)),
        ("edge_jitter", 78, Short(v.edge_jitter)),
        ("edge_silhouette_color", 67, Color(v.edge_silhouette_color)),
        ("edge_silhouette_width", 79, Short(v.edge_silhouette_width)),
        ("edge_halo_gap", 170, Short(v.edge_halo_gap)),
        ("edge_isoline_count", 171, Short(v.edge_isoline_count)),
        ("edge_hide_precision", 290, Bool(v.edge_hide_precision)),
        ("edge_style_apply", 93, Short(v.edge_style_apply)),
        ("display_brightness", 44, Double(v.display_brightness)),
        ("display_shadow_type", 173, Short(v.display_shadow_type)),
    ];
    let counted = c.peek(70).and_then(DxfValue::as_i64);
    match counted {
        Some(n) => {
            c.take(70);
            let mut all: Vec<(String, VisualStyleValue, Option<i16>)> = named
                .iter()
                .enumerate()
                .map(|(i, (name, _, val))| {
                    (
                        name.to_string(),
                        val.clone(),
                        v.property_flags.get(i).copied(),
                    )
                })
                .collect();
            for (i, p) in v.extended.iter().enumerate() {
                all.push((format!("extended[{i}]"), p.value.clone(), Some(p.flag)));
            }
            c.derived(
                "property count",
                "70",
                n == all.len() as i64,
                all.len().to_string(),
                n.to_string(),
            );
            score_property_list(c, &all);
        }
        None => {
            for (i, (name, code, val)) in named.iter().enumerate() {
                score_vs_value(c, name, *code, val);
                if let Some(flag) = v.property_flags.get(i) {
                    c.int(&format!("{name} flag"), 176, i64::from(*flag), None);
                }
            }
            c.dwg_only("edge_unknown_byte", v.edge_unknown_byte);
            if v.display_unknown_short != 0 {
                c.dwg_only("display_unknown_short", v.display_unknown_short);
            }
        }
    }
}

/// One named VISUALSTYLE property in its own group.
fn score_vs_value(c: &mut Ctx<'_>, name: &str, code: u16, val: &VisualStyleValue) {
    match val {
        VisualStyleValue::Short(x) => c.int(name, code, i64::from(*x), None),
        VisualStyleValue::Double(x) => c.real(name, code, *x, None),
        VisualStyleValue::Bool(x) => c.int(name, code, i64::from(*x), None),
        VisualStyleValue::Text(x) => c.text(name, code, x, None),
        VisualStyleValue::Color(col) => {
            let rgb_code = if code == 63 { 421 } else { 420 };
            match vs_aci(col) {
                Some(aci) => c.int(name, code, aci, None),
                None => {
                    c.take(code);
                    c.int(
                        &format!("{name} (RGB)"),
                        rgb_code,
                        i64::from(col.payload()),
                        None,
                    );
                }
            }
            if vs_aci(col).is_some() && c.peek(rgb_code).is_some() {
                c.take(rgb_code);
            }
        }
    }
}

/// The R2013+ counted property list: each value group (62 and an optional
/// 420 for a colour) followed by its 176 flag, compared by position.
fn score_property_list(c: &mut Ctx<'_>, all: &[(String, VisualStyleValue, Option<i16>)]) {
    let free = c.free();
    let mut k = 0usize;
    for (name, val, flag) in all {
        let Some(&(i, code, x)) = free.get(k) else {
            c.dwg_only(name, ());
            continue;
        };
        k += 1;
        c.take_at(i);
        let p = c.precision();
        let (agree, dwg, dxf) = match (val, code) {
            (VisualStyleValue::Short(a), 90) => (
                x.as_i64() == Some(i64::from(*a)),
                a.to_string(),
                x.to_string(),
            ),
            (VisualStyleValue::Double(a), 40) => (
                x.as_f64().is_some_and(|b| reals_agree(*a, b, p)),
                format!("{a:?}"),
                x.to_string(),
            ),
            (VisualStyleValue::Bool(a), 290) => (
                x.as_i64() == Some(i64::from(*a)),
                a.to_string(),
                x.to_string(),
            ),
            (VisualStyleValue::Text(a), 1) => (
                x.as_str().map(caret_decode).as_deref() == Some(a.as_str()),
                format!("{a:?}"),
                x.to_string(),
            ),
            (VisualStyleValue::Color(col), 62) => {
                let rgb = match free.get(k) {
                    Some(&(j, 420, r)) => {
                        k += 1;
                        c.take_at(j);
                        r.as_i64()
                    }
                    _ => None,
                };
                match (vs_aci(col), rgb) {
                    (Some(aci), _) => (x.as_i64() == Some(aci), aci.to_string(), x.to_string()),
                    (None, Some(r)) => (
                        r == i64::from(col.payload()),
                        format!("{:#08X}", col.payload()),
                        format!("{r:#08X}"),
                    ),
                    (None, None) => (false, format!("{:#08X}", col.payload()), x.to_string()),
                }
            }
            _ => (false, format!("{val:?}"), format!("{code}={x}")),
        };
        c.derived(name, &code.to_string(), agree, dwg, dxf);
        if let Some(f) = flag {
            match free.get(k) {
                Some(&(j, 176, fx)) => {
                    k += 1;
                    c.take_at(j);
                    c.derived(
                        &format!("{name} flag"),
                        "176",
                        fx.as_i64() == Some(i64::from(*f)),
                        f.to_string(),
                        fx.to_string(),
                    );
                }
                _ => c.dwg_only(&format!("{name} flag"), f),
            }
        }
    }
}

// §20.4.80 IMAGE, and WIPEOUT, which shares its record, against the DXF
// Reference's AcDbRasterImage and AcDbWipeout groups: 90 the class version
// (not decoded), 10 the insertion point, 11 and 12 the u and v vectors, 13
// the image size in pixels, 70 the display flags, 280 the clipping state,
// 281 to 283 brightness, contrast and fade, 290 the clip mode (R2010+), 71
// the clip boundary type and 91 the clip vertex count with the vertices in
// 14. The export closes a polygon boundary by repeating its first vertex,
// so the count is the decoded vertices plus one and the extra vertex is
// compared with the first; a rectangle's two corners are written as they
// are. The image definition (340) and its reactor (360) are handles.
fn score_image(c: &mut Ctx<'_>, i: &dwg::entities::image::Image, subclass: &str) {
    use dwg::entities::image::ClipBoundary;
    if !c.subclass(subclass) {
        c.dwg_only(&format!("(the export has no {subclass} subclass)"), ());
        return;
    }
    c.point3("insertion_point", 10, i.insertion_point, None);
    c.point3("u_vector", 11, i.u_vector, None);
    c.point3("v_vector", 12, i.v_vector, None);
    c.point2("image_size", 13, i.image_size);
    c.int("display_flags", 70, i64::from(i.display_flags), None);
    c.int("clipping", 280, i64::from(i.clipping), None);
    c.int("brightness", 281, i64::from(i.brightness), None);
    c.int("contrast", 282, i64::from(i.contrast), None);
    c.int("fade", 283, i64::from(i.fade), None);
    if c.peek(290).is_some() {
        c.int("clip_mode", 290, i64::from(i.clip_mode), None);
    }
    let (kind, verts) = match &i.clip_boundary {
        ClipBoundary::Rectangle {
            lower_left,
            upper_right,
        } => (1, vec![*lower_left, *upper_right]),
        ClipBoundary::Polygon(p) => (2, p.clone()),
    };
    c.int("clip_boundary_type", 71, kind, None);
    let closing = usize::from(kind == 2 && !verts.is_empty());
    let want = (verts.len() + closing) as i64;
    match c.take(91).and_then(DxfValue::as_i64) {
        Some(n) => c.derived(
            "clip vertex count (a polygon is closed by repeating its first)",
            "91",
            n == want,
            want.to_string(),
            n.to_string(),
        ),
        None => c.dwg_only("clip vertex count", want),
    }
    for (k, v) in verts.iter().enumerate() {
        c.point2(&format!("clip_boundary[{k}]"), 14, *v);
    }
    if closing == 1 {
        c.point2(
            "clip_boundary closing vertex (the first again)",
            14,
            verts[0],
        );
    }
}

// §20.4.81 against the DXF Reference's AcDbRasterImageDef: 90 the class
// version, 1 the file path, 10 the image size in pixels, 11 the default
// pixel size, 280 the loaded flag and 281 the resolution units.
fn score_imagedef(c: &mut Ctx<'_>, d: &dwg::entities::imagedef::ImageDef) {
    if !c.subclass("AcDbRasterImageDef") {
        c.dwg_only("(the export has no AcDbRasterImageDef subclass)", ());
        return;
    }
    c.int("class_version", 90, i64::from(d.class_version), None);
    c.text("file_path", 1, &d.file_path, None);
    c.real("image_size_pixels.x", 10, d.image_size_pixels.0, None);
    c.real("image_size_pixels.y", 20, d.image_size_pixels.1, None);
    c.real("pixel_width_size", 11, d.pixel_width_size, None);
    c.real("pixel_height_size", 21, d.pixel_height_size, None);
    c.int("is_loaded", 280, i64::from(d.is_loaded), None);
    c.int("pixel_size_units", 281, i64::from(d.pixel_size_units), None);
}

/// A colour as MLEADERSTYLE writes it: the CMC's 32-bit true-colour word as
/// a signed group-90-range integer (`0xC1000000` ByBlock is -1056964608).
/// Before R2004 the CMC is a bare index (§2.11), which the export writes
/// as the matching word: 0 ByBlock `0xC1000000`, 256 ByLayer `0xC0000000`,
/// 257 none `0xC8000000`, an index `0xC3000000 | n`.
fn color_word(c: &ObjectColor) -> i64 {
    let word = if c.rgb != 0 {
        c.rgb
    } else {
        match c.index {
            0 => 0xC100_0000,
            256 => 0xC000_0000,
            257 => 0xC800_0000,
            n => 0xC300_0000 | u32::from(n),
        }
    };
    i64::from(word as i32)
}

/// §20.4.87 MLEADERSTYLE: each field against the group the spec's DXF
/// column gives it (which are also the DXF Reference's AcDbMLeaderStyle
/// groups). Angles are radians on both sides.
fn score_mleaderstyle(c: &mut Ctx<'_>, m: &dwg::objects::acad_mleader_style::AcadMLeaderStyle) {
    c.subclass("AcDbMLeaderStyle");
    c.int("version", 179, i64::from(m.version), None);
    c.int("content_type", 170, i64::from(m.content_type), None);
    c.int(
        "draw_mleader_order_type",
        171,
        i64::from(m.draw_mleader_order_type),
        None,
    );
    c.int(
        "draw_leader_order_type",
        172,
        i64::from(m.draw_leader_order_type),
        None,
    );
    c.int(
        "max_leader_segment_points",
        90,
        i64::from(m.max_leader_segment_points),
        None,
    );
    c.real("first_segment_angle", 40, m.first_segment_angle, None);
    c.real("second_segment_angle", 41, m.second_segment_angle, None);
    c.int("leader_line_type", 173, i64::from(m.leader_line_type), None);
    c.int(
        "leader_line_color",
        91,
        color_word(&m.leader_line_color),
        None,
    );
    c.int(
        "leader_lineweight",
        92,
        i64::from(m.leader_lineweight),
        None,
    );
    c.int("enable_landing", 290, i64::from(m.enable_landing), None);
    c.real("landing_gap", 42, m.landing_gap, None);
    c.int("enable_dogleg", 291, i64::from(m.enable_dogleg), None);
    c.real("dogleg_length", 43, m.dogleg_length, None);
    c.text("description", 3, &m.description, None);
    c.real("arrowhead_size", 44, m.arrowhead_size, None);
    c.text(
        "default_mtext_contents",
        300,
        &m.default_mtext_contents,
        None,
    );
    c.int(
        "text_left_attachment",
        174,
        i64::from(m.text_left_attachment),
        None,
    );
    c.int(
        "text_right_attachment",
        178,
        i64::from(m.text_right_attachment),
        None,
    );
    c.int("text_angle_type", 175, i64::from(m.text_angle_type), None);
    c.int(
        "text_alignment_type",
        176,
        i64::from(m.text_alignment_type),
        None,
    );
    c.int("text_color", 93, color_word(&m.text_color), None);
    c.real("text_height", 45, m.text_height, None);
    c.int(
        "enable_frame_text",
        292,
        i64::from(m.enable_frame_text),
        None,
    );
    c.int(
        "text_align_always_left",
        297,
        i64::from(m.text_align_always_left),
        None,
    );
    c.real("align_space", 46, m.align_space, None);
    c.int(
        "block_content_color",
        94,
        color_word(&m.block_content_color),
        None,
    );
    c.real("block_content_scale_x", 47, m.block_content_scale_x, None);
    c.real("block_content_scale_y", 49, m.block_content_scale_y, None);
    c.real("block_content_scale_z", 140, m.block_content_scale_z, None);
    c.int(
        "enable_block_content_scale",
        293,
        i64::from(m.enable_block_content_scale),
        None,
    );
    c.real(
        "block_content_rotation",
        141,
        m.block_content_rotation,
        None,
    );
    c.int(
        "enable_block_content_rotation",
        294,
        i64::from(m.enable_block_content_rotation),
        None,
    );
    c.int(
        "block_content_connection_type",
        177,
        i64::from(m.block_content_connection_type),
        None,
    );
    c.real("scale", 142, m.scale, None);
    c.int(
        "overwrite_property_value",
        295,
        i64::from(m.overwrite_property_value),
        None,
    );
    c.int("is_annotative", 296, i64::from(m.is_annotative), None);
    c.real("break_gap_size", 143, m.break_gap_size, None);
    // 271 to 273 exist from R2010; the earlier exports omit them and the
    // earlier DWGs have no such fields.
    for (name, code, v) in [
        (
            "text_attachment_direction",
            271,
            m.text_attachment_direction,
        ),
        (
            "bottom_text_attachment_direction",
            272,
            m.bottom_text_attachment_direction,
        ),
        (
            "top_text_attachment_direction",
            273,
            m.top_text_attachment_direction,
        ),
    ] {
        if c.peek(code).is_some() {
            c.int(name, code, i64::from(v), None);
        } else {
            c.dwg_only(&format!("{name} (no such field before R2010)"), v);
        }
    }
    c.int("extended_flag", 298, i64::from(m.extended_flag), Some(0));
}

// §20.4.38 against the DXF Reference's AcDbViewport groups, named in the
// decoder's field list: 10, 40, 41, then the R2000+ view block. Angles are
// degrees in the DXF (51, 50) and radians in the DWG. The ambient light
// colour is a CMC whose ACI index is group 63 (421 is the RGB the export
// derives from it). The count of frozen layers is the handle stream's;
// the export lists the layers as 331 handles.
fn score_viewport(c: &mut Ctx<'_>, v: &dwg::entities::viewport::Viewport) {
    if !c.subclass("AcDbViewport") {
        c.dwg_only("(the export has no AcDbViewport subclass)", ());
        return;
    }
    c.point3("center", 10, v.center, None);
    c.real("width", 40, v.width, None);
    c.real("height", 41, v.height, None);
    let Some(w) = &v.view else {
        return;
    };
    c.point3("view_target", 17, w.view_target, None);
    c.point3("view_direction", 16, w.view_direction, None);
    c.angle("view_twist_angle", 51, w.view_twist_angle, None);
    c.real("view_height", 45, w.view_height, None);
    c.real("lens_length", 42, w.lens_length, None);
    c.real("front_clip_z", 43, w.front_clip_z, None);
    c.real("back_clip_z", 44, w.back_clip_z, None);
    c.angle("snap_angle", 50, w.snap_angle, None);
    c.point2("view_center", 12, w.view_center);
    c.point2("snap_base", 13, w.snap_base);
    c.point2("snap_spacing", 14, w.snap_spacing);
    c.point2("grid_spacing", 15, w.grid_spacing);
    c.int("circle_zoom", 72, i64::from(w.circle_zoom), None);
    if let Some(g) = w.grid_major {
        c.int("grid_major", 61, i64::from(g), None);
    }
    c.dwg_only("frozen_layer_count", w.frozen_layer_count);
    c.int("status_flags", 90, i64::from(w.status_flags), None);
    c.text("style_sheet", 1, &w.style_sheet, None);
    c.int("render_mode", 281, i64::from(w.render_mode), None);
    c.int("ucs_at_origin", 74, i64::from(w.ucs_at_origin), None);
    c.int("ucs_per_viewport", 71, i64::from(w.ucs_per_viewport), None);
    c.point3("ucs_origin", 110, w.ucs_origin, None);
    c.point3("ucs_x_axis", 111, w.ucs_x_axis, None);
    c.point3("ucs_y_axis", 112, w.ucs_y_axis, None);
    c.real("ucs_elevation", 146, w.ucs_elevation, None);
    c.int(
        "ucs_ortho_view_type",
        79,
        i64::from(w.ucs_ortho_view_type),
        None,
    );
    if let Some(s) = w.shade_plot_mode {
        c.int("shade_plot_mode", 170, i64::from(s), None);
    }
    if let Some(l) = &w.lighting {
        c.int(
            "use_default_lights",
            292,
            i64::from(l.use_default_lights),
            None,
        );
        c.int(
            "default_lighting_type",
            282,
            i64::from(l.default_lighting_type),
            None,
        );
        c.real("brightness", 141, l.brightness, None);
        c.real("contrast", 142, l.contrast, None);
        let col = &l.ambient_light_color;
        match col.rgb >> 24 {
            0xC3 => {
                c.int(
                    "ambient_light_color (ACI)",
                    63,
                    i64::from(col.rgb & 0xFF),
                    None,
                );
                if let Some(v) = c.take(421) {
                    c.not_decoded(
                        "421 (the RGB the export derives from the ACI)",
                        v.to_string(),
                    );
                }
            }
            0xC2 => {
                if let Some(v) = c.take(63) {
                    c.not_decoded(
                        "63 (the ACI the export derives from the RGB)",
                        v.to_string(),
                    );
                }
                c.int(
                    "ambient_light_color (RGB, CMC word 0xC2 payload)",
                    421,
                    i64::from(col.rgb & 0x00FF_FFFF),
                    None,
                );
            }
            _ => c.dwg_only("ambient_light_color (neither ACI nor RGB)", col.rgb),
        }
    }
}

// §20.4.60 VIEW: 40, 41, 10/20, 12/22/32 target, 11/21/31 direction, 50
// twist (degrees in the DXF), 42, 43, 44, 71 view mode (four stored bits,
// converted), 281 render mode, 70 bit 1 paper space, 72 associated UCS.
fn score_view(c: &mut Ctx<'_>, v: &dwg::tables::view::ViewEntry) {
    c.subclass("AcDbViewTableRecord");
    c.text("name", 2, &v.header.name, None);
    c.bit("is_paperspace", 70, 1, v.is_paperspace);
    flags_64_xdep(c, &v.header);
    c.done(70);
    c.real("view_height", 40, v.view_height, None);
    c.real("view_width", 41, v.view_width, None);
    c.point2("view_center", 10, v.view_center);
    c.point3("target", 12, v.target, None);
    c.point3("view_direction", 11, v.view_direction, None);
    c.angle("twist_angle", 50, v.twist_angle, None);
    c.real("lens_length", 42, v.lens_length, None);
    c.real("front_clip", 43, v.front_clip, None);
    c.real("back_clip", 44, v.back_clip, None);
    c.int("view_mode", 71, i64::from(v.view_mode), None);
    c.int("render_mode", 281, i64::from(v.render_mode), Some(0));
    c.int(
        "is_associated_ucs",
        72,
        i64::from(v.is_associated_ucs),
        Some(0),
    );
}

// §20.4.93: BL numentries, then that many sort handles, handle code 0 in the
// data stream. The DXF writes each entry as 331 (the entity, a handle of the
// handle stream the decoder does not read) then 5 (the sort handle), after
// the record's own 5; the entry count has no group of its own. All entries
// count under one field.
fn score_sortents(c: &mut Ctx<'_>, t: &dwg::objects::block_sortents_table::SortEntsTable) {
    c.subclass("AcDbSortentsTable");
    let exported = c.record().all(5).count().saturating_sub(1);
    c.derived(
        "entry count (5 groups)",
        "5",
        t.sort_handles.len() == exported,
        t.sort_handles.len().to_string(),
        exported.to_string(),
    );
    for h in &t.sort_handles {
        match c.take(5).and_then(DxfValue::as_handle) {
            Some(x) => c.derived(
                "sort_handle",
                "5",
                x == *h,
                format!("{h:X}"),
                format!("{x:X}"),
            ),
            None => c.dwg_only("sort_handle", format!("{h:X}")),
        }
    }
}
