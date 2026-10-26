//! `AcDb:Header`: the drawing's system variables (ODA spec v5.4.1 §9).
//!
//! [`HeaderVars::decode`] reads every variable of the section, in the
//! order [`crate::header_table::TABLE`] gives, for the release the file
//! is. Each comes back under its AutoCAD name (`$INSBASE`, `$EXTMIN`,
//! `$DIMTAD`, ...) as a typed [`Value`]; the slots §9 calls unknown are
//! carried too, named `UNKNOWN_<code>_<n>`.
//!
//! # The section's frame
//!
//! ```text
//! sentinel (16)  RL size  [RL 0]  [RL bits]  data ... pad  RS crc  sentinel (16)
//! ```
//!
//! - `size` counts bytes from the first byte after it to the end of the
//!   data, padding included. The CRC (`RS`, seed `0xC0C1`) covers every
//!   byte after the first sentinel up to the CRC: the size word, the
//!   words below and the data.
//! - R2007 adds the `RL` size in bits (§9 lists it as "R2007 Only"); it
//!   is also present on R2010, R2013 and R2018, on every file measured.
//! - R2010 and later put an `RL` between the size and the size in bits
//!   ("only present if the maintenance version is greater than 3", and
//!   always on R2018). It reads 0 and is outside `size`, the high word
//!   of a 64-bit size, as in `AcDb:Classes`. Which of §9's two
//!   maintenance versions the condition names is not stated, and every
//!   R2010 and R2013 drawing of the corpus carries the word with both
//!   header bytes (0x0B and 0x12) above 3, so the section's own length
//!   decides: the word is there when `20 + 4 + size + 2 + 16` is the
//!   length.
//!
//! # The streams
//!
//! Up to R2004 the variables are one bit stream: text is inline and so
//! are handles. R2007 and later split it as an object is (§19.1): the
//! data stream, then the string stream and its trailer, then the handle
//! stream, with `HANDSEED` the one handle that stays in the data stream.
//!
//! # What the files say that §9 does not
//!
//! `ARCHITECTURE.md` §7n has the measurements. In short: `MENUNAME` is read
//! on every release; four `BS` slots end the section on every release from
//! R14 (in the handle stream from R2007) and five `B` slots follow them from
//! R2004; the padding is all ones on every AutoCAD 2016 to 2025 file; R2000
//! and later store `TDCREATE` and `TDUPDATE` as universal time, so they are
//! named `$TDUCREATE` and `$TDUUPDATE`; and a `CMC` from R2004 carries its
//! meaning in the top byte of its `BL`.
//!
//! # How it fails
//!
//! Every length is checked against the bytes before it is used, and every
//! failure is an [`Error`]: a section shorter than its frame, a size that
//! does not fit, a sentinel or CRC that differs, a bit code that runs past
//! the data, a text that claims more characters than the stream holds. A
//! decode that reads every slot but does not stop on the last byte of the
//! data is an error in [`HeaderVars::decode`] and a recorded
//! [`HeaderVars::diagnostics`] entry in [`HeaderVars::parse_lossy`].

use std::collections::HashMap;

use crate::bitcursor::{BitCursor, Handle};
use crate::crc::crc8;
use crate::error::{Error, Result};
use crate::header_table::{self, Kind, R14, R2000, R2004, R2007, R2013, R2018, Rel, Var};
use crate::limits::ParseLimits;
use crate::version::Version;

/// A `CMC` colour (§2.11).
#[derive(Debug, Clone, PartialEq)]
pub struct Color {
    /// The colour index (`BS`).
    pub index: i16,
    /// From R2004, the `BL` RGB word.
    pub rgb: Option<u32>,
    /// From R2004, the `RC`: bit 1 announces a colour name, bit 2 a book
    /// name.
    pub flags: u8,
    /// The colour name, when the flags announce one.
    pub name: Option<String>,
    /// The book name, when the flags announce one.
    pub book: Option<String>,
}

