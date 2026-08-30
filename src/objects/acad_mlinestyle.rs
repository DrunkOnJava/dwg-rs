//! MLINESTYLE object (ODA *Open Design Specification for .dwg files*
//! v5.4.1 §20.4.73) — multiline style definition.
//!
//! MLINESTYLE is the style record backing the MLINE entity: it
//! describes how many parallel lines are drawn, each line's offset from
//! the reference axis, its colour, and its linetype.
//!
//! # The spec prescribes the whole record (§20.4.73)
//!
//! ```text
//! TV   name             -- name of this style
//! TV   desc             -- description of this style
//! BS   flags            -- DXF flag word (the spec tabulates the bit
//!                          permutation between DWG and DXF)
//! CMC  fillcolor        -- fill colour for this style
//! BD   startang         -- start angle
//! BD   endang           -- end angle
//! RC   linesinstyle     -- number of lines in this style
//! REPEAT linesinstyle times:
//!   BD   offset         -- offset of this segment
//!   CMC  color          -- colour of this segment
//!   BS   ltindex        -- linetype index, before R2018
//!   H    linetype       -- linetype handle, R2018+
//! END REPEAT
//! ```
//!
//! An earlier revision of this module cited "§19.6.4 (L6-13)" — the
//! spec has no §19.6 chapter — and read `BS` where §20.4.73 prints
//! `CMC` for both colour fields, which is 42 bits short per record on
//! every real file. The citation is withdrawn and the field list is the
//! §20.4.73 one.
//!
//! # It closes on every corpus record
//!
//! `H` fields cost no data-stream bits on any release this crate walks
//! (R2000+ moves the handle references past the `RL` object-data-size
//! boundary), so the R2018 per-line handle is invisible to the data
//! cursor while the pre-R2018 `BS ltindex` is not. With that one
//! version switch the field list ends exactly on the data-stream
//! boundary for all ten MLINESTYLE records of the corpus:
//!
//! | Release | Records | Budget | Delta |
//! |---|---|---|---|
//! | R2004 | 3 (`{arc,circle,line}_2004.dwg` handle 96) | 526 | 0 |
//! | R2010 | 3 (`*_2010.dwg` handle 96) | 442 | 0 |
//! | R2013 | 3 (`*_2013.dwg` handle 96) | 442 | 0 |
//! | R2018 | 1 (`sample_AC1032.dwg` handle 24) | 406 | 0 |
//!
//! The R2013 → R2018 difference is exactly the two `BS ltindex` fields
//! the R2018 record replaces with handles: both records carry two
//! lines, both write `ltindex` in the 16-bit `BS` form (18 bits), and
//! 442 − 2 × 18 = 406.
//!
//! Decoded values corroborate the reading: the `STANDARD` style of
//! every file decodes `startang = endang = π/2`, `fillcolor` method
//! `0xC0` (ByLayer), two lines at offsets `+0.5` and `−0.5`, each
//! ByLayer, and `ltindex = 32767` — the "no linetype" sentinel.

use crate::error::{Error, Result};
use crate::objects::color::{self, ObjectColor};
use crate::objects::modern;
use crate::version::Version;

/// Format-limit cap on the number of lines a single MLINESTYLE may
/// carry (§20.4.73 stores the count in one `RC`; AutoCAD's own limit is
/// 16 elements per style).
const MAX_MLINESTYLE_LINES: usize = 16;

/// One of the parallel line elements of an MLINESTYLE (§20.4.73).
#[derive(Debug, Clone, PartialEq)]
pub struct MlineStyleLine {
    /// Offset of this segment from the reference axis.
    pub offset: f64,
    /// Colour of this segment.
    pub color: ObjectColor,
    /// Linetype index, before R2018. `32767` is the "none" sentinel.
    /// R2018+ stores a linetype handle instead and leaves this `0`.
    pub linetype_index: i16,
}

/// A decoded MLINESTYLE record (§20.4.73).
#[derive(Debug, Clone, PartialEq)]
pub struct AcadMlinestyle {
    /// Name of this style.
    pub name: String,
    /// Description of this style.
    pub description: String,
    /// DXF flag word.
    pub flags: i16,
    /// Fill colour for this style.
    pub fill_color: ObjectColor,
    /// Start angle, radians.
    pub start_angle: f64,
    /// End angle, radians.
    pub end_angle: f64,
    /// The style's parallel line elements, in wire order.
    pub lines: Vec<MlineStyleLine>,
}

