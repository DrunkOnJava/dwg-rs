//! MTEXT entity (§19.4.23) — multi-line text.
//!
//! Unlike TEXT, MTEXT stores a formatted paragraph that may span
//! multiple lines, with embedded style codes (\\L for underline,
//! \\O for overline, \\S for stacked fractions, etc.) interpreted by
//! the renderer. The stream includes the insertion point, the text
//! direction vector, the rectangular width, the nominal text
//! height, and the string itself.
//!
//! # Stream shape (R2000+)
//!
//! ```text
//! BD3  insertion_point
//! BD3  extrusion
//! BD3  x_axis_direction    -- unit-length vector along text baseline
//! BD   rect_width           -- column width (text wraps to this)
//! (R2007+)
//!   BD   rect_height         -- for auto-height columns
//! BD   nominal_text_height   -- per-line height
//! BS   attachment_point     -- 1..9, alignment in the bounding box
//! BS   drawing_direction    -- left-to-right / right-to-left / ...
//! BD   extents_height        -- actual rendered height
//! BD   extents_width         -- actual rendered width (after layout)
//! TV   text_string           -- with embedded MTEXT control codes
//! BS   linespace_style        -- R2000+
//! BD   linespace_factor       -- R2000+
//! B    unknown_b              -- R2000+ (spec calls it "unknown bit")
//! (R2004+)
//!   BL   background_flags
//!   (if background_flags & 0x01)
//!     BD   background_scale_factor
//!     CMC  background_color
//!     BL   background_transparency
//! ```
//!
//! [`decode`] reads through `text_string` + `linespace_style/factor +
//! unknown_b`, which covers the fields viewers actually render, and is
//! the pre-R2007 path. R2007+ files go through
//! [`decode_modern_split_stream`], which additionally consumes the
//! background block because the split-stream layout is validated
//! against a bit position and so cannot stop early.
//!
//! # Measured — `background_scale_factor` is a `BD`
//!
//! It is the only MTEXT record in `sample_AC1032.dwg` with the
//! background bit set (handle `0x6D8`, text `"\A1;7.0711"`). Reading
//! the field as a `BL` leaves the following `CMC` on a 16-bit code
//! yielding colour index 768 and then a reserved `11` bit pattern;
//! reading it as a `BD` recovers `1.25` (bytes `00 00 00 00 00 00 F4
//! 3F` at bit 559) and the record decodes.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};
use crate::string_stream;
use crate::tables::modern;
use crate::version::Version;

/// Cap on the R2018+ per-column height list (§20.4.46 `BD 46`).
pub const MAX_COLUMN_HEIGHTS: usize = 4_096;

#[derive(Debug, Clone, PartialEq)]
pub struct MText {
    pub insertion_point: Point3D,
    pub extrusion: Vec3D,
    pub x_axis_direction: Vec3D,
    pub rect_width: f64,
    pub rect_height: Option<f64>,
    pub nominal_text_height: f64,
    pub attachment_point: i16,
    pub drawing_direction: i16,
    pub extents_height: f64,
    pub extents_width: f64,
    pub text: String,
    pub linespace_style: i16,
    pub linespace_factor: f64,
    /// R2018+ `B` — the record stores "is NOT annotative"; this is its
    /// negation. `true` on pre-R2018 records, which do not carry it.
    pub is_annotative: bool,
    /// R2018+ `BS 71` column type: 0 none, 1 static, 2 dynamic.
    pub column_type: i16,
    /// R2018+ `BD 44` column width (0 when there are no columns).
    pub column_width: f64,
    /// R2018+ `BD 45` column gutter (0 when there are no columns).
    pub column_gutter: f64,
    /// R2018+ `BD 46` per-column heights — written only for dynamic
    /// columns that are not auto-height.
    pub column_heights: Vec<f64>,
}