impl Color {
    /// The AutoCAD colour index DXF writes for this colour: group 62 or
    /// the `$DIMCLR*` integer. Up to R2000 it is the `BS`. From R2004 the
    /// `BS` is always 0 and the colour is the top byte of the `BL`:
    /// `0xC0` is BYLAYER (256), `0xC1` BYBLOCK (0) and `0xC3` an indexed
    /// colour in the low byte, as measured on the public corpus. `0xC2`
    /// is a true colour with no index, and an unknown top byte gives
    /// `None`.
    pub fn aci(&self) -> Option<i16> {
        let Some(rgb) = self.rgb else {
            return Some(self.index);
        };
        match rgb >> 24 {
            0xC0 => Some(256),
            0xC1 => Some(0),
            0xC3 => Some((rgb & 0xFF) as i16),
            _ => None,
        }
    }
}

/// DXF's lineweight in hundredths of a millimetre for the 5-bit index
/// the flags word carries (§9 `CELWEIGHT`). Indices 0 to 23 are AutoCAD's
/// published lineweight list (the settings of its LWEIGHT command and the
/// values group 370 of Autodesk's public DXF Reference takes, 0.00 mm to
/// 2.11 mm) in ascending order; 29 is BYLAYER (-1), 30 BYBLOCK (-2) and 31
/// the default (-3). §9 does not give this table. The public corpus
/// measures index 29 only; every other entry is the published list and is
/// not measured.
pub fn lineweight_hundredths(index: u8) -> Option<i16> {
    const SET: [i16; 24] = [
        0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
        200, 211,
    ];
    match index {
        0..=23 => Some(SET[index as usize]),
        29 => Some(-1),
        30 => Some(-2),
        31 => Some(-3),
        _ => None,
    }
}

/// One decoded variable.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `B`, or a flag taken from the flags word.
    Bool(bool),
    /// `RC`.
    Byte(u8),
    /// `BS`, or a field taken from the flags word.
    Short(i16),
    /// `BL`.
    Long(i32),
    /// `BLL`.
    LongLong(u64),
    /// `BD`.
    Double(f64),
    /// `2RD`.
    Point2([f64; 2]),
    /// `3BD`.
    Point3([f64; 3]),
    /// `TV`.
    Text(String),
    /// `H`.
    Handle(Handle),
    /// `CMC`.
    Color(Color),
    /// A Julian day and the milliseconds into the day.
    Date {
        /// Julian day number.
        day: i32,
        /// Milliseconds into the day.
        ms: i32,
    },
}

impl Value {
    /// The value as a double, when it is one.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Double(v) => Some(*v),
            _ => None,
        }
    }

    /// The value widened to `i64`, when it is an integer, a byte or a
    /// flag.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Bool(b) => Some(i64::from(*b)),
            Self::Byte(v) => Some(i64::from(*v)),
            Self::Short(v) => Some(i64::from(*v)),
            Self::Long(v) => Some(i64::from(*v)),
            Self::LongLong(v) => i64::try_from(*v).ok(),
            _ => None,
        }
    }

    /// The value as text, when it is a `TV`.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    /// The value as a handle reference, when it is an `H`.
    pub fn as_handle(&self) -> Option<Handle> {
        match self {
            Self::Handle(h) => Some(*h),
            _ => None,
        }
    }

    /// A date as one Julian date, the day plus the fraction of it.
    pub fn as_julian(&self) -> Option<f64> {
        match self {
            Self::Date { day, ms } => Some(f64::from(*day) + f64::from(*ms) / 86_400_000.0),
            _ => None,
        }
    }
}

/// A decoded slot: its row of the table and what the stream held.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The table row (name, bit code, releases).
    pub var: &'static Var,
    /// The decoded value.
    pub value: Value,
}

