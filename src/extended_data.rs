//! Extended entity data (§28) and XRECORD data (§20.4.104) read as values, with
//! their strings in the code page each string names (§3.2.5, [`crate::code_page`]).
//!
//! Both are lists of values keyed by a DXF group code and both hold their text as
//! single bytes in a code page up to R2004, so one reader serves them. The record
//! and the reader here are for R13 to R2004; R2007 and later hold UTF-16 and are
//! refused ([`Error::Unsupported`]).
//!
//! # What the specification says and what the corpus measures
//!
//! §28 gives each extended data item a one-byte code (the DXF group code minus
//! 1000) and a value: a string (`R13-R2004: 1st byte of value is the length N; this
//! is followed by a 2-byte short indicating the codepage, followed by N single-byte
//! characters`), a brace (1 byte), a layer or entity handle (8 bytes), a binary chunk
//! (a length byte and the bytes), three doubles, one double, a short or a long.
//! §20.4.104 gives XRECORD's pairs as an `RS` group code and a value of the width
//! the code's DXF type has, and a string as "the string length (RC), the dwgcodepage
//! (RC), and then the string".
//!
//! Measured on every pre-R2007 drawing of the pinned corpus (ARCHITECTURE.md §7as):
//! both strings are the same three bytes before the characters, **an `RS` length and
//! an `RC` code page**: the specification's "RC length, RC page" (§20.4.104) and
//! "RC length, short page" (§28) are each a length below 256, a zero byte and the
//! page, and no string of the corpus is 256 bytes or longer, so only the `RS` length
//! is a reading the data cannot tell from the others. The page byte of every one of
//! those strings equals the file header's short.
//!
//! The XRECORD value widths are those of the DXF Reference's group code ranges
//! (public documentation, disclosed in CLEANROOM.md), taken for a range only where a
//! code of the range occurs in the corpus and every XRECORD body then closes exactly
//! on its length. A code outside those ranges is [`Error::Unsupported`], never read
//! at a guessed width.

use crate::bitcursor::Handle;
use crate::code_page::CodePage;
use crate::error::{Error, Result};
use crate::object::{RawObject, body_cursor};
use crate::version::Version;

/// One value of a group-coded list, by the kind its DXF group code has.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupValue {
    /// A string, decoded in the code page its own page byte names; the page is
    /// kept beside the text.
    Text {
        /// The string's own page byte (§28, §20.4.104), as a [`CodePage`].
        page: CodePage,
        /// The text.
        text: String,
    },
    /// Extended data's `{` (`open`) or `}` (§28, code 1002).
    Brace {
        /// `true` for `{`.
        open: bool,
    },
    /// A handle: 8 bytes, most significant first in extended data (§28 codes 1003
    /// and 1005, "read as hex") and least significant first in an XRECORD (330 to
    /// 369), as measured against the exports' handles.
    Handle(u64),
    /// A binary chunk: a length byte and that many bytes.
    Binary(Vec<u8>),
    /// A 3D point: three doubles.
    Point([f64; 3]),
    /// A double.
    Real(f64),
    /// A 16-bit integer.
    Short(i16),
    /// A 32-bit integer.
    Long(i32),
    /// An 8-bit integer or a boolean flag.
    Byte(u8),
}

/// A value and its DXF group code (an extended data item's code is 1000 plus the
/// byte §28 stores).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupItem {
    /// The DXF group code.
    pub code: u16,
    /// The value.
    pub value: GroupValue,
}

/// One application's extended entity data (§28): the application's table handle and
/// its items.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtendedData {
    /// The application (APPID) table handle, as the chain stores it.
    pub app: Handle,
    /// The items after the handle, in order.
    pub items: Vec<GroupItem>,
}

