//! MLEADERSTYLE object — the style record behind the MULTILEADER
//! entity (leader geometry, landing, arrowhead, text and block content).
//!
//! # There is no spec prescription for this object
//!
//! The ODA *Open Design Specification for .dwg files* v5.4.1 lists
//! `MLEADERSTYLE` among the classes a drawing registers in
//! `AcDb:Classes`, but its object-prescription chapter §20.4 runs from
//! `20.4.1 Common Entity Data` to `20.4.104 XRECORD` and carries **no
//! entry** for it. Everything below was derived by measuring real
//! records against the boundary the format itself provides.
//!
//! # The wire shape — measured
//!
//! ```text
//! BS   version                       -- R2010+ only
//! BS   content_type                  BS   draw_mleader_order_type
//! BS   draw_leader_order_type        BS   max_leader_segment_points
//! BD   first_segment_angle           BD   second_segment_angle
//! BS   leader_line_type              CMC  leader_line_color
//! BL   leader_lineweight             B    enable_landing
//! BD   landing_gap                   B    enable_dogleg
//! BD   dogleg_length                 TV   description
//! BD   arrowhead_size                TV   default_mtext_contents
//! BS   text_left_attachment          BS   text_right_attachment
//! BS   text_angle_type               BS   text_alignment_type
//! CMC  text_color                    BD   text_height
//! B    enable_frame_text             B    text_align_always_left
//! BD   align_space                   CMC  block_content_color
//! BD   block_content_scale_x         BD   block_content_scale_y
//! BD   block_content_scale_z         B    enable_block_content_scale
//! BD   block_content_rotation        B    enable_block_content_rotation
//! BS   block_content_connection      BD   scale
//! B    overwrite_property_value      B    is_annotative
//! BD   break_gap_size
//! BS   text_attachment_direction     -- R2010+ only
//! BS   bottom_text_attachment_dir    -- R2010+ only
//! BS   top_text_attachment_dir       -- R2010+ only
//! B    extended_flag                 -- R2013+ only
//! ```
//!
//! `H` fields (leader linetype, arrow head, text style, block content)
//! live in the handle stream and cost no data-stream bits, so they do
//! not appear above; the field list is the data stream only.
//!
//! # Why this is not a guess
//!
//! Every record's data fields have to end exactly on the first bit of
//! its string stream (R2010+) or on its `RL` object-data-size (R2004),
//! the boundary `objects::modern::ObjectStream::finish` enforces. The
//! list above closes **all eleven** MLEADERSTYLE records of the corpus
//! with delta 0:
//!
//! | Release | Records | Budget | Delta |
//! |---|---|---|---|
//! | R2004 | 3 (`{arc,circle,line}_2004.dwg` handle 103) | 744 | 0 |
//! | R2010 | 3 (`*_2010.dwg` handle 103) | 692 | 0 |
//! | R2013 | 3 (`*_2013.dwg` handle 103) | 693 | 0 |
//! | R2018 | 2 (`sample_AC1032.dwg` handles 216, 229) | 693 / 749 | 0 |
//!
//! The three version switches are measured, not assumed. R2004 is
//! exactly one 10-bit `BS` shorter at the head and 22 bits shorter at
//! the tail than R2010; R2010 is exactly one bit shorter than R2013,
//! and the R2018 records reproduce the R2013 length.
//!
//! Corroboration from the decoded values of the `Standard` style — the
//! only style all four releases carry:
//!
//! - `leader_line_color`, `text_color` and `block_content_color` all
//!   decode as true-colour word `0xC1000000`, the ByBlock method octet;
//! - `leader_lineweight` decodes `-2`, the ByBlock lineweight sentinel;
//! - `landing_gap` decodes `0.09`, `dogleg_length` `0.36`,
//!   `arrowhead_size` `0.18`, `text_height` `0.18`, `break_gap_size`
//!   `0.125` — AutoCAD's shipped defaults for the `Standard` multileader
//!   style, to the last digit;
//! - the three block-content scales decode `1`, `1`, `1` and the
//!   overall `scale` `1`;
//! - `max_leader_segment_points` decodes `2` and both segment angle
//!   constraints `0`.
//!
//! A field list off by one bit reproduces none of that.
//!
//! # Naming
//!
//! The field **types, widths and order** above are measured. The
//! **names** follow the conventional `AcDbMLeaderStyle` property
//! ordering as recalled from Autodesk's public *DXF Reference*
//! (published documentation — see `CLEANROOM.md`); the values listed
//! above corroborate the leader, landing, arrowhead, text-height and
//! block-scale slots, while the attachment and flag slots are
//! positional labels. Treat them as labels for slots whose layout is
//! proven, not as verified semantics.
//!
//! # Versions
//!
//! R2004 through R2018 all decode. R13-R2000 are not reachable by this
//! crate's object walker, and MLEADERSTYLE post-dates them.