/// Decoded form of the `AcDb:Header` section.
#[derive(Debug, Clone)]
pub struct HeaderVars {
    /// First 16 bytes of the section (§9's beginning sentinel).
    pub sentinel: [u8; 16],
    /// The `RL` size of the section, in bytes (§9: "Size of the section").
    pub size: u32,
    /// The `RL` size in bits, from R2007.
    pub size_in_bits: Option<u32>,
    /// The stored `RS` CRC.
    pub crc: u16,
    /// The CRC of the bytes after the sentinel, which §9 says it holds.
    pub crc_computed: u16,
    /// Whether the CRC of the bytes after the sentinel equals it.
    pub crc_ok: bool,
    /// Whether the ending sentinel is the §9 one.
    pub end_sentinel_ok: bool,
    /// The data bytes: the bit stream, from after the size words to the
    /// CRC.
    pub body: Vec<u8>,
    /// Decoded format version.
    pub version: Version,
    /// Every slot the release carries, in stream order (read it through
    /// [`Self::entries`]; it is private so the name index cannot go stale).
    entries: Vec<Entry>,
    /// Bit offset in [`Self::body`] at which the data stream ended.
    pub end_bit: usize,
    /// From R2007, the bit at which the handle stream ended.
    pub handle_end_bit: Option<usize>,
    /// Whether every slot was read and the data ends on that slot's last
    /// byte.
    pub closed: bool,
    /// Whether the bits between the last slot and the end of the data are
    /// all ones, as they are on every file measured (§9 says random).
    pub pad_ones: bool,
    /// What a lossy parse found wrong; empty for a clean decode.
    pub diagnostics: Vec<String>,
    index: HashMap<&'static str, usize>,
}

struct Frame {
    sentinel: [u8; 16],
    size: u32,
    bits: Option<u32>,
    body: (usize, usize),
    crc: u16,
    crc_computed: u16,
    crc_ok: bool,
    end_sentinel_ok: bool,
}

/// The section's ending sentinel (§9).
const END_SENTINEL: [u8; 16] = [
    0x30, 0x84, 0xE0, 0xDC, 0x02, 0x21, 0xC7, 0x56, 0xA0, 0x83, 0x97, 0x47, 0xB1, 0x92, 0xCC, 0xA0,
];

/// Smallest section: both sentinels, the size word and the CRC.
const MIN_SECTION: usize = 16 + 4 + 2 + 16;

fn release_of(version: Version) -> Rel {
    match version {
        Version::R14 => R14,
        Version::R2000 => R2000,
        Version::R2004 => R2004,
        Version::R2007 => R2007,
        Version::R2010 => header_table::R2010,
        Version::R2013 => R2013,
        Version::R2018 => R2018,
    }
}

fn rl_at(bytes: &[u8], at: usize) -> Result<u32> {
    let word = bytes.get(at..at + 4).ok_or(Error::Truncated {
        offset: at as u64,
        wanted: 4,
        len: bytes.len() as u64,
    })?;
    Ok(u32::from_le_bytes(word.try_into().expect("4 bytes")))
}

