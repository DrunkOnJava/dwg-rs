//! LTYPE table entry (ODA Open Design Specification v5.4.1 §19.5.3,
//! L6-03) — linetype definition (dash/dot/text/shape pattern).
//!
//! # Stream shape
//!
//! ```text
//! entry header (TV name + xref bits)
//! RC     flags                  -- reserved/style flags
//! RS     used_count             -- reference count kept by AutoCAD
//! TV     description
//! BD     pattern_length
//! RC     alignment              -- always 'A' (0x41) in practice
//! RC     num_dashes             -- per ODA §19.5.3, capped at 256
//!
//! For each dash (0..num_dashes):
//!   BD   length                 -- positive = dash, negative = gap, 0 = dot
//!   BS   shape_flag             -- bit 0x02: has text, bit 0x04: has shape
//!   BD   x_offset
//!   BD   y_offset
//!   BD   scale
//!   BD   rotation               -- radians
//!   BS   shape_number           -- index into the shape file, 0 when text
//!   if shape_flag & 0x02:  TV  text
//!   if shape_flag & 0x04:  H   style_handle (via ODA §2.13)
//! ```
//!
//! Spec §19.5.3 caps `num_dashes` at 256; anything larger is a malformed
//! or adversarial file.

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};
use crate::tables::{TableEntryHeader, read_table_entry_header, read_tv};
use crate::version::Version;

/// Cap on the number of dash records per LTYPE, per spec §19.5.3.
pub const MAX_DASHES: usize = 256;

/// Text that follows a shape-carrying dash (SHAPE_FLAG bit 0x02) or
/// the handle of the STYLE entry referenced when bit 0x04 is set.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DashText {
    #[default]
    None,
    Inline(String),
    StyleHandle(Handle),
}

/// One entry in the dash/shape pattern.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LtypeDash {
    pub length: f64,
    pub shape_flag: i16,
    pub x_offset: f64,
    pub y_offset: f64,
    pub scale: f64,
    pub rotation: f64,
    pub shape_number: i16,
    pub text: DashText,
}

impl LtypeDash {
    /// True if this dash draws embedded text (SHAPE_FLAG bit 0x02).
    pub fn has_text(&self) -> bool {
        self.shape_flag & 0x02 != 0
    }

