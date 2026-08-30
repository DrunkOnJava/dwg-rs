//! BLOCK_HEADER (aka BLOCK_RECORD) table entry (ODA spec v5.4.1
//! §20.4.52) — the record for a block definition. Holds the block's
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
//!   TV   description
//!   BL   preview_bytes + that many RC
//! (R2007+)
//!   BS   insert_units
//!   B    explodable
//!   RC   block_scaling
//! ```
//!
//! # The stored name is a stem for auto-generated blocks (#70)
//!
//! On R2007+ the entry name lives in the record's string stream
//! (§19.1) as the first of three `TV` slots — name, xref path,
//! description. That slot is **not** always the block's full name.
//! For blocks AutoCAD names itself — the anonymous `*D<n>` / `*T<n>` /
//! `*U<n>` families and the second and later `*Paper_Space<n>` layout
//! blocks — the BLOCK_HEADER stores only the stem and the generated
//! numeric suffix appears solely on the `BLOCK` sentinel entity that
//! opens the definition.
//!
//! ## Measured, then ground-truthed
//!
//! `examples/probe_block_names.rs` reads both records' string streams
//! positionally. On `arc_2013.dwg` the two BLOCK_HEADERs at handles
//! 0x6C and 0x74 are byte-identical apart from their own handle and
//! their handle streams; both store the 12-unit string `*Paper_Space`
//! in a string stream whose §19.1 trailer declares exactly 206 bits
//! (10-bit `BS` length + 192 bits of UTF-16 + two empty `TV` slots at
//! 2 bits each). There is no room for, and no other copy of, a
//! suffix: a 13-unit `*Paper_Space0` would need 222. Their `BLOCK`
//! sentinels at 0x6D and 0x75 read `*Paper_Space` and
//! `*Paper_Space0`.
//!
//! AutoCAD's own DXF twin of that file — `arc_2013.dxf` in the public
//! `nextgis/dwg_samples` repository, the same drawing exported by the
//! producer — names the two BLOCK_RECORDs `*Paper_Space` (handle
//! `6C`) and `*Paper_Space0` (handle `74`). The `BLOCK` sentinel is
//! therefore the authority and the BLOCK_HEADER slot is the stem.
//! The same split holds on all 27 block definitions of
//! `sample_AC1032.dwg`, where the header stem is a prefix of the
//! sentinel name in 27/27 cases and the remainder is decimal digits
//! every time.
//!
//! A decoder cannot recover the suffix from the BLOCK_HEADER alone, so
//! [`BlockRecord::block_sentinel_handle`] surfaces the handle the
//! record's own handle stream gives the `BLOCK` entity and
//! [`crate::graph::resolve_block_names`] performs the join.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::{Error, Result};
use crate::string_stream;
use crate::tables::{TableEntryHeader, modern, read_table_entry_header, read_tv};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct BlockRecord {
    /// The record's own stored name. For auto-generated blocks this is
    /// only the stem — see the module docs and
    /// [`Self::block_sentinel_handle`].
    pub header: TableEntryHeader,
    pub is_anonymous: bool,
    pub has_attribs: bool,
    pub is_xref: bool,
    pub xref_overlay: bool,
    pub is_loaded_xref: bool,
    pub num_owned_objects: Option<u32>,
    pub base_point: Point3D,
    pub xref_path: String,
    /// Block description (`TV`, R2004+); empty when the block has none.
    pub description: String,
    /// Handle of the `BLOCK` sentinel entity that opens this
    /// definition, read from the record's own handle stream
    /// (§20.4.52). `None` before R2010, where this crate does not
    /// locate the handle stream.
    ///
    /// The sentinel carries the block's full name; this record's
    /// [`header`](Self::header) name may be only its stem.
    pub block_sentinel_handle: Option<u64>,
}

/// Cap on the insert-count run of §20.4.52: a non-zero `RC` per insert
/// handle, terminated by a zero. No realistic block carries more.
const MAX_INSERT_COUNT: usize = 256;

/// Cap on the §20.4.52 binary preview blob of one BLOCK_HEADER.
const MAX_PREVIEW_BYTES: usize = 1_000_000;