use crate::error::Result;
use crate::objects::color::{self, ObjectColor};
use crate::objects::modern;
use crate::version::Version;

/// A decoded MLEADERSTYLE record.
#[derive(Debug, Clone, PartialEq)]
pub struct AcadMLeaderStyle {
    /// Style-format version; `2` in every record measured. R2010+ only,
    /// `0` on older releases which do not store it.
    pub version: i16,
    /// Content kind (`2` = mtext on the shipped `Standard` style).
    pub content_type: i16,
    /// Multileader draw-order selector.
    pub draw_mleader_order_type: i16,
    /// Leader draw-order selector.
    pub draw_leader_order_type: i16,
    /// Maximum number of leader segment points.
    pub max_leader_segment_points: i32,
    /// First segment angle constraint, radians.
    pub first_segment_angle: f64,
    /// Second segment angle constraint, radians.
    pub second_segment_angle: f64,
    /// Leader line kind (`1` = straight on the shipped style).
    pub leader_line_type: i16,
    /// Leader line colour.
    pub leader_line_color: ObjectColor,
    /// Leader lineweight; `-2` is ByBlock.
    pub leader_lineweight: i32,
    /// Whether the leader ends in a landing.
    pub enable_landing: bool,
    /// Gap between the landing and the content.
    pub landing_gap: f64,
    /// Whether the leader has a dogleg.
    pub enable_dogleg: bool,
    /// Dogleg length.
    pub dogleg_length: f64,
    /// Style description.
    pub description: String,
    /// Arrowhead size.
    pub arrowhead_size: f64,
    /// Default MTEXT contents for new multileaders.
    pub default_mtext_contents: String,
    /// Text attachment on the left (positional name).
    pub text_left_attachment: i16,
    /// Text attachment on the right (positional name).
    pub text_right_attachment: i16,
    /// Text angle selector (positional name).
    pub text_angle_type: i16,
    /// Text alignment selector (positional name).
    pub text_alignment_type: i16,
    /// Text colour.
    pub text_color: ObjectColor,
    /// Text height.
    pub text_height: f64,
    /// Whether the text is framed.
    pub enable_frame_text: bool,
    /// Whether text is always aligned left (positional name).
    pub text_align_always_left: bool,
    /// Alignment space.
    pub align_space: f64,
    /// Block-content colour.
    pub block_content_color: ObjectColor,
    /// Block-content scale, X.
    pub block_content_scale_x: f64,
    /// Block-content scale, Y.
    pub block_content_scale_y: f64,
    /// Block-content scale, Z.
    pub block_content_scale_z: f64,
    /// Whether the block-content scale is honoured.
    pub enable_block_content_scale: bool,
    /// Block-content rotation, radians.
    pub block_content_rotation: f64,
    /// Whether the block-content rotation is honoured.
    pub enable_block_content_rotation: bool,
    /// Block-content connection selector (positional name).
    pub block_content_connection_type: i16,
    /// Overall style scale.
    pub scale: f64,
    /// Overwrite-property flag (positional name).
    pub overwrite_property_value: bool,
    /// Annotative flag (positional name).
    pub is_annotative: bool,
    /// Break gap size.
    pub break_gap_size: f64,
    /// Text attachment direction (positional name). R2010+ only.
    pub text_attachment_direction: i16,
    /// Bottom text attachment direction (positional name). R2010+ only.
    pub bottom_text_attachment_direction: i16,
    /// Top text attachment direction (positional name). R2010+ only.
    pub top_text_attachment_direction: i16,
    /// The single further flag R2013 added at the tail; its meaning is
    /// not determined. `false` on releases that do not store it.
    pub extended_flag: bool,
}

