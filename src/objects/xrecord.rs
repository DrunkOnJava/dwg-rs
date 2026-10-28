//! XRECORD object (§20.4.104) — opaque key/value storage under a
//! parent DICTIONARY.
//!
//! XRECORD is how the DWG format extends itself without changing the
//! spec: each XRECORD is a list of (group-code, value) pairs like
//! DXF group codes (10 = 3D point, 40 = double, 1 = string, etc.).
//! The consuming application knows what group codes mean for its
//! specific dictionary entry.
//!
//! # Stream shape
//!
//! ```text
//! BL     num_databytes          -- size of the raw body (≤ 16 MiB)
//! // body: concatenated raw bytes of DXF-style (code, value) pairs
//! // encoded per group-code type, following "the normal AutoCAD DXF
//! // convention for group codes" (§20.4.104).
//! BS     cloning_flags          -- R2000+ only
//! ```
//!
//! §20.4.104 types XRECORD `(varies)`. From R2000 on it is the fixed
//! code `0x4F`; R14 files give it a class number from AcDb:Classes, so
//! `entities::dispatch` routes the `XRECORD` class name here as well
//! (#149). Measured 2026-10-26: with that route every XRECORD of the
//! public corpus closes on its boundary, the 1,154 R14 ones included
//! (115 in each of six nextgis R14 drawings, 122 in `ellipse_R14.dwg`,
//! 342 in ACadSharp's `sample_AC1014.dwg`).
//!
//! # Opaque-body handling
//!
//! This initial decoder treats the body as opaque bytes. Full typed
//! (code, value) tuple decomposition mirrors the XData type map
//! (spec §3.5) and will land in a follow-up — the shape of each
//! pair depends on the specific dictionary entry that owns the
//! XRECORD, so many consumers will prefer the raw bytes anyway.
//!
//! Byte-count is capped at [`MAX_XRECORD_BYTES`] to bound allocation
//! from adversarial inputs.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::version::Version;

/// Sanity cap on XRECORD body size (16 MiB). Real dictionary payloads
/// are typically a few hundred bytes.
pub const MAX_XRECORD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XRecord {
    /// Opaque raw bytes of the DXF-style (code, value) pair sequence.
    pub data: Vec<u8>,
    /// R2000+ cloning flags (0 if not present — i.e. R14 file).
    pub cloning_flags: i16,
}

/// One value of an XRECORD's data (§20.4.104): the type follows the group
/// code's range, as in DXF.
#[derive(Debug, Clone, PartialEq)]
pub enum XValue {
    /// A string (codes 1 to 9, 100 to 102, 105, 300 to 309, 410 to 419, 430 to
    /// 439, 470 to 479, 999, 1000 to 1003).
    Text(String),
    /// A 16-, 32- or 64-bit integer or a byte (the numeric ranges of DXF).
    Int(i64),
    /// A real (40 to 59, 140 to 149, 460 to 469, 1040 to 1042).
    Real(f64),
    /// Three reals (10 to 39, 110 to 119, 210 to 239, 1010 to 1013).
    Point([f64; 3]),
    /// A handle (320 to 369, 390 to 399, 480, 481, 1005), as eight bytes.
    Handle(u64),
    /// A chunk of binary data (310 to 319, 1004).
    Binary(Vec<u8>),
}

/// One (group code, value) pair of an XRECORD.
#[derive(Debug, Clone, PartialEq)]
pub struct XItem {
    /// The DXF group code.
    pub code: u16,
    /// Its value.
    pub value: XValue,
}