fn frame(bytes: &[u8], version: Version, limits: &ParseLimits, strict: bool) -> Result<Frame> {
    if bytes.len() < MIN_SECTION {
        return Err(Error::Truncated {
            offset: 0,
            wanted: MIN_SECTION,
            len: bytes.len() as u64,
        });
    }
    if bytes.len() > limits.max_header_bytes {
        return Err(Error::SectionMap(format!(
            "AcDb:Header is {} bytes, over the {} byte limit",
            bytes.len(),
            limits.max_header_bytes
        )));
    }
    let mut sentinel = [0u8; 16];
    sentinel.copy_from_slice(&bytes[..16]);
    if strict && sentinel != HeaderVars::SENTINEL {
        return Err(Error::SectionMap(
            "AcDb:Header does not start with the §9 sentinel".into(),
        ));
    }
    let size = rl_at(bytes, 16)?;
    let size_len = size as usize;

    // (bytes before `size` counts from, where the size in bits sits).
    let candidates: &[(usize, Option<usize>)] = match version {
        Version::R14 | Version::R2000 | Version::R2004 => &[(20, None)],
        Version::R2007 => &[(20, Some(20))],
        Version::R2010 | Version::R2013 => &[(24, Some(24)), (20, Some(20))],
        Version::R2018 => &[(24, Some(24))],
    };
    let exact = |from: usize| {
        from.checked_add(size_len)
            .and_then(|end| end.checked_add(18))
            == Some(bytes.len())
    };
    let (from, bits_at) = match candidates.iter().find(|(from, _)| exact(*from)) {
        Some(c) => *c,
        None if strict => {
            return Err(Error::SectionMap(format!(
                "AcDb:Header size {size} does not account for its {} bytes",
                bytes.len()
            )));
        }
        None => candidates[0],
    };
    let data_end = from
        .checked_add(size_len)
        .filter(|end| end.checked_add(2).is_some_and(|e| e <= bytes.len()))
        .ok_or(Error::Truncated {
            offset: 16,
            wanted: size_len,
            len: bytes.len() as u64,
        })?;
    let bits = bits_at.map(|at| rl_at(bytes, at)).transpose()?;
    let body_start = bits_at.map_or(from, |at| at + 4);
    if body_start > data_end {
        return Err(Error::SectionMap(
            "AcDb:Header size is shorter than its own size words".into(),
        ));
    }
    let crc = u16::from_le_bytes([bytes[data_end], bytes[data_end + 1]]);
    let crc_computed = crc8(0xC0C1, &bytes[16..data_end]);
    let crc_ok = crc_computed == crc;
    let end_sentinel_ok = bytes.get(data_end + 2..data_end + 18) == Some(&END_SENTINEL[..]);
    Ok(Frame {
        sentinel,
        size,
        bits,
        body: (body_start, data_end),
        crc,
        crc_computed,
        crc_ok,
        end_sentinel_ok,
    })
}

struct SplitPlan {
    strings: Option<crate::string_stream::StringStream>,
    handles_at: usize,
}

fn tail_is_ones(bytes: &[u8], from: usize) -> bool {
    (from..bytes.len() * 8).all(|i| bit_at(bytes, i) == Some(true))
}

fn bit_at(bytes: &[u8], bit: usize) -> Option<bool> {
    let byte = *bytes.get(bit / 8)?;
    Some((byte >> (7 - (bit % 8))) & 1 != 0)
}

struct Reader<'a> {
    data: BitCursor<'a>,
    version: Version,
    split: bool,
    strings: Option<crate::string_stream::StringReader<'a>>,
    handles: Option<BitCursor<'a>>,
}