/// Decode an MLINESTYLE straight from its raw object payload (§20.4.73),
/// taking its `TV` fields from the R2007+ string stream and checking
/// that the data fields end exactly on the data-stream boundary.
pub fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadMlinestyle> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let style = read_fields(&mut split, version)?;
    split.finish("MLINESTYLE")?;
    Ok(style)
}

/// Read the §20.4.73 field list off an already-opened object stream.
fn read_fields(split: &mut modern::ObjectStream<'_>, version: Version) -> Result<AcadMlinestyle> {
    let name = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let description = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let flags = split.data.read_bs()?;
    let fill_color = color::read(&mut split.data, &mut split.strings, version)?;
    let start_angle = split.data.read_bd()?;
    let end_angle = split.data.read_bd()?;
    let num_lines = split.data.read_rc()? as usize;
    if num_lines > MAX_MLINESTYLE_LINES {
        return Err(Error::SectionMap(format!(
            "MLINESTYLE claims {num_lines} lines (max {MAX_MLINESTYLE_LINES})"
        )));
    }
    let mut lines = Vec::with_capacity(num_lines);
    for _ in 0..num_lines {
        let offset = split.data.read_bd()?;
        let color = color::read(&mut split.data, &mut split.strings, version)?;
        // R2018 moved the per-line linetype to a handle, which lives in
        // the handle stream and costs no data-stream bits.
        let linetype_index = if matches!(version, Version::R2018) {
            0
        } else {
            split.data.read_bs()?
        };
        lines.push(MlineStyleLine {
            offset,
            color,
            linetype_index,
        });
    }
    Ok(AcadMlinestyle {
        name,
        description,
        flags,
        fill_color,
        start_angle,
        end_angle,
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn cmc(w: &mut BitWriter, index: u16, rgb: u32, color_byte: u8) {
        w.write_bs_u(index);
        w.write_bl_u(rgb);
        w.write_rc(color_byte);
    }

    /// The `STANDARD` style every corpus file carries: two elements at
    /// ±0.5, both ByLayer, both with the `32767` linetype sentinel.
    fn build(version: Version) -> Vec<u8> {
        let mut body = modern::tests::r2018_object_prefix(1);
        body.write_bs(0); // flags
        cmc(&mut body, 0, 0xC000_0000, 0); // fill colour, ByLayer
        body.write_bd(std::f64::consts::FRAC_PI_2);
        body.write_bd(std::f64::consts::FRAC_PI_2);
        body.write_rc(2); // linesinstyle
        for offset in [0.5, -0.5] {
            body.write_bd(offset);
            cmc(&mut body, 0, 0xC000_0000, 0);
            if !matches!(version, Version::R2018) {
                body.write_bs(32767);
            }
        }
        let bits = crate::string_stream::tests::bits_of(&body);
        crate::string_stream::tests::build_payload(&bits, &["STANDARD", ""])
    }

    #[test]
    fn r2018_mlinestyle_closes_on_its_string_stream() {
        let payload = build(Version::R2018);
        let style = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(style.name, "STANDARD");
        assert_eq!(style.description, "");
        assert_eq!(style.flags, 0);
        assert_eq!(style.fill_color.method(), 0xC0);
        assert!((style.start_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((style.end_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(style.lines.len(), 2);
        assert_eq!(style.lines[0].offset, 0.5);
        assert_eq!(style.lines[1].offset, -0.5);
        assert_eq!(style.lines[0].color.method(), 0xC0);
        // R2018 takes the per-line linetype from the handle stream.
        assert_eq!(style.lines[0].linetype_index, 0);
    }

    /// The pre-R2018 `BS ltindex` is not optional: an R2013 body read
    /// with the R2018 field list leaves both index words unread, and the
    /// boundary check has to reject that.
    #[test]
    fn r2013_body_rejected_by_the_r2018_field_list() {
        let payload = build(Version::R2013);
        let style = decode_object(&payload, 8, None, Version::R2013).unwrap();
        assert_eq!(style.lines[0].linetype_index, 32767);
        assert!(decode_object(&payload, 8, None, Version::R2018).is_err());
    }

    /// …and the converse.
    #[test]
    fn r2018_body_rejected_by_the_r2013_field_list() {
        let payload = build(Version::R2018);
        assert!(decode_object(&payload, 8, None, Version::R2013).is_err());
    }

    #[test]
    fn rejects_too_many_lines() {
        let mut body = modern::tests::r2018_object_prefix(0);
        body.write_bs(0);
        cmc(&mut body, 0, 0xC000_0000, 0);
        body.write_bd(0.0);
        body.write_bd(0.0);
        body.write_rc(17); // > 16
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["BIG", ""]);
        let err = decode_object(&payload, 8, None, Version::R2018).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("MLINESTYLE")));
    }
}