struct Bytes<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|e| *e <= self.b.len())
            .ok_or_else(|| Error::SectionMap("XRECORD value runs past its data".into()))?;
        let out = &self.b[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u16(&mut self) -> Result<u16> {
        let v = self.take(2)?;
        Ok(u16::from_le_bytes([v[0], v[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let v = self.take(4)?;
        Ok(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }

    fn u64(&mut self) -> Result<u64> {
        let v = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(v);
        Ok(u64::from_le_bytes(a))
    }

    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }
}

impl XRecord {
    /// The data as (group code, value) pairs: per pair an `RS` code and a value
    /// of the type DXF gives that code. Fails when a value does not fit the
    /// data, or when a code has no type here, so a record that reads at all
    /// reads to its last byte.
    ///
    /// Up to R2004 the data is read by [`crate::extended_data::xrecord_values`],
    /// which decodes each string in the code page it names (§3.2.5, #375) and reads
    /// only the group code ranges the corpus measures. From R2007 a string is an
    /// `RS` length and that many UTF-16 units; a trailing NUL is dropped. A handle
    /// is eight bytes (an `RLL`).
    pub fn items(&self, version: Version) -> Result<Vec<XItem>> {
        if !version.is_r2007_plus() {
            return crate::extended_data::xrecord_items(&self.data, version)?
                .into_iter()
                .map(|item| {
                    use crate::extended_data::GroupValue;
                    let value = match item.value {
                        GroupValue::Text { page, text } => {
                            // A page the crate does not name is decoded as UTF-8 with
                            // U+FFFD, which alters application data: refuse a string
                            // that is not ASCII there.
                            if matches!(page, crate::code_page::CodePage::Unknown(_))
                                && !text.is_ascii()
                            {
                                return Err(Error::SectionMap(
                                    "XRECORD string is in a code page the crate does not name"
                                        .into(),
                                ));
                            }
                            XValue::Text(text)
                        }
                        GroupValue::Handle(h) => XValue::Handle(h),
                        GroupValue::Binary(b) => XValue::Binary(b),
                        GroupValue::Point(p) => XValue::Point(p),
                        GroupValue::Real(r) => XValue::Real(r),
                        GroupValue::Short(n) => XValue::Int(i64::from(n)),
                        GroupValue::Long(n) => XValue::Int(i64::from(n)),
                        GroupValue::Byte(n) => XValue::Int(i64::from(n)),
                        GroupValue::Brace { .. } => {
                            return Err(Error::SectionMap(
                                "XRECORD data holds an extended data brace".into(),
                            ));
                        }
                    };
                    Ok(XItem {
                        code: item.code,
                        value,
                    })
                })
                .collect();
        }
        let mut r = Bytes {
            b: &self.data,
            at: 0,
        };
        let mut out = Vec::new();
        while r.at < r.b.len() {
            let code = r.u16()?;
            let value = match code {
                1..=9
                | 100..=102
                | 105
                | 300..=309
                | 410..=419
                | 430..=439
                | 470..=479
                | 999
                | 1000..=1003 => XValue::Text(Self::text(&mut r)?),
                10..=39 | 110..=119 | 210..=239 | 1010..=1013 => {
                    XValue::Point([r.f64()?, r.f64()?, r.f64()?])
                }
                40..=59 | 140..=149 | 460..=469 | 1040..=1042 => XValue::Real(r.f64()?),
                60..=79 | 170..=179 | 270..=279 | 370..=389 | 400..=409 | 1060..=1070 => {
                    XValue::Int(i64::from(r.u16()? as i16))
                }
                90..=99 | 420..=429 | 440..=459 | 1071 => XValue::Int(i64::from(r.u32()? as i32)),
                160..=169 => XValue::Int(r.u64()? as i64),
                280..=299 => XValue::Int(i64::from(r.take(1)?[0])),
                310..=319 | 1004 => {
                    let n = usize::from(r.take(1)?[0]);
                    XValue::Binary(r.take(n)?.to_vec())
                }
                320..=369 | 390..=399 | 480 | 481 | 1005 => XValue::Handle(r.u64()?),
                other => {
                    return Err(Error::SectionMap(format!(
                        "XRECORD group code {other} has no value type"
                    )));
                }
            };
            out.push(XItem { code, value });
        }
        Ok(out)
    }

    fn text(r: &mut Bytes<'_>) -> Result<String> {
        let len = usize::from(r.u16()?);
        let raw = r.take(
            len.checked_mul(2)
                .ok_or_else(|| Error::SectionMap("XRECORD string length overflows".into()))?,
        )?;
        let mut units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if units.last() == Some(&0) {
            units.pop();
        }
        String::from_utf16(&units)
            .map_err(|_| Error::SectionMap("XRECORD string is not valid UTF-16".into()))
    }
}

/// Decodes the `XRecord` payload that follows the common object header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<XRecord> {
    let data_bytes = c.read_bl()? as usize;
    if data_bytes > MAX_XRECORD_BYTES {
        return Err(Error::SectionMap(format!(
            "XRECORD data bytes {data_bytes} exceeds {MAX_XRECORD_BYTES} sanity cap"
        )));
    }
    let mut data = Vec::with_capacity(data_bytes);
    for _ in 0..data_bytes {
        data.push(c.read_rc()?);
    }
    let cloning_flags = if version == Version::R14 {
        0
    } else {
        c.read_bs()?
    };
    Ok(XRecord {
        data,
        cloning_flags,
    })
}

/// Decode an XRECORD straight from its raw object payload, checking the
/// body ends exactly on the data-stream boundary.
///
/// # Measured: the field list is already version-independent
///
/// XRECORD holds no `TV` at all — its (code, value) pairs are opaque
/// bytes — so unlike its dictionary siblings it needs no string-stream
/// port, only the boundary check. `arc_2013.dwg` handle 19 confirms the
/// shape: 87 bits from the end of the common object data to the handle
/// stream, spent as `BL num_databytes` (10 bits, value 8), eight bytes
/// of body (64 bits), `BS cloning_flags` (10 bits) and the one-bit
/// §19.1 trailer. `arc_2004.dwg` handle 19 spends the same fields
/// against its inline `RL` object-data-size, with no trailer bit.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<XRecord> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = decode(&mut split.data, version)?;
    split.finish("XRECORD")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_empty_xrecord() {
        let mut w = BitWriter::new();
        w.write_bl(0);
        w.write_bs(1);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let x = decode(&mut c, Version::R2000).unwrap();
        assert!(x.data.is_empty());
        assert_eq!(x.cloning_flags, 1);
    }

    #[test]
    fn roundtrip_populated_xrecord() {
        let mut w = BitWriter::new();
        let payload: [u8; 5] = [1, 2, 3, 4, 5];
        w.write_bl(payload.len() as i32);
        for b in &payload {
            w.write_rc(*b);
        }
        w.write_bs(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let x = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(x.data, &payload);
        assert_eq!(x.cloning_flags, 0);
    }

    #[test]
    fn r14_skips_cloning_flags() {
        let mut w = BitWriter::new();
        w.write_bl(3);
        w.write_rc(0xAA);
        w.write_rc(0xBB);
        w.write_rc(0xCC);
        // no cloning_flags for R14
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let x = decode(&mut c, Version::R14).unwrap();
        assert_eq!(x.data, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(x.cloning_flags, 0);
    }

    #[test]
    fn r2018_xrecord_closes_one_bit_before_the_handle_stream() {
        let mut body = crate::objects::modern::tests::r2018_object_prefix(1);
        body.write_bl(3);
        body.write_rc(0xAA);
        body.write_rc(0xBB);
        body.write_rc(0xCC);
        body.write_bs(1);
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::objects::modern::tests::build_payload_without_strings(&bits);
        let x = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(x.data, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(x.cloning_flags, 1);
    }

    #[test]
    fn rejects_oversized_body() {
        let mut w = BitWriter::new();
        // Inflate declared size past the cap.
        w.write_bl((MAX_XRECORD_BYTES + 1) as i32);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("XRECORD")));
    }
}