impl Reader<'_> {
    fn tv(&mut self) -> Result<String> {
        if !self.split {
            return crate::tables::read_tv(&mut self.data, self.version);
        }
        match self.strings.as_mut() {
            Some(s) => s.read_tv(),
            None => Ok(String::new()),
        }
    }

    fn handle(&mut self) -> Result<Handle> {
        if !self.split {
            return self.data.read_handle();
        }
        match self.handles.as_mut() {
            Some(h) => h.read_handle(),
            None => Ok(Handle {
                code: 0,
                counter: 0,
                value: 0,
            }),
        }
    }

    fn cmc(&mut self, rel: Rel) -> Result<Color> {
        let index = self.data.read_bs()?;
        if rel < R2004 {
            return Ok(Color {
                index,
                rgb: None,
                flags: 0,
                name: None,
                book: None,
            });
        }
        let rgb = self.data.read_bl_u()?;
        let flags = self.data.read_rc()?;
        let name = if flags & 1 != 0 {
            Some(self.tv()?)
        } else {
            None
        };
        let book = if flags & 2 != 0 {
            Some(self.tv()?)
        } else {
            None
        };
        Ok(Color {
            index,
            rgb: Some(rgb),
            flags,
            name,
            book,
        })
    }

    fn value(
        &mut self,
        row: &Var,
        rel: Rel,
        flags: u32,
        earlier: &[Entry],
    ) -> Result<Option<Value>> {
        Ok(Some(match row.kind {
            Kind::B => Value::Bool(self.data.read_b()?),
            Kind::Bs => Value::Short(self.data.read_bs()?),
            Kind::Bl => Value::Long(self.data.read_bl()?),
            Kind::Bll => Value::LongLong(self.data.read_bll()?),
            Kind::Bd => Value::Double(self.data.read_bd()?),
            Kind::Rc => Value::Byte(self.data.read_rc()?),
            Kind::Bd3 => {
                let c = &mut self.data;
                Value::Point3([c.read_bd()?, c.read_bd()?, c.read_bd()?])
            }
            Kind::Rd2 => {
                let c = &mut self.data;
                Value::Point2([c.read_rd()?, c.read_rd()?])
            }
            Kind::Tv => Value::Text(self.tv()?),
            Kind::H => Value::Handle(self.handle()?),
            Kind::HData => Value::Handle(self.data.read_handle()?),
            Kind::TailB => {
                let c = self.handles.as_mut().unwrap_or(&mut self.data);
                Value::Bool(c.read_b()?)
            }
            Kind::TailBs => {
                let c = self.handles.as_mut().unwrap_or(&mut self.data);
                Value::Short(c.read_bs()?)
            }
            Kind::HWhen(name, eq) => {
                let present = earlier
                    .iter()
                    .find(|e| e.var.name == name)
                    .and_then(|e| e.value.as_i64())
                    == Some(i64::from(eq));
                if !present {
                    return Ok(None);
                }
                Value::Handle(self.handle()?)
            }
            Kind::Cmc => Value::Color(self.cmc(rel)?),
            Kind::Date => {
                let day = self.data.read_bl()?;
                let ms = self.data.read_bl()?;
                Value::Date { day, ms }
            }
            Kind::Lineweight => match lineweight_hundredths((flags & 0x1F) as u8) {
                Some(v) => Value::Short(v),
                None => {
                    return Err(Error::SectionMap(format!(
                        "lineweight index {} is not one AutoCAD defines",
                        flags & 0x1F
                    )));
                }
            },
            Kind::Bits { mask, invert } => {
                let field = flags & mask;
                if mask.count_ones() == 1 {
                    Value::Bool((field != 0) != invert)
                } else {
                    Value::Short((field >> mask.trailing_zeros()) as i16)
                }
            }
        }))
    }
}

impl HeaderVars {
    /// §9's beginning sentinel (the 16 bytes before the size word).
    pub const SENTINEL: [u8; 16] = [
        0xCF, 0x7B, 0x1F, 0x23, 0xFD, 0xDE, 0x38, 0xA9, 0x5F, 0x7C, 0x68, 0xB8, 0x4E, 0x6D, 0x33,
        0x5F,
    ];

    /// Decode a decompressed `AcDb:Header` section (§9) under
    /// [`ParseLimits::safe`]. Errors on any frame, CRC, sentinel or
    /// stream-boundary failure.
    pub fn parse_strict(bytes: &[u8], version: Version) -> Result<Self> {
        Self::decode(bytes, version, &ParseLimits::safe())
    }

    /// Decode a decompressed `AcDb:Header` section (§9) under `limits`.
    ///
    /// Strict: the sentinels and CRC must be right, every slot of the
    /// release must read, and the data must end on the last slot's byte.
    pub fn decode(bytes: &[u8], version: Version, limits: &ParseLimits) -> Result<Self> {
        let h = Self::build(bytes, version, limits, true)?;
        if !h.crc_ok {
            return Err(Error::CrcMismatch {
                context: "AcDb:Header (§9)".into(),
                expected: h.crc,
                actual: h.crc_computed,
            });
        }
        if !h.end_sentinel_ok {
            return Err(Error::SectionMap(
                "AcDb:Header does not end with the §9 sentinel".into(),
            ));
        }
        if let Some(d) = h.diagnostics.first() {
            return Err(Error::SectionMap(d.clone()));
        }
        Ok(h)
    }