/// Decode an MLEADERSTYLE straight from its raw object payload, taking
/// its `TV` fields from the R2007+ string stream and checking that the
/// data fields end exactly on the data-stream boundary.
pub fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadMLeaderStyle> {
    let mut s = modern::open(payload, body_start, inline_data_end, version)?;
    let has_version = version.is_r2010_plus();
    let style_version = if has_version { s.data.read_bs()? } else { 0 };
    let content_type = s.data.read_bs()?;
    let draw_mleader_order_type = s.data.read_bs()?;
    let draw_leader_order_type = s.data.read_bs()?;
    let max_leader_segment_points = s.data.read_bl()?;
    let first_segment_angle = s.data.read_bd()?;
    let second_segment_angle = s.data.read_bd()?;
    let leader_line_type = s.data.read_bs()?;
    let leader_line_color = color::read(&mut s.data, &mut s.strings, version)?;
    let leader_lineweight = s.data.read_bl()?;
    let enable_landing = s.data.read_b()?;
    let landing_gap = s.data.read_bd()?;
    let enable_dogleg = s.data.read_b()?;
    let dogleg_length = s.data.read_bd()?;
    let description = modern::read_tv(&mut s.data, &mut s.strings, version)?;
    let arrowhead_size = s.data.read_bd()?;
    let default_mtext_contents = modern::read_tv(&mut s.data, &mut s.strings, version)?;
    let text_left_attachment = s.data.read_bs()?;
    let text_right_attachment = s.data.read_bs()?;
    let text_angle_type = s.data.read_bs()?;
    let text_alignment_type = s.data.read_bs()?;
    let text_color = color::read(&mut s.data, &mut s.strings, version)?;
    let text_height = s.data.read_bd()?;
    let enable_frame_text = s.data.read_b()?;
    let text_align_always_left = s.data.read_b()?;
    let align_space = s.data.read_bd()?;
    let block_content_color = color::read(&mut s.data, &mut s.strings, version)?;
    let block_content_scale_x = s.data.read_bd()?;
    let block_content_scale_y = s.data.read_bd()?;
    let block_content_scale_z = s.data.read_bd()?;
    let enable_block_content_scale = s.data.read_b()?;
    let block_content_rotation = s.data.read_bd()?;
    let enable_block_content_rotation = s.data.read_b()?;
    let block_content_connection_type = s.data.read_bs()?;
    let scale = s.data.read_bd()?;
    let overwrite_property_value = s.data.read_b()?;
    let is_annotative = s.data.read_b()?;
    let break_gap_size = s.data.read_bd()?;
    let (
        text_attachment_direction,
        bottom_text_attachment_direction,
        top_text_attachment_direction,
    ) = if has_version {
        (s.data.read_bs()?, s.data.read_bs()?, s.data.read_bs()?)
    } else {
        (0, 0, 0)
    };
    let extended_flag = if matches!(version, Version::R2013 | Version::R2018) {
        s.data.read_b()?
    } else {
        false
    };
    s.finish("MLEADERSTYLE")?;
    Ok(AcadMLeaderStyle {
        version: style_version,
        content_type,
        draw_mleader_order_type,
        draw_leader_order_type,
        max_leader_segment_points,
        first_segment_angle,
        second_segment_angle,
        leader_line_type,
        leader_line_color,
        leader_lineweight,
        enable_landing,
        landing_gap,
        enable_dogleg,
        dogleg_length,
        description,
        arrowhead_size,
        default_mtext_contents,
        text_left_attachment,
        text_right_attachment,
        text_angle_type,
        text_alignment_type,
        text_color,
        text_height,
        enable_frame_text,
        text_align_always_left,
        align_space,
        block_content_color,
        block_content_scale_x,
        block_content_scale_y,
        block_content_scale_z,
        enable_block_content_scale,
        block_content_rotation,
        enable_block_content_rotation,
        block_content_connection_type,
        scale,
        overwrite_property_value,
        is_annotative,
        break_gap_size,
        text_attachment_direction,
        bottom_text_attachment_direction,
        top_text_attachment_direction,
        extended_flag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn cmc(w: &mut BitWriter, rgb: u32) {
        w.write_bs_u(0);
        w.write_bl_u(rgb);
        w.write_rc(0);
    }

    /// The `Standard` multileader style, with the values every corpus
    /// record decodes.
    fn build(version: Version) -> Vec<u8> {
        // R2010 predates the R2013+ AcDs binary-data bit of the common
        // object prefix, so it takes the shorter prefix.
        let mut w = if matches!(version, Version::R2013 | Version::R2018) {
            modern::tests::r2018_object_prefix(1)
        } else {
            modern::tests::r2004_object_prefix(1)
        };
        write_common_body(&mut w, version, false);
        let bits = crate::string_stream::tests::bits_of(&w);
        crate::string_stream::tests::build_payload(&bits, &["Standard", ""])
    }

    /// Write the field list itself. `inline_strings` selects the
    /// pre-R2007 layout, where the two `TV` slots consume data bits.
    fn write_common_body(w: &mut BitWriter, version: Version, inline_strings: bool) {
        if version.is_r2010_plus() {
            w.write_bs(2); // version
        }
        w.write_bs(2); // content_type
        w.write_bs(1); // draw_mleader_order_type
        w.write_bs(0); // draw_leader_order_type
        w.write_bl(2); // max_leader_segment_points
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bs(1); // leader_line_type
        cmc(w, 0xC100_0000);
        w.write_bl(-2); // leader_lineweight
        w.write_b(true); // enable_landing
        w.write_bd(0.09);
        w.write_b(true); // enable_dogleg
        w.write_bd(0.36);
        if inline_strings {
            modern::tests::write_inline_tv(w, "Standard");
        }
        w.write_bd(0.18); // arrowhead_size
        if inline_strings {
            modern::tests::write_inline_tv(w, "");
        }
        w.write_bs(1);
        w.write_bs(6);
        w.write_bs(1);
        w.write_bs(0);
        cmc(w, 0xC100_0000);
        w.write_bd(0.18); // text_height
        w.write_b(false);
        w.write_b(false);
        w.write_bd(0.18); // align_space
        cmc(w, 0xC100_0000);
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_b(true);
        w.write_bd(0.0);
        w.write_b(true);
        w.write_bs(0);
        w.write_bd(1.0); // scale
        w.write_b(false);
        w.write_b(true);
        w.write_bd(0.125); // break_gap_size
        if version.is_r2010_plus() {
            w.write_bs(0);
            w.write_bs(9);
            w.write_bs(9);
        }
        if matches!(version, Version::R2013 | Version::R2018) {
            w.write_b(true);
        }
    }

    #[test]
    fn r2018_mleader_style_closes_on_its_string_stream() {
        let payload = build(Version::R2018);
        let s = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(s.version, 2);
        assert_eq!(s.content_type, 2);
        assert_eq!(s.max_leader_segment_points, 2);
        assert_eq!(s.leader_line_color.method(), 0xC1);
        assert_eq!(s.leader_lineweight, -2);
        assert!(s.enable_landing && s.enable_dogleg);
        assert!((s.landing_gap - 0.09).abs() < 1e-12);
        assert!((s.dogleg_length - 0.36).abs() < 1e-12);
        assert!((s.arrowhead_size - 0.18).abs() < 1e-12);
        assert!((s.text_height - 0.18).abs() < 1e-12);
        assert!((s.break_gap_size - 0.125).abs() < 1e-12);
        assert_eq!(s.description, "Standard");
        assert_eq!(s.default_mtext_contents, "");
        assert_eq!(s.block_content_scale_x, 1.0);
        assert_eq!(s.top_text_attachment_direction, 9);
        assert!(s.extended_flag);
    }

    #[test]
    fn r2010_mleader_style_has_no_r2013_tail_flag() {
        let payload = build(Version::R2010);
        let s = decode_object(&payload, 8, None, Version::R2010).unwrap();
        assert_eq!(s.version, 2);
        assert!(!s.extended_flag);
        assert_eq!(s.bottom_text_attachment_direction, 9);
    }

    /// The R2004 layout: `TV` fields inline, no leading `version` word
    /// and no attachment-direction tail, closing on the `RL`
    /// object-data-size boundary.
    #[test]
    fn r2004_mleader_style_omits_the_r2010_fields() {
        let mut w = modern::tests::r2004_object_prefix(1);
        write_common_body(&mut w, Version::R2004, true);
        let end = w.position_bits();
        let bytes = w.into_bytes();
        let s = decode_object(&bytes, 0, Some(end), Version::R2004).unwrap();
        assert_eq!(s.version, 0);
        assert_eq!(s.text_attachment_direction, 0);
        assert_eq!(s.description, "Standard");
        assert_eq!(s.leader_lineweight, -2);
        assert!((s.break_gap_size - 0.125).abs() < 1e-12);
    }

    #[test]
    fn r2013_body_rejected_by_the_r2010_field_list() {
        let payload = build(Version::R2018);
        assert!(decode_object(&payload, 8, None, Version::R2010).is_err());
    }
}