struct Bytes<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    fn done(&self) -> bool {
        self.at >= self.b.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.b.len())
            .ok_or_else(|| Error::SectionMap("group-coded data ends inside a value".into()))?;
        let out = &self.b[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn real(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    fn text(&mut self) -> Result<GroupValue> {
        let [lo, hi] = self.array()?;
        let len = usize::from(u16::from_le_bytes([lo, hi]));
        let [page] = self.array()?;
        let page = CodePage::from_header(u16::from(page));
        Ok(GroupValue::Text {
            page,
            text: page.decode(self.take(len)?),
        })
    }

    fn binary(&mut self) -> Result<GroupValue> {
        let [len] = self.array()?;
        Ok(GroupValue::Binary(self.take(usize::from(len))?.to_vec()))
    }

    fn handle_le(&mut self) -> Result<GroupValue> {
        Ok(GroupValue::Handle(u64::from_le_bytes(self.array()?)))
    }

    fn handle_be(&mut self) -> Result<GroupValue> {
        Ok(GroupValue::Handle(u64::from_be_bytes(self.array()?)))
    }

    fn point(&mut self) -> Result<GroupValue> {
        Ok(GroupValue::Point([
            self.real()?,
            self.real()?,
            self.real()?,
        ]))
    }
}

fn pre_r2007(version: Version, what: &str) -> Result<()> {
    if version.is_r2007_plus() {
        return Err(Error::Unsupported {
            feature: format!("{what} from R2007 (UTF-16 strings; only R13 to R2004 are read)"),
        });
    }
    Ok(())
}

/// The values of an XRECORD's data (§20.4.104): `RS` group code and value, to the end
/// of `data`. A code outside the ranges the corpus measures, or a value that runs
/// past the data, is an error.
pub(crate) fn xrecord_items(data: &[u8], version: Version) -> Result<Vec<GroupItem>> {
    pre_r2007(version, "XRECORD values")?;
    let mut r = Bytes { b: data, at: 0 };
    let mut out = Vec::new();
    while !r.done() {
        let [lo, hi] = r.array()?;
        let code = u16::from_le_bytes([lo, hi]);
        let value = match code {
            1..=4 | 6..=9 | 102 | 300..=309 | 1000..=1003 => r.text()?,
            10..=39 => r.point()?,
            40..=59 | 140..=149 | 460..=469 => GroupValue::Real(r.real()?),
            60..=79 | 170..=179 | 270..=279 | 370..=379 | 1070 => {
                GroupValue::Short(i16::from_le_bytes(r.array()?))
            }
            90..=99 | 420..=429 | 440..=449 | 1071 => {
                GroupValue::Long(i32::from_le_bytes(r.array()?))
            }
            280..=299 => GroupValue::Byte(r.array::<1>()?[0]),
            310..=319 | 1004 => r.binary()?,
            330..=369 => r.handle_le()?,
            _ => {
                return Err(Error::Unsupported {
                    feature: format!("XRECORD group code {code} (its width is not measured)"),
                });
            }
        };
        out.push(GroupItem { code, value });
    }
    Ok(out)
}

/// The values of the XRECORD `x` (§20.4.104), R13 to R2004: its body read as `RS` group
/// code and value pairs. [`crate::objects::xrecord::XRecord::data`] is always there to
/// read as bytes.
pub fn xrecord_values(
    x: &crate::objects::xrecord::XRecord,
    version: Version,
) -> Result<Vec<GroupItem>> {
    xrecord_items(&x.data, version)
}

fn extended_items(payload: &[u8]) -> Result<Vec<GroupItem>> {
    let mut r = Bytes { b: payload, at: 0 };
    let mut out = Vec::new();
    while !r.done() {
        let [byte] = r.array()?;
        let code = 1000 + u16::from(byte);
        let value = match byte {
            0 => r.text()?,
            2 => GroupValue::Brace {
                open: r.array::<1>()?[0] == 0,
            },
            3 | 5 => r.handle_be()?,
            4 => r.binary()?,
            10..=13 => r.point()?,
            40..=42 => GroupValue::Real(r.real()?),
            70 => GroupValue::Short(i16::from_le_bytes(r.array()?)),
            71 => GroupValue::Long(i32::from_le_bytes(r.array()?)),
            _ => {
                return Err(Error::Unsupported {
                    feature: format!("extended data item code {code}"),
                });
            }
        };
        out.push(GroupItem { code, value });
    }
    Ok(out)
}

/// The extended entity data (§28) of the walked record `raw`, application by
/// application: the chain of `BS` size, application handle and payload that follows
/// the object handle, ended by a size of 0, with each payload read into items.
///
/// Reads R13 to R2004 only. A payload that does not close exactly on its size, or
/// holds an item code §28 does not list, is an error; the chain the common data
/// readers skip is unchanged.
pub fn read(raw: &RawObject, version: Version) -> Result<Vec<ExtendedData>> {
    pre_r2007(version, "extended entity data")?;
    const MAX_APPLICATIONS: usize = 256;
    let mut c = body_cursor(raw, version)?;
    let mut out = Vec::new();
    for _ in 0..MAX_APPLICATIONS {
        let size = c.read_bs_u()?;
        if size == 0 {
            return Ok(out);
        }
        let app = c.read_handle()?;
        let payload = (0..size)
            .map(|_| c.read_rc())
            .collect::<Result<Vec<u8>>>()?;
        out.push(ExtendedData {
            app,
            items: extended_items(&payload)?,
        });
    }
    Err(Error::SectionMap(format!(
        "extended data chain exceeds {MAX_APPLICATIONS} applications"
    )))
}