    /// Decode what can be decoded of an `AcDb:Header` section (§9).
    ///
    /// A section shorter than the frame gives an empty result. Any later
    /// failure stops the slots there and is kept in
    /// [`Self::diagnostics`]; the slots read before it are returned.
    pub fn parse_lossy(bytes: &[u8], version: Version) -> Result<Self> {
        Ok(
            Self::build(bytes, version, &ParseLimits::safe(), false).unwrap_or_else(|e| Self {
                sentinel: [0; 16],
                size: 0,
                size_in_bits: None,
                crc: 0,
                crc_computed: 0,
                crc_ok: false,
                end_sentinel_ok: false,
                body: Vec::new(),
                version,
                entries: Vec::new(),
                end_bit: 0,
                handle_end_bit: None,
                closed: false,
                pad_ones: false,
                diagnostics: vec![e.to_string()],
                index: HashMap::new(),
            }),
        )
    }

    /// Back-compat name for [`Self::parse_lossy`].
    pub fn parse(bytes: &[u8], version: Version) -> Result<Self> {
        Self::parse_lossy(bytes, version)
    }

    fn build(bytes: &[u8], version: Version, limits: &ParseLimits, strict: bool) -> Result<Self> {
        let f = frame(bytes, version, limits, strict)?;
        let body = bytes[f.body.0..f.body.1].to_vec();
        let mut h = Self {
            sentinel: f.sentinel,
            size: f.size,
            size_in_bits: f.bits,
            crc: f.crc,
            crc_computed: f.crc_computed,
            crc_ok: f.crc_ok,
            end_sentinel_ok: f.end_sentinel_ok,
            body,
            version,
            entries: Vec::new(),
            end_bit: 0,
            handle_end_bit: None,
            closed: false,
            pad_ones: false,
            diagnostics: Vec::new(),
            index: HashMap::new(),
        };
        if !f.crc_ok {
            h.diagnostics.push("CRC differs".into());
        }
        if !f.end_sentinel_ok {
            h.diagnostics.push("ending sentinel differs".into());
        }
        if version.is_r2007_plus() {
            h.read_split(version);
            return Ok(h);
        }
        h.read_inline(version);
        Ok(h)
    }