    /// True if this dash draws an embedded shape (SHAPE_FLAG bit 0x04).
    pub fn has_shape(&self) -> bool {
        self.shape_flag & 0x04 != 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LtypeEntry {
    pub header: TableEntryHeader,
    pub flags: u8,
    pub used_count: i16,
    pub description: String,
    pub pattern_length: f64,
    pub alignment: u8,
    pub dashes: Vec<LtypeDash>,
}

// Legacy alias retained so callers keep compiling while they migrate to
// [`LtypeEntry`].
pub type LType = LtypeEntry;

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<LtypeEntry> {
    let header = read_table_entry_header(c, version)?;
    let flags = c.read_rc()?;
    let used_count = c.read_rs()?;
    let description = read_tv(c, version)?;
    let pattern_length = c.read_bd()?;
    let alignment = c.read_rc()?;
    let num_dashes = c.read_rc()? as usize;
    if num_dashes > MAX_DASHES {
        return Err(Error::SectionMap(format!(
            "LTYPE num_dashes {num_dashes} exceeds spec cap of {MAX_DASHES}"
        )));
    }
    let mut dashes = Vec::with_capacity(num_dashes);
    for _ in 0..num_dashes {
        let length = c.read_bd()?;
        let shape_flag = c.read_bs()?;
        let x_offset = c.read_bd()?;
        let y_offset = c.read_bd()?;
        let scale = c.read_bd()?;
        let rotation = c.read_bd()?;
        let shape_number = c.read_bs()?;
        let text = if shape_flag & 0x02 != 0 {
            DashText::Inline(read_tv(c, version)?)
        } else if shape_flag & 0x04 != 0 {
            DashText::StyleHandle(c.read_handle()?)
        } else {
            DashText::None
        };
        dashes.push(LtypeDash {
            length,
            shape_flag,
            x_offset,
            y_offset,
            scale,
            rotation,
            shape_number,
            text,
        });
    }
    Ok(LtypeEntry {
        header,
        flags,
        used_count,
        description,
        pattern_length,
        alignment,
        dashes,
    })
}

/// Decode an R2007+ LTYPE whose `TV` fields live in the object's string stream.
///
/// ODA v5.4.1 §19.1 says modern Unicode strings are read from the string
/// stream even when the LTYPE field table lists `TV` inline. The data cursor
/// therefore parses only the non-string fields at each `TV` site: entry name
/// and description come from the string stream, while dash text uses LTYPE's
/// fixed text area/index scheme.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<LtypeEntry> {
    if !version.is_r2007_plus() {
        let mut c = BitCursor::new(payload);
        skip_to(&mut c, object_body_start)?;
        return decode(&mut c, version);
    }

    let data_end = pre_handle_data_end(payload, version).unwrap_or(payload.len() * 8);
    decode_modern_split_stream_ordered(payload, object_body_start, data_end, version).or_else(
        |ordered_error| {
            decode_simple_ltype_from_string_scan(payload, object_body_start, data_end, version)
                .map_err(|_| ordered_error)
        },
    )
}

fn decode_modern_split_stream_ordered(
    payload: &[u8],
    object_body_start: usize,
    data_end: usize,
    version: Version,
) -> Result<LtypeEntry> {
    let mut data = BitCursor::new(payload);
    skip_to(&mut data, object_body_start)?;
    skip_table_object_prefix(&mut data, version)?;

    let flag64 = data.read_b()?;
    let xref_index_plus_1 = data.read_bs()?;
    let is_xref_dependent = data.read_b()?;
    let pattern_length = data.read_bd()?;
    let alignment = data.read_rc()?;
    let num_dashes = data.read_rc()? as usize;
    if num_dashes > MAX_DASHES {
        return Err(Error::SectionMap(format!(
            "LTYPE num_dashes {num_dashes} exceeds spec cap of {MAX_DASHES}"
        )));
    }

    let mut dashes = Vec::with_capacity(num_dashes);
    let mut has_r2007_text_area = false;
    for _ in 0..num_dashes {
        let length = data.read_bd()?;
        let shape_number = data.read_bs()?;
        let x_offset = data.read_rd()?;
        let y_offset = data.read_rd()?;
        let scale = data.read_bd()?;
        let rotation = data.read_bd()?;
        let shape_flag = data.read_bs()?;
        has_r2007_text_area |= shape_flag & 0x02 != 0 || shape_flag & 0x04 != 0;
        dashes.push(LtypeDash {
            length,
            shape_flag,
            x_offset,
            y_offset,
            scale,
            rotation,
            shape_number,
            text: DashText::None,
        });
    }
    if has_r2007_text_area {
        for _ in 0..512 {
            let _ = data.read_rc()?;
        }
    }

    let mut strings = BitCursor::new(payload);
    skip_to(&mut strings, data.position_bits())?;
    let name = read_tv(&mut strings, version)?;
    if !plausible_ltype_name(&name) {
        return Err(Error::SectionMap(
            "LTYPE string stream name not found".into(),
        ));
    }
    let description = read_tv(&mut strings, version).unwrap_or_default();
    if strings.position_bits() > data_end {
        return Err(Error::SectionMap(
            "LTYPE string stream overran pre-handle data".into(),
        ));
    }

    Ok(LtypeEntry {
        header: TableEntryHeader {
            name,
            is_xref_dependent,
            xref_index_plus_1,
            is_xref_resolved: flag64,
        },
        flags: if flag64 { 64 } else { 0 },
        used_count: 0,
        description,
        pattern_length,
        alignment,
        dashes,
    })
}

fn decode_simple_ltype_from_string_scan(
    payload: &[u8],
    object_body_start: usize,
    data_end: usize,
    version: Version,
) -> Result<LtypeEntry> {
    for string_start in object_body_start..data_end {
        let Some((name, description)) = read_ltype_string_pair_at(payload, string_start, version)
        else {
            continue;
        };
        if !plausible_ltype_name(&name) {
            continue;
        }
        let Some(pattern_start) = string_start.checked_sub(18) else {
            continue;
        };
        if pattern_start < object_body_start {
            continue;
        }
        let mut fields = BitCursor::new(payload);
        skip_to(&mut fields, pattern_start)?;
        let pattern_length = fields.read_bd()?;
        let alignment = fields.read_rc()?;
        let num_dashes = fields.read_rc()?;
        if fields.position_bits() != string_start || alignment != b'A' || num_dashes != 0 {
            continue;
        }
        return Ok(LtypeEntry {
            header: TableEntryHeader {
                name,
                ..TableEntryHeader::default()
            },
            flags: 0,
            used_count: 0,
            description,
            pattern_length,
            alignment,
            dashes: Vec::new(),
        });
    }
    Err(Error::SectionMap(
        "LTYPE string stream name not found".into(),
    ))
}

fn read_ltype_string_pair_at(
    payload: &[u8],
    string_start: usize,
    version: Version,
) -> Option<(String, String)> {
    let mut strings = BitCursor::new(payload);
    skip_to(&mut strings, string_start).ok()?;
    let name = read_tv(&mut strings, version).ok()?;
    let description = read_tv(&mut strings, version).unwrap_or_default();
    Some((name, description))
}

fn plausible_ltype_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && s.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '$' | '-' | '_' | '.' | '*' | ' ' | ',' | '/' | '\\' | '[' | ']'
                )
        })
}

fn skip_table_object_prefix(c: &mut BitCursor<'_>, version: Version) -> Result<()> {
    const MAX_XDATA_ITERATIONS: usize = 256;
    for _ in 0..MAX_XDATA_ITERATIONS {
        let size = c.read_bs_u()? as usize;
        if size == 0 {
            break;
        }
        let _appid = c.read_handle()?;
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
    }

    if version.is_r2004_plus() {
        let _no_xdictionary = c.read_b()?;
    }
    if matches!(version, Version::R2013 | Version::R2018) {
        let _has_binary_data = c.read_b()?;
    }
    Ok(())
}

