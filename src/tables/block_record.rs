//! BLOCK_HEADER (aka BLOCK_RECORD) table entry (§19.5.51) — the
//! authoritative record for a block definition. Holds the block's
//! name, base point, and the handles of its first/last entities.
//!
//! # Stream shape
//!
//! ```text
//! entry header (name + xref bits)
//! B      is_anonymous
//! B      has_attribs
//! B      is_xref           -- block references external file
//! B      xref_overlay      -- for xref blocks, overlay vs attach
//! B      is_loaded_xref    -- xref has resolved
//! (R2004+)
//!   BL   num_owned_objects
//! BD3    base_point
//! TV     xref_path         -- filesystem path for xref blocks
//! (R2004+)
//!   RC*  insert_count_bytes  -- until 0x00 terminator; lets older
//!                               readers skip the count
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::{Error, Result};
use crate::tables::{TableEntryHeader, read_table_entry_header, read_tv};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct BlockRecord {
    pub header: TableEntryHeader,
    pub is_anonymous: bool,
    pub has_attribs: bool,
    pub is_xref: bool,
    pub xref_overlay: bool,
    pub is_loaded_xref: bool,
    pub num_owned_objects: Option<u32>,
    pub base_point: Point3D,
    pub xref_path: String,
}

#[derive(Debug, Clone)]
struct ModernBlockFields {
    header: TableEntryHeader,
    is_anonymous: bool,
    has_attribs: bool,
    is_xref: bool,
    xref_overlay: bool,
    is_loaded_xref: bool,
    num_owned_objects: Option<u32>,
    base_point: Point3D,
    string_start: usize,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<BlockRecord> {
    let header = read_table_entry_header(c, version)?;
    let is_anonymous = c.read_b()?;
    let has_attribs = c.read_b()?;
    let is_xref = c.read_b()?;
    let xref_overlay = c.read_b()?;
    let is_loaded_xref = c.read_b()?;
    let num_owned_objects = if version.is_r2004_plus() {
        Some(c.read_bl()? as u32)
    } else {
        None
    };
    let base_point = read_bd3(c)?;
    let xref_path = read_tv(c, version)?;
    Ok(BlockRecord {
        header,
        is_anonymous,
        has_attribs,
        is_xref,
        xref_overlay,
        is_loaded_xref,
        num_owned_objects,
        base_point,
        xref_path,
    })
}

/// Decode an R2007+ BLOCK_HEADER whose TV fields live in the object's
/// trailing string stream.
///
/// Public ODA spec v5.4.1 §19.1/§19.4.50 says Unicode strings in R2007+
/// objects are stored in a separate string stream even though the object
/// field table still lists the `TV` fields inline. This keeps the legacy
/// [`decode`] path intact for inline/synthetic streams and gives the object
/// dispatcher a clean-room split-stream path for real modern DWGs.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<BlockRecord> {
    if !version.is_r2007_plus() {
        let mut c = BitCursor::new(payload);
        skip_to(&mut c, object_body_start)?;
        return decode(&mut c, version);
    }

    let data_end = pre_handle_data_end(payload, version).unwrap_or(payload.len() * 8);
    let mut data = BitCursor::new(payload);
    skip_to(&mut data, object_body_start)?;
    skip_table_object_prefix(&mut data, version)?;
    let data_start = data.position_bits();

    let parsed_fields = parse_modern_block_fields(&mut data, version).ok();
    let string_start = parsed_fields
        .as_ref()
        .and_then(|fields| {
            if plausible_tv_at(payload, fields.string_start, data_end, version) {
                Some(fields.string_start)
            } else {
                None
            }
        })
        .or_else(|| find_block_name_string_start(payload, data_start, data_end, version))
        .ok_or_else(|| Error::SectionMap("BLOCK_HEADER string stream not found".into()))?;

    let mut strings = BitCursor::new(payload);
    skip_to(&mut strings, string_start)?;
    let name = read_tv(&mut strings, version)?;
    let xref_path = read_tv(&mut strings, version).unwrap_or_default();
    let _description = read_tv(&mut strings, version).unwrap_or_default();