/// Decodes a `BlockRecord` table entry that follows the common object header.
///
/// # Field list (§20.4.52)
///
/// ```text
/// TV   entry name        -- via read_table_entry_header
/// B    64-flag, BS xrefindex+1, B xdep
/// B    anonymous, B hasatts, B blkisxref, B xrefoverlaid
/// R2000+: B loaded bit
/// R2004+: BL owned object count
/// 3BD  base point
/// TV   xref pathname
/// R2000+: RC* insert count (non-zero bytes, terminated by a zero RC)
///         TV block description
///         BL size of preview data, then that many RC
/// R2007+: BS insert units, B explodable, RC block scaling
/// ```
///
/// # Measured
///
/// The three R2000 and three R2004 BLOCK_HEADER records of every corpus
/// file ended their field list exactly **12 bits** before their
/// data-stream boundary until the R2000+ tail above was added: the
/// terminating zero `RC` of the insert count is 8 bits, an empty `TV`
/// description is a `BS` on the `10` (value 0) code — 2 bits — and a
/// zero preview size is a `BL` on the same code, another 2. All six now
/// close on delta 0. The `B loaded bit` is likewise R2000+; reading it
/// on R14 ran those records off the end of their payload.
///
/// This is the inline layout, so
/// [`BlockRecord::block_sentinel_handle`] is `None` on this path — the
/// handle stream is only located from R2010 on.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<BlockRecord> {
    let header = read_table_entry_header(c, version)?;
    let is_anonymous = c.read_b()?;
    let has_attribs = c.read_b()?;
    let is_xref = c.read_b()?;
    let xref_overlay = c.read_b()?;
    let is_loaded_xref = if matches!(version, Version::R14) {
        false
    } else {
        c.read_b()?
    };
    let num_owned_objects = if version.is_r2004_plus() {
        Some(c.read_bl()? as u32)
    } else {
        None
    };
    let base_point = read_bd3(c)?;
    let xref_path = read_tv(c, version)?;
    let mut description = String::new();
    if !matches!(version, Version::R14) {
        // Insert-count run: zero or more non-zero `RC`s, then a zero.
        for _ in 0..=MAX_INSERT_COUNT {
            if c.read_rc()? == 0 {
                break;
            }
        }
        description = read_tv(c, version)?;
        let preview_bytes = c.read_bl_u()? as usize;
        if preview_bytes > MAX_PREVIEW_BYTES {
            return Err(Error::SectionMap(format!(
                "BLOCK_HEADER preview data claims {preview_bytes} bytes \
                 (>{MAX_PREVIEW_BYTES} sanity cap)"
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
        description,
        block_sentinel_handle: None,
    })
}

/// Decode an R2007+ BLOCK_HEADER whose `TV` fields live in the
/// object's trailing string stream (§19.1 + §20.4.52).
///
/// The data-stream fields are held to the string-stream boundary by
/// [`modern::SplitStream::finish`], so a mis-read field surfaces as an
/// error instead of a plausible-looking name. All 27 BLOCK_HEADER
/// records of `sample_AC1032.dwg` (R2018) and all 3 of each of
/// `arc_2010.dwg`, `arc_2013.dwg` and `line_2013.dwg` close with
/// delta 0 under this field list.
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

    let mut split = modern::open_table_entry(payload, object_body_start, version)?;
    let (flag64, xref_index_plus_1, is_xref_dependent) = modern::read_entry_flags(&mut split.data)?;
    let is_anonymous = split.data.read_b()?;
    let has_attribs = split.data.read_b()?;
    let is_xref = split.data.read_b()?;
    let xref_overlay = split.data.read_b()?;
    let is_loaded_xref = split.data.read_b()?;
    let num_owned_objects = if version.is_r2004_plus() {
        Some(split.data.read_bl()? as u32)
    } else {
        None
    };
    let base_point = read_bd3(&mut split.data)?;
    read_insert_count(&mut split.data)?;
    read_preview(&mut split.data)?;
    let _insert_units = split.data.read_bs()?;
    let _explodable = split.data.read_b()?;
    let _block_scaling = split.data.read_rc()?;
    split.finish("BLOCK_HEADER")?;

    let name = split.strings.read_tv()?;
    let xref_path = split.strings.read_tv()?;
    let description = split.strings.read_tv()?;

    Ok(BlockRecord {
        header: TableEntryHeader {
            name,
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
        xref_path,
        description,
        block_sentinel_handle: block_sentinel_handle(payload, version),
    })
}

/// Skip the insert-count run: zero or more non-zero `RC` values
/// terminated by `0x00` (§20.4.52).
fn read_insert_count(c: &mut BitCursor<'_>) -> Result<()> {
    for _ in 0..=256 {
        if c.read_rc()? == 0 {
            return Ok(());
        }
    }
    Ok(())
}

/// Skip the binary preview block: a `BL` byte count and that many `RC`.
fn read_preview(c: &mut BitCursor<'_>) -> Result<()> {
    let preview_bytes = c.read_bl_u()? as usize;
    for _ in 0..preview_bytes {
        let _ = c.read_rc()?;
    }
    Ok(())
}

/// Handle of the `BLOCK` sentinel entity a BLOCK_HEADER's handle
/// stream names, or `None` when the stream does not have the shape
/// §20.4.52 prescribes.
///
/// # Why this exists
///
/// The sentinel carries the block's full name where the BLOCK_HEADER
/// stores only a stem for auto-generated blocks (module docs, #70), so
/// resolving the real name needs this link.
///
/// # Measured
///
/// §20.4.52 lists the record's handle references in the order block
/// control (owner), reactors, xdictionary, a NULL handle, then the
/// BLOCK entity. The NULL is a hard pointer with a zero-byte counter
/// (`code 5`, `counter 0`), which makes it a self-identifying anchor:
/// scanning forward to it and taking the next reference needs no
/// reactor count and no xdictionary flag. On `sample_AC1032.dwg` that
/// rule names a record the walker classifies as `BLOCK` for 27 of 27
/// BLOCK_HEADERs, and 3 of 3 on each of `arc_2010.dwg`,
/// `arc_2013.dwg`, `circle_2010.dwg` and `line_2013.dwg`.
pub fn block_sentinel_handle(payload: &[u8], version: Version) -> Option<u64> {
    // References past the NULL anchor are a mis-read, not a longer
    // prologue; bound the scan so garbage terminates.
    const MAX_SCAN: usize = 16;

    let start = string_stream::data_section_end(payload, version)?;
    let mut c = BitCursor::new(payload);
    string_stream::seek(&mut c, start).ok()?;
    let mut saw_null = false;
    for _ in 0..MAX_SCAN {
        if c.remaining_bits() < 8 {
            return None;
        }
        let h = c.read_handle().ok()?;
        if saw_null {
            // Only an absolute reference names a record directly; a
            // relative one would need the owner's handle, which this
            // function does not have.
            return h.is_absolute().then_some(h.value);
        }
        saw_null = h.code == 5 && h.counter == 0;
    }
    None
}

fn skip_to(c: &mut BitCursor<'_>, bit: usize) -> Result<()> {
    while c.position_bits() < bit {
        let _ = c.read_b()?;
    }
    Ok(())
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
        // 4 block flags + the R2000+ loaded bit — all false
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
        // R2000+ tail: insert-count terminator, description, preview size
        w.write_rc(0);
        w.write_bs_u(0);
        w.write_bl(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode(&mut c, Version::R2004).unwrap();
        assert_eq!(b.header.name, "*Model_Space");
        assert_eq!(b.num_owned_objects, Some(42));
        assert!(!b.is_xref);
    }

    /// §20.4.52 puts the loaded bit, the insert-count run, the
    /// description and the preview blob under "R2000+"; an R14 record
    /// stops after the xref pathname.
    #[test]
    fn r14_block_record_stops_after_the_xref_path() {
        let mut w = BitWriter::new();
        let s = b"*MODEL_SPACE";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
        w.write_b(false); // anonymous
        w.write_b(false); // has attribs
        w.write_b(false); // is xref
        w.write_b(false); // xref overlaid
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bs_u(0); // empty xref path
        let end = w.position_bits();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode(&mut c, Version::R14).unwrap();
        assert_eq!(b.header.name, "*MODEL_SPACE");
        assert_eq!(b.num_owned_objects, None);
        assert!(!b.is_loaded_xref);
        assert_eq!(c.position_bits(), end);
    }

    /// Body bits of a minimal R2018 BLOCK_HEADER, from the object's
    /// common prefix through the last data-stream field.
    fn block_header_body(name_is_anonymous: bool) -> Vec<bool> {
        let mut w = BitWriter::new();
        w.write_bs_u(0); // no EED
        w.write_b(true); // no xdictionary
        w.write_b(false); // no AcDs binary data
        w.write_b(true); // 64-flag
        w.write_b(false); // xref dependent
        w.write_bs(1); // xref index + 1
        w.write_b(name_is_anonymous); // anonymous
        w.write_b(false); // has attribs
        w.write_b(false); // is xref
        w.write_b(false); // overlay
        w.write_b(false); // loaded bit
        w.write_bl(0); // owned objects
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_rc(0); // insert-count terminator
        w.write_bl(0); // preview bytes
        w.write_bs(0); // insert units
        w.write_b(true); // explodable
        w.write_rc(0); // block scaling
        crate::string_stream::tests::bits_of(&w)
    }

    #[test]
    fn r2007_split_stream_block_record_reads_name_from_string_stream() {
        let payload = crate::string_stream::tests::build_payload(
            &block_header_body(false),
            &["*Paper_Space", "", "a description"],
        );
        let block = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(block.header.name, "*Paper_Space");
        assert_eq!(block.xref_path, "");
        assert_eq!(block.description, "a description");
        assert_eq!(block.num_owned_objects, Some(0));
        assert!(block.header.is_xref_resolved);
        assert!(!block.header.is_xref_dependent);
        assert_eq!(block.header.xref_index_plus_1, 1);
        assert!(block.base_point.x == 0.0 && block.base_point.y == 0.0 && block.base_point.z == 0.0);
    }

    /// A data field read one bit wide leaves the cursor off the string
    /// stream, and [`modern::SplitStream::finish`] must reject that
    /// instead of returning a plausible-looking name. Feeding the
    /// R2018 decoder a body that is one bit short is the cheapest way
    /// to prove the boundary is enforced.
    #[test]
    fn a_misaligned_block_header_body_is_rejected() {
        let mut body = block_header_body(false);
        body.pop();
        let payload = crate::string_stream::tests::build_payload(&body, &["*Model_Space", "", ""]);
        let err = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap_err();
        assert!(
            format!("{err}").contains("BLOCK_HEADER data fields ended at bit"),
            "unexpected error: {err}"
        );
    }

    /// Build an R2018 object payload with a real handle stream: the
    /// leading `MC`, the data body, the string stream and its §19.1
    /// trailer, then `handles` starting on the trailer's very next bit.
    ///
    /// `crate::string_stream::tests::build_payload` pads the trailer out
    /// to a byte boundary and calls that filler the handle stream,
    /// which is enough to exercise the string reader but leaves no
    /// decodable handle references. This helper packs real ones.
    fn build_payload_with_handles(body: &[bool], strings: &[&str], handles: &[u8]) -> Vec<u8> {
        let mut sw = BitWriter::new();
        for s in strings {
            sw.write_bs_u(s.encode_utf16().count() as u16);
            for unit in s.encode_utf16() {
                sw.write_rc((unit & 0xFF) as u8);
                sw.write_rc((unit >> 8) as u8);
            }
        }
        let string_bits = sw.position_bits();
        let string_bytes = sw.into_bytes();

        let mut w = BitWriter::new();
        w.write_rc(0x00); // MC placeholder, patched below
        for bit in body {
            w.write_b(*bit);
        }
        for i in 0..string_bits {
            w.write_b((string_bytes[i / 8] >> (7 - (i % 8))) & 1 != 0);
        }
        w.write_rs(string_bits as i16);
        w.write_b(true);
        let trailer_end = w.position_bits();
        for b in handles {
            w.write_rc(*b);
        }
        let pad = (8 - w.position_bits() % 8) % 8;
        for _ in 0..pad {
            w.write_b(false);
        }
        let total_bits = w.position_bits();
        let mut bytes = w.into_bytes();
        // `data_section_end` reads total - MC + mc_field_bits, so the
        // recorded MC must be `total - trailer_end + 8`.
        let mc = total_bits - trailer_end + 8;
        assert!(mc < 0x80, "test helper only builds single-byte MC values");
        bytes[0] = mc as u8;
        bytes
    }

    /// §20.4.52 puts a NULL handle (hard pointer, zero-byte counter)
    /// immediately before the BLOCK entity handle, so the NULL is a
    /// self-identifying anchor.
    #[test]
    fn block_sentinel_handle_follows_the_null_anchor() {
        // owner (code 4, one value byte), NULL (code 5, no bytes),
        // BLOCK entity (code 3, two value bytes = 0x0509).
        let payload = build_payload_with_handles(
            &block_header_body(true),
            &["*D", "", ""],
            &[0x41, 0x01, 0x50, 0x32, 0x05, 0x09],
        );
        let block = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(block.header.name, "*D");
        assert_eq!(block.block_sentinel_handle, Some(0x0509));
        assert_eq!(
            block_sentinel_handle(&payload, Version::R2018),
            Some(0x0509)
        );
    }

    /// An xdictionary reference sits between the owner and the NULL on
    /// records that carry one; the anchor rule must skip it.
    #[test]
    fn block_sentinel_handle_skips_an_xdictionary_reference() {
        let payload = build_payload_with_handles(
            &block_header_body(false),
            &["MyBlock", "", ""],
            &[0x41, 0x01, 0x32, 0x06, 0xF3, 0x50, 0x32, 0x06, 0xF4],
        );
        assert_eq!(
            block_sentinel_handle(&payload, Version::R2018),
            Some(0x06F4)
        );
    }

    /// No NULL anchor in the stream means no claim is made.
    #[test]
    fn block_sentinel_handle_is_none_without_the_anchor() {
        let payload = build_payload_with_handles(
            &block_header_body(false),
            &["x", "", ""],
            &[0x41, 0x01, 0x41, 0x02],
        );
        assert_eq!(block_sentinel_handle(&payload, Version::R2018), None);
    }

    fn write_tu(w: &mut BitWriter, s: &str) {
        w.write_bs_u(s.encode_utf16().count() as u16);
        for unit in s.encode_utf16() {
            w.write_rc((unit & 0xFF) as u8);
            w.write_rc((unit >> 8) as u8);
        }
    }
}