    fn read_inline(&mut self, version: Version) {
        let rel = release_of(version);
        let body = std::mem::take(&mut self.body);
        let mut r = Reader {
            data: BitCursor::new(&body),
            version,
            split: false,
            strings: None,
            handles: None,
        };
        let mut flags = 0u32;
        let mut done = true;
        for row in header_table::rows_for(rel) {
            match r.value(row, rel, flags, &self.entries) {
                Ok(Some(value)) => {
                    if row.name == "FLAGS" {
                        flags = value.as_i64().unwrap_or(0) as u32;
                    }
                    self.entries.push(Entry { var: row, value });
                }
                Ok(None) => {}
                Err(e) => {
                    self.diagnostics.push(format!(
                        "{} (bit {}): {e}",
                        row.name,
                        r.data.position_bits()
                    ));
                    done = false;
                    break;
                }
            }
        }
        self.end_bit = r.data.position_bits();
        if done {
            let total = body.len() * 8;
            if self.end_bit <= total && total - self.end_bit < 8 {
                self.closed = true;
                self.pad_ones = tail_is_ones(&body, self.end_bit);
            } else {
                self.diagnostics.push(format!(
                    "the slots end at bit {} of {total}: the data does not close",
                    self.end_bit
                ));
            }
        }
        self.body = body;
        self.index = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.var.name, i))
            .collect();
    }

    fn read_split(&mut self, version: Version) {
        let rel = release_of(version);
        let body = std::mem::take(&mut self.body);
        let total = body.len() * 8;
        let plan = match self.split_plan(&body) {
            Ok(p) => p,
            Err(e) => {
                self.diagnostics.push(e);
                self.body = body;
                return;
            }
        };
        let strings_end = plan.strings.map_or(plan.handles_at - 1, |s| s.end_bit);
        let reader = match plan.strings {
            Some(stream) => crate::string_stream::StringReader::new(&body, stream),
            None => Ok(crate::string_stream::StringReader::empty(&body)),
        };
        let mut handles = BitCursor::new(&body);
        let (strings, ()) = match (
            reader,
            crate::string_stream::seek(&mut handles, plan.handles_at),
        ) {
            (Ok(s), Ok(())) => (s, ()),
            _ => {
                self.diagnostics
                    .push("a stream starts past the data".into());
                self.body = body;
                return;
            }
        };
        let string_start = plan.strings.map_or(plan.handles_at - 1, |s| s.start_bit);
        let mut r = Reader {
            data: BitCursor::new(&body),
            version,
            split: true,
            strings: Some(strings),
            handles: Some(handles),
        };
        let mut flags = 0u32;
        let mut done = true;
        for row in header_table::rows_for(rel) {
            match r.value(row, rel, flags, &self.entries) {
                Ok(Some(value)) => {
                    if row.name == "FLAGS" {
                        flags = value.as_i64().unwrap_or(0) as u32;
                    }
                    self.entries.push(Entry { var: row, value });
                }
                Ok(None) => {}
                Err(e) => {
                    self.diagnostics.push(format!(
                        "{} (data bit {}): {e}",
                        row.name,
                        r.data.position_bits()
                    ));
                    done = false;
                    break;
                }
            }
        }
        self.end_bit = r.data.position_bits();
        let handle_end = r.handles.as_ref().map_or(0, |h| h.position_bits());
        let string_pos = r.strings.as_ref().map_or(0, |s| s.position_bits());
        if done {
            let mut closes = true;
            if self.end_bit != string_start {
                self.diagnostics.push(format!(
                    "the data stream ends at bit {}, the string stream starts at {string_start}",
                    self.end_bit
                ));
                closes = false;
            }
            if plan.strings.is_some() && string_pos != strings_end {
                self.diagnostics.push(format!(
                    "the strings end at bit {string_pos}, the string stream at {strings_end}"
                ));
                closes = false;
            }
            if handle_end > total || total - handle_end >= 8 {
                self.diagnostics.push(format!(
                    "the handles end at bit {handle_end} of {total}: the handle stream does not close"
                ));
                closes = false;
            }
            self.closed = closes;
            if closes {
                self.pad_ones = tail_is_ones(&body, handle_end);
            }
        }
        self.handle_end_bit = Some(handle_end);
        self.body = body;
        self.index = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.var.name, i))
            .collect();
    }

    /// Where the string stream and the handle stream sit (§9, §19.1): the
    /// size in bits counts from 32 bits before the data, it ends at the
    /// first handle bit, and the string stream is found backwards from
    /// there.
    fn split_plan(&self, body: &[u8]) -> std::result::Result<SplitPlan, String> {
        let bits = self
            .size_in_bits
            .ok_or_else(|| "no size in bits".to_string())? as usize;
        let total = body.len() * 8;
        let handles_at = bits
            .checked_sub(32)
            .filter(|at| *at <= total && *at >= 1)
            .ok_or_else(|| format!("size in bits {bits} does not fit the {total} bit data"))?;
        let flag = bit_at(body, handles_at - 1)
            .ok_or_else(|| "the strings-present bit is outside the data".to_string())?;
        if !flag {
            return Ok(SplitPlan {
                strings: None,
                handles_at,
            });
        }
        let word = |at: usize| -> Option<u16> {
            let mut c = BitCursor::new(body);
            crate::string_stream::seek(&mut c, at).ok()?;
            c.read_rs().ok().map(|v| v as u16)
        };
        let lo_at = handles_at
            .checked_sub(17)
            .ok_or_else(|| "no room for the string-stream size".to_string())?;
        let lo = word(lo_at).ok_or_else(|| "the string-stream size is unreadable".to_string())?;
        let (size, end_bit) = if lo & 0x8000 != 0 {
            let hi_at = lo_at
                .checked_sub(16)
                .ok_or_else(|| "no room for the high string-stream size".to_string())?;
            let hi = word(hi_at).ok_or_else(|| "the high size is unreadable".to_string())?;
            ((usize::from(lo & 0x7FFF)) | (usize::from(hi) << 15), hi_at)
        } else {
            (usize::from(lo), lo_at)
        };
        let start_bit = end_bit
            .checked_sub(size)
            .ok_or_else(|| format!("the string stream of {size} bits starts before the data"))?;
        Ok(SplitPlan {
            strings: Some(crate::string_stream::StringStream { start_bit, end_bit }),
            handles_at,
        })
    }

    /// The variable named `name` (`$INSBASE`, `UNKNOWN_BD_1`, ...), when
    /// the release carries it.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.index.get(name).map(|&i| &self.entries[i].value)
    }

    /// Every slot the release carries, in stream order. Short when a lossy
    /// parse stopped on a failure.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Every slot, in stream order.
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// The slots §9 calls unknown, in stream order.
    pub fn unknowns(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.var.is_unknown())
    }

    /// A `BD` variable.
    pub fn get_f64(&self, name: &str) -> Option<f64> {
        self.get(name)?.as_f64()
    }

    /// A `B`, `BS`, `BL` or `RC` variable.
    pub fn get_i64(&self, name: &str) -> Option<i64> {
        self.get(name)?.as_i64()
    }

    /// A `TV` variable.
    pub fn get_text(&self, name: &str) -> Option<&str> {
        self.get(name)?.as_text()
    }

    /// An `H` variable.
    pub fn get_handle(&self, name: &str) -> Option<Handle> {
        self.get(name)?.as_handle()
    }

    /// Whether the section's leading sentinel matches §9's.
    pub fn has_valid_sentinel(&self) -> bool {
        self.sentinel == Self::SENTINEL
    }

    /// Read the first BD in the header bit-stream (the `$UNKNOWN` default
    /// `412148564080.0` that §9 lists first).
    pub fn read_first_bd(&self) -> Option<f64> {
        let mut c = BitCursor::new(&self.body);
        c.read_bd().ok()
    }

    /// Read the first N `BD` values of the bit-stream.
    pub fn read_first_n_bds(&self, n: usize) -> Vec<f64> {
        let mut c = BitCursor::new(&self.body);
        let mut out = Vec::with_capacity(n.min(1024));
        for _ in 0..n.min(1024) {
            match c.read_bd() {
                Ok(v) => out.push(v),
                Err(_) => break,
            }
        }
        out
    }

    /// Read the first N `BL` values of the bit-stream.
    pub fn read_first_n_bls(&self, n: usize) -> Vec<i32> {
        let mut c = BitCursor::new(&self.body);
        let mut out = Vec::with_capacity(n.min(1024));
        for _ in 0..n.min(1024) {
            match c.read_bl() {
                Ok(v) => out.push(v),
                Err(_) => break,
            }
        }
        out
    }

    /// Length of the bit-stream body in bytes.
    pub fn body_len(&self) -> usize {
        self.body.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_bytes() {
        let h = HeaderVars::parse(&[], Version::R2018).unwrap();
        assert_eq!(h.body_len(), 0);
        assert!(!h.has_valid_sentinel());
    }

    #[test]
    fn parse_short_bytes_is_lenient() {
        let h = HeaderVars::parse(&[0u8; 10], Version::R2018).unwrap();
        assert_eq!(h.body_len(), 0);
    }

    #[test]
    fn parse_lossy_matches_parse() {
        let h = HeaderVars::parse_lossy(&[0u8; 10], Version::R2018).unwrap();
        assert_eq!(h.body_len(), 0);
        assert!(!h.has_valid_sentinel());
    }

    #[test]
    fn parse_strict_errors_on_short_input() {
        let err = HeaderVars::parse_strict(&[0u8; 10], Version::R2018).unwrap_err();
        assert!(
            matches!(
                err,
                crate::error::Error::Truncated {
                    wanted: MIN_SECTION,
                    len: 10,
                    ..
                }
            ),
            "err={err:?}"
        );
    }
}