/// Decodes the `MText` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<MText> {
    let insertion_point = read_bd3(c)?;
    let extrusion = read_bd3(c)?;
    let x_axis_direction = read_bd3(c)?;
    let rect_width = c.read_bd()?;
    let rect_height = if version.is_r2007_plus() {
        Some(c.read_bd()?)
    } else {
        None
    };
    let nominal_text_height = c.read_bd()?;
    let attachment_point = c.read_bs()?;
    let drawing_direction = c.read_bs()?;
    let extents_height = c.read_bd()?;
    let extents_width = c.read_bd()?;
    let text = read_tv(c, version)?;
    let linespace_style = c.read_bs()?;
    let linespace_factor = c.read_bd()?;
    let _unknown_b = c.read_b()?;
    Ok(MText {
        insertion_point,
        extrusion,
        x_axis_direction,
        rect_width,
        rect_height,
        nominal_text_height,
        attachment_point,
        drawing_direction,
        extents_height,
        extents_width,
        text,
        linespace_style,
        linespace_factor,
        is_annotative: true,
        column_type: 0,
        column_width: 0.0,
        column_gutter: 0.0,
        column_heights: Vec::new(),
    })
}

/// Decode an R2007+ MTEXT whose text lives in the object's string
/// stream (ODA v5.4.1 §19.1 split layout, §19.4.44 MTEXT field table).
///
/// # The bug this replaces
///
/// [`decode`] reads `text` inline. On R2007+ that position holds the
/// *next data field*, not the string, so MTEXT never errored — it
/// returned a one-character string (`"ѕ"`) built from whatever two
/// bytes followed `extents_width`, for every record in the file. Silent
/// wrong is worse than an error: the coverage report counted it as
/// decoded.
///
/// # Measured — MTEXT's string stream holds exactly one `TV`
///
/// `text` is MTEXT's only variable-text field, so its string stream
/// must contain exactly one string and nothing else. Across all 22
/// MTEXT records of `sample_AC1032.dwg` (R2018) the stream length
/// equals the encoded length of that one string to the bit, with no
/// padding: `"Text"` occupies 74 bits of a 74-bit stream, `"Table
/// sample"` 202 of 202, `"Sample annotation"` 282 of 282, `"this is a
/// Mtext\nwith multiple lines in it"` 666 of 666, and the file's
/// 3,436-character MTEXT 54,978 of 54,978.
///
/// That makes "the reader is exhausted after one `read_tv`" a
/// self-validating check on the split-stream layout, and it is enforced
/// below alongside the requirement that the data fields do not run past
/// the string-stream start bit.
///
/// # Not yet decoded: the R2018 tail
///
/// The data fields this decoder understands end well short of the
/// string stream on R2018 — 567 bits short on `sample_AC1032.dwg`'s
/// first MTEXT (data fields end at bit 480, string stream starts at
/// 1047). Scanning that gap for the doubles already read shows the
/// record *repeats* them: `insertion.x` at bit 493, `insertion.y` at
/// 559, `rect_width` at 627, `extents_width` at 695, `extents_height`
/// at 761 and `rect_width` again at 847, each as a `BD` with the
/// two-bit `00` full-double prefix. That is consistent with the R2018
/// annotative / column block restating the geometry, but the field
/// boundaries between those repeats are not established from bytes yet,
/// so this decoder does not guess at them — it asserts the weaker
/// `<= string_start` bound instead of the exact equality the
/// symbol-table ports use. See `examples/probe_mtext_fields.rs`.
pub fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<MText> {
    let mut c = BitCursor::new(payload);
    string_stream::seek(&mut c, object_body_start)?;
    crate::common_entity::read_common_entity_data(&mut c, version)?;
    let mut mtext = read_modern_fields(&mut c, version)?;

    // A record whose `strings present` trailer bit is clear has an empty
    // text; that is a valid encoding, not a mis-read, so there is
    // nothing to validate against and `text` stays empty.
    let Some(stream) = string_stream::locate(payload, version) else {
        return Ok(mtext);
    };
    let at = c.position_bits();
    if at != stream.start_bit {
        return Err(modern::misaligned("MTEXT", at, stream.start_bit));
    }

    let mut strings = string_stream::StringReader::new(payload, stream)?;
    mtext.text = strings.read_tv()?;
    if !strings.is_exhausted() {
        return Err(Error::SectionMap(format!(
            "MTEXT string stream still has {} bits after its only TV — \
             the split-stream layout was mis-read",
            strings.remaining_bits()
        )));
    }
    Ok(mtext)
}

/// Read the R2007+ MTEXT field body from the data stream, leaving
/// [`MText::text`] empty for the caller to fill from the string stream.
/// Public wrapper over the R2007+ MTEXT field body, for probes under
/// `examples/` that need to walk an embedded MTEXT record.
pub fn read_modern_fields_probe(c: &mut BitCursor<'_>, version: Version) -> Result<MText> {
    read_modern_fields(c, version)
}