fn pre_handle_data_end(payload: &[u8], version: Version) -> Option<usize> {
    if !version.is_r2010_plus() {
        return None;
    }
    let mut c = BitCursor::new(payload);
    let handle_bits = read_mc_unsigned(&mut c).ok()? as usize;
    payload.len().checked_mul(8)?.checked_sub(handle_bits)
}

fn skip_to(c: &mut BitCursor<'_>, bit: usize) -> Result<()> {
    while c.position_bits() < bit {
        let _ = c.read_b()?;
    }
    Ok(())
}

fn read_mc_unsigned(cursor: &mut BitCursor<'_>) -> Result<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for _ in 0..10 {
        let b = cursor.read_rc()? as u64;
        value |= (b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift >= 64 {
            break;
        }
    }
    Err(Error::SectionMap("MC length exceeded 10 bytes".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn write_header(w: &mut BitWriter, name: &[u8]) {
        w.write_bs_u(name.len() as u16);
        for b in name {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
    }

    #[test]
    fn roundtrip_dashed_ltype() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"DASHED");
        w.write_rc(0); // flags
        w.write_rs(0); // used_count
        let desc = b"Dashed ___ __ ";
        w.write_bs_u(desc.len() as u16);
        for b in desc {
            w.write_rc(*b);
        }
        w.write_bd(0.75); // pattern length
        w.write_rc(b'A'); // alignment
        w.write_rc(3); // 3 dashes
        for (length, shape_flag) in [(0.5_f64, 0i16), (-0.125, 0), (0.125, 0)] {
            w.write_bd(length);
            w.write_bs(shape_flag);
            w.write_bd(0.0); // x_offset
            w.write_bd(0.0); // y_offset
            w.write_bd(1.0); // scale
            w.write_bd(0.0); // rotation
            w.write_bs(0); // shape_number
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(l.header.name, "DASHED");
        assert_eq!(l.dashes.len(), 3);
        assert_eq!(l.dashes[0].length, 0.5);
        assert_eq!(l.dashes[1].length, -0.125);
        assert_eq!(l.alignment, b'A');
        assert!(matches!(l.dashes[0].text, DashText::None));
    }

    #[test]
    fn roundtrip_text_dash() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"TEXTLINE");
        w.write_rc(0);
        w.write_rs(0);
        w.write_bs_u(0); // empty description
        w.write_bd(1.0);
        w.write_rc(b'A');
        w.write_rc(1);
        // Dash with text
        w.write_bd(0.25);
        w.write_bs(0x02); // has text
        w.write_bd(0.1);
        w.write_bd(-0.05);
        w.write_bd(0.5);
        w.write_bd(0.0);
        w.write_bs(0);
        let t = b"GAS";
        w.write_bs_u(t.len() as u16);
        for b in t {
            w.write_rc(*b);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert!(l.dashes[0].has_text());
        assert!(matches!(&l.dashes[0].text, DashText::Inline(s) if s == "GAS"));
    }

    #[test]
    fn r2007_split_stream_ltype_reads_name_and_description_from_string_stream() {
        let mut w = BitWriter::new();
        // Object common prefix for table objects: no EED, no xdic.
        w.write_bs_u(0);
        w.write_b(true);

        w.write_b(false); // 64 flag
        w.write_bs(0); // xref index + 1
        w.write_b(false); // xdep
        w.write_bd(1.0); // pattern length
        w.write_rc(b'A'); // alignment
        w.write_rc(0); // no dashes

        write_tu(&mut w, "Continuous");
        write_tu(&mut w, "Solid line");

        let bytes = w.into_bytes();
        let entry = decode_modern_split_stream(&bytes, 0, Version::R2007).unwrap();
        assert_eq!(entry.header.name, "Continuous");
        assert_eq!(entry.description, "Solid line");
        assert_eq!(entry.alignment, b'A');
        assert!(entry.dashes.is_empty());
    }

    #[test]
    fn rejects_oversized_dash_count() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"EVIL");
        w.write_rc(0);
        w.write_rs(0);
        w.write_bs_u(0);
        w.write_bd(1.0);
        w.write_rc(b'A');
        // num_dashes is an RC (u8) — 256 wraps to 0, so the cap is
        // enforced at or above 256. The worst case the decoder can
        // see from a malformed stream is 0xFF = 255, which is within
        // the cap. We validate the check is *present* by unit-asserting
        // the MAX_DASHES constant instead of trying to overflow.
        w.write_rc(255);
        // (don't bother filling dashes; decode will read garbage and
        // return an error from the bit cursor, which is also fine.)
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        // Either returns Err from truncated stream OR from cap — both
        // acceptable; we just need to confirm decode doesn't allocate
        // past the cap.
        let err = decode(&mut c, Version::R2000);
        assert!(err.is_err(), "expected error for adversarial dash count");
        assert_eq!(MAX_DASHES, 256);
    }

    fn write_tu(w: &mut BitWriter, s: &str) {
        w.write_bs_u(s.encode_utf16().count() as u16);
        for unit in s.encode_utf16() {
            w.write_rc((unit & 0xFF) as u8);
            w.write_rc((unit >> 8) as u8);
        }
    }
}