    let mut fields = parsed_fields.unwrap_or_else(|| ModernBlockFields {
        header: TableEntryHeader::default(),
        is_anonymous: false,
        has_attribs: false,
        is_xref: false,
        xref_overlay: false,
        is_loaded_xref: false,
        num_owned_objects: None,
        base_point: Point3D {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        string_start,
    });
    fields.header.name = name;

    Ok(BlockRecord {
        header: fields.header,
        is_anonymous: fields.is_anonymous,
        has_attribs: fields.has_attribs,
        is_xref: fields.is_xref,
        xref_overlay: fields.xref_overlay,
        is_loaded_xref: fields.is_loaded_xref,
        num_owned_objects: fields.num_owned_objects,
        base_point: fields.base_point,
        xref_path,
    })
}

fn parse_modern_block_fields(c: &mut BitCursor<'_>, version: Version) -> Result<ModernBlockFields> {
    let flag64 = c.read_b()?;
    let xref_index_plus_1 = c.read_bs()?;
    let is_xref_dependent = c.read_b()?;
    let is_anonymous = c.read_b()?;
    let has_attribs = c.read_b()?;
    let is_xref = c.read_b()?;
    let xref_overlay = c.read_b()?;
    let is_loaded_xref = if !matches!(version, Version::R14) {
        c.read_b()?
    } else {
        false
    };
    let num_owned_objects = if version.is_r2004_plus() {
        Some(c.read_bl()? as u32)
    } else {
        None
    };
    let base_point = read_bd3(c)?;
    if !matches!(version, Version::R14) {
        // Insert-count bytes: zero or more non-zero RC values, then 0.
        for _ in 0..=256 {
            if c.read_rc()? == 0 {
                break;
            }
        }
    }
    if !matches!(version, Version::R14) {
        let preview_bytes = c.read_bl_u()? as usize;
        if preview_bytes > 1_000_000 {
            return Err(Error::SectionMap(format!(
                "BLOCK_HEADER preview data claims {preview_bytes} bytes"
            )));
        }
        for _ in 0..preview_bytes {
            let _ = c.read_rc()?;
        }
    }
    if version.is_r2007_plus() {
        let _insert_units = c.read_bs()?;
        let _explodable = c.read_b()?;
        let _block_scaling = c.read_rc()?;
    }

    Ok(ModernBlockFields {
        header: TableEntryHeader {
            name: String::new(),
            is_xref_dependent,
            xref_index_plus_1,
            is_xref_resolved: flag64,
        },
        is_anonymous,
        has_attribs,
        is_xref,
        xref_overlay,
        is_loaded_xref,
        num_owned_objects,
        base_point,
        string_start: c.position_bits(),
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

fn find_block_name_string_start(
    payload: &[u8],
    start_bit: usize,
    end_bit: usize,
    version: Version,
) -> Option<usize> {
    let mut best: Option<(i32, usize)> = None;
    for bit in start_bit..end_bit {
        let Some((name, next)) = read_tv_at(payload, bit, version) else {
            continue;
        };
        if next > end_bit || !is_plausible_block_name(&name) {
            continue;
        }
        let score = block_name_score(&name);
        match best {
            Some((best_score, _)) if best_score >= score => {}
            _ => best = Some((score, bit)),
        }
    }
    best.map(|(_, bit)| bit)
}

fn plausible_tv_at(payload: &[u8], bit: usize, end_bit: usize, version: Version) -> bool {
    read_tv_at(payload, bit, version)
        .map(|(name, next)| next <= end_bit && is_plausible_block_name(&name))
        .unwrap_or(false)
}

fn read_tv_at(payload: &[u8], bit: usize, version: Version) -> Option<(String, usize)> {
    let mut c = BitCursor::new(payload);
    skip_to(&mut c, bit).ok()?;
    let s = read_tv(&mut c, version).ok()?;
    Some((s, c.position_bits()))
}

fn is_plausible_block_name(name: &str) -> bool {
    if name.is_empty() || name.chars().count() > 255 {
        return false;
    }
    name.chars().all(|ch| {
        ch == '-'
            || ch == '_'
            || ch == '.'
            || ch == '$'
            || ch == '*'
            || ch == '{'
            || ch == '}'
            || ch.is_ascii_alphanumeric()
    })
}

fn block_name_score(name: &str) -> i32 {
    let mut score = 0;
    if name.starts_with("*Model_Space") || name.starts_with("*Paper_Space") {
        score += 1000;
    } else if name.starts_with('*') {
        score += 600;
    } else if name.starts_with('_') {
        score += 450;
    } else if name.starts_with('{') && name.ends_with('}') {
        score += 400;
    } else if name
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric())
    {
        score += 300;
    }
    score + name.chars().count().min(255) as i32
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

    #[test]
    fn roundtrip_model_space_block_record_r2004() {
        let mut w = BitWriter::new();
        let s = b"*Model_Space";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
        // 5 flag bits — all false
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
        // R2004+: num_owned_objects
        w.write_bl(42);
        // base point at origin
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // empty xref path
        w.write_bs_u(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode(&mut c, Version::R2004).unwrap();
        assert_eq!(b.header.name, "*Model_Space");
        assert_eq!(b.num_owned_objects, Some(42));
        assert!(!b.is_xref);
    }

    #[test]
    fn r2007_split_stream_block_record_reads_name_from_string_stream() {
        let mut w = BitWriter::new();
        // Object common prefix for table objects: no EED, no xdic.
        w.write_bs_u(0);
        w.write_b(true);

        w.write_b(true); // 64 flag
        w.write_bs(1); // xref index + 1
        w.write_b(false); // xdep
        w.write_b(false); // anonymous
        w.write_b(false); // has attribs
        w.write_b(false); // is xref
        w.write_b(false); // overlay
        w.write_b(false); // loaded bit
        w.write_bl(0); // owned objects
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_rc(0); // insert count terminator
        w.write_bl(0); // preview bytes
        w.write_bs(0); // insert units
        w.write_b(true); // explodable
        w.write_rc(0); // scaling

        write_tu(&mut w, "*Paper_Space");
        w.write_bs_u(0); // xref path
        w.write_bs_u(0); // description

        let bytes = w.into_bytes();
        let block = decode_modern_split_stream(&bytes, 0, Version::R2007).unwrap();
        assert_eq!(block.header.name, "*Paper_Space");
        assert_eq!(block.xref_path, "");
        assert_eq!(block.num_owned_objects, Some(0));
        assert!(block.header.is_xref_resolved);
        assert!(
            block.base_point.x == 0.0 && block.base_point.y == 0.0 && block.base_point.z == 0.0
        );
    }

    fn write_tu(w: &mut BitWriter, s: &str) {
        w.write_bs_u(s.encode_utf16().count() as u16);
        for unit in s.encode_utf16() {
            w.write_rc((unit & 0xFF) as u8);
            w.write_rc((unit >> 8) as u8);
        }
    }
}