pub(crate) fn read_modern_fields(c: &mut BitCursor<'_>, version: Version) -> Result<MText> {
    let insertion_point = read_bd3(c)?;
    let extrusion = read_bd3(c)?;
    let x_axis_direction = read_bd3(c)?;
    let rect_width = c.read_bd()?;
    let rect_height = if version.is_r2007_plus() {
        Some(c.read_bd()?)
    } else {
        None
    };
    let nominal_text_height = c.read_bd()?;
    let attachment_point = c.read_bs()?;
    let drawing_direction = c.read_bs()?;
    let extents_height = c.read_bd()?;
    let extents_width = c.read_bd()?;
    // `TV text` sits here in field order; on R2007+ its characters are
    // in the string stream and it consumes no data-stream bits.
    let linespace_style = c.read_bs()?;
    let linespace_factor = c.read_bd()?;
    let _unknown_b = c.read_b()?;
    if version.is_r2004_plus() {
        let background_flags = c.read_bl()?;
        // §20.4.46: the background block follows when bit 0x01 is set,
        // "or in case of R2018 bit 0x10" — the R2018 text-frame bit.
        let frame_bit = if matches!(version, Version::R2018) {
            0x10
        } else {
            0
        };
        if background_flags & (0x01 | frame_bit) != 0 {
            let _background_scale = c.read_bd()?;
            let _background_color_index = c.read_bs_u()?;
            let _background_color_rgb = c.read_bl_u()?;
            let _background_color_byte = c.read_rc()?;
            let _background_transparency = c.read_bl()?;
        }
    }
    let mut is_annotative = true;
    let mut column_type = 0i16;
    let mut column_width = 0.0f64;
    let mut column_gutter = 0.0f64;
    let mut column_heights = Vec::new();
    if matches!(version, Version::R2018) {
        let is_not_annotative = c.read_b()?;
        is_annotative = !is_not_annotative;
        if is_not_annotative {
            let _version = c.read_bs()?;
            let _default_flag = c.read_b()?;
            // `H` registered application — handle stream, no data bits.
            let _attachment_point = c.read_bl()?;
            let _x_axis_dir = read_bd3(c)?;
            let _insertion_point = read_bd3(c)?;
            let _rect_width = c.read_bd()?;
            let _rect_height = c.read_bd()?;
            let _extents_width = c.read_bd()?;
            let _extents_height = c.read_bd()?;
            column_type = c.read_bs()?;
            if column_type != 0 {
                let height_count = c.read_bl_u()? as usize;
                column_width = c.read_bd()?;
                column_gutter = c.read_bd()?;
                let auto_height = c.read_b()?;
                let _flow_reversed = c.read_b()?;
                if !auto_height && column_type == 2 {
                    if height_count > MAX_COLUMN_HEIGHTS || height_count > c.remaining_bits() {
                        return Err(Error::SectionMap(format!(
                            "MTEXT column height count {height_count} exceeds cap \
                             ({MAX_COLUMN_HEIGHTS}) or remaining_bits ({})",
                            c.remaining_bits()
                        )));
                    }
                    column_heights.reserve(height_count);
                    for _ in 0..height_count {
                        column_heights.push(c.read_bd()?);
                    }
                }
            }
        }
    }
    Ok(MText {
        insertion_point,
        extrusion,
        x_axis_direction,
        rect_width,
        rect_height,
        nominal_text_height,
        attachment_point,
        drawing_direction,
        extents_height,
        extents_width,
        text: String::new(),
        linespace_style,
        linespace_factor,
        is_annotative,
        column_type,
        column_width,
        column_gutter,
        column_heights,
    })
}

fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if version.is_r2007_plus() {
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            let lo = c.read_rc()? as u16;
            let hi = c.read_rc()? as u16;
            units.push((hi << 8) | lo);
        }
        if units.last() == Some(&0) {
            units.pop();
        }
        String::from_utf16(&units)
            .map_err(|_| Error::SectionMap("MTEXT is not valid UTF-16".into()))
    } else {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(c.read_rc()?);
        }
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Build an R2018 MTEXT payload with the text in the string stream
    /// and read it back through [`decode_modern_split_stream`].
    #[test]
    fn r2018_split_stream_mtext_reads_text_from_string_stream() {
        let mut body = BitWriter::new();
        // Common entity preamble (§19.4.1), R2018 shape.
        body.write_bs_u(0); // EED terminator
        body.write_b(false); // no graphics
        body.write_bb(0b00); // entmode = ByLayer
        body.write_bl(0); // num_reactors
        body.write_b(true); // xdictionary missing
        body.write_b(false); // has DS binary data (R2013+)
        body.write_bs(0); // CMC colour — BYLAYER
        body.write_bd(1.0); // linetype scale
        body.write_bb(0b00); // ltype flags
        body.write_bb(0b00); // plotstyle flags
        body.write_bb(0b00); // material flags (R2007+)
        body.write_rc(0); // shadow flags (R2007+)
        body.write_b(false); // full visual style (R2010+)
        body.write_b(false); // face visual style
        body.write_b(false); // edge visual style
        body.write_bs(0); // invisibility
        body.write_rc(0); // lineweight
        // MTEXT field body (§19.4.44).
        for v in [183.5, 5.25, 0.0] {
            body.write_bd(v); // insertion point
        }
        for v in [0.0, 0.0, 1.0] {
            body.write_bd(v); // extrusion
        }
        for v in [1.0, 0.0, 0.0] {
            body.write_bd(v); // x-axis direction
        }
        body.write_bd(15.75); // rect width
        body.write_bd(0.0); // rect height (R2007+)
        body.write_bd(1.0); // nominal text height
        body.write_bs(1); // attachment point
        body.write_bs(5); // drawing direction
        body.write_bd(2.5); // extents height
        body.write_bd(13.5); // extents width
        // `TV text` lives in the string stream — no data-stream bits.
        body.write_bs(1); // linespace style
        body.write_bd(1.0); // linespace factor
        body.write_b(false); // unknown bit
        body.write_bl(0); // background flags (R2004+), no background

        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["Hello\\PMTEXT"]);
        let m = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(m.text, "Hello\\PMTEXT");
        assert_eq!(
            m.insertion_point,
            Point3D {
                x: 183.5,
                y: 5.25,
                z: 0.0
            }
        );
        assert_eq!(m.rect_width, 15.75);
        assert_eq!(m.rect_height, Some(0.0));
        assert_eq!(m.nominal_text_height, 1.0);
        assert_eq!(m.attachment_point, 1);
        assert_eq!(m.drawing_direction, 5);
        assert_eq!(m.extents_height, 2.5);
        assert_eq!(m.extents_width, 13.5);
        assert_eq!(m.linespace_style, 1);
    }

    /// A record whose data fields run past the string-stream start bit
    /// must error rather than return a plausible-looking string.
    #[test]
    fn r2018_split_stream_mtext_rejects_a_short_body() {
        let mut body = BitWriter::new();
        body.write_bs_u(0);
        body.write_b(false);
        body.write_bb(0b00);
        body.write_bl(0);
        body.write_b(true);
        body.write_b(false);
        body.write_bs(0);
        body.write_bd(1.0);
        body.write_bb(0b00);
        body.write_bb(0b00);
        body.write_bb(0b00);
        body.write_rc(0);
        body.write_b(false);
        body.write_b(false);
        body.write_b(false);
        body.write_bs(0);
        body.write_rc(0);
        // Only the insertion point — the rest of the field list is absent,
        // so the reader runs off the end of the data stream.
        for v in [1.0, 2.0, 3.0] {
            body.write_bd(v);
        }
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["x"]);
        assert!(decode_modern_split_stream(&payload, 8, Version::R2018).is_err());
    }

    #[test]
    fn roundtrip_mtext_r2000() {
        let mut w = BitWriter::new();
        // insertion point
        w.write_bd(10.0);
        w.write_bd(20.0);
        w.write_bd(0.0);
        // extrusion
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        // x axis direction
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(100.0); // rect width
        w.write_bd(2.5); // text height
        w.write_bs(1); // attachment
        w.write_bs(5); // drawing direction
        w.write_bd(5.0); // extents height
        w.write_bd(50.0); // extents width
        // TV "Hi\nEveryone"
        let s = b"Hi\\PEveryone";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_bs(0); // linespace_style
        w.write_bd(1.0); // linespace_factor
        w.write_b(false); // unknown bit
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(
            m.insertion_point,
            Point3D {
                x: 10.0,
                y: 20.0,
                z: 0.0
            }
        );
        assert_eq!(m.rect_width, 100.0);
        assert_eq!(m.text, "Hi\\PEveryone");
        assert_eq!(m.attachment_point, 1);
    }
}
