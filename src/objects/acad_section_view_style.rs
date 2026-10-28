//! ACDBSECTIONVIEWSTYLE object — the style record behind a model
//! documentation *section view* (identifier, cutting-plane arrows,
//! view label, and the cut-surface hatch).
//!
//! # There is no spec prescription for this object
//!
//! The ODA *Open Design Specification for .dwg files* v5.4.1 has **no
//! §20.4 entry** for `ACDBSECTIONVIEWSTYLE`; its object-prescription
//! chapter runs from `20.4.1 Common Entity Data` to `20.4.104 XRECORD`
//! and stops. The field list below was derived by measuring real
//! records against the boundary the format itself provides.
//!
//! # The wire shape — measured
//!
//! ```text
//! BS   unknown_head
//! TV   name
//! TV   name_alias                    -- R2018 only
//! B    unknown_flag_a                -- R2018 only
//! B    unknown_flag_b                -- R2018 only
//! BS   flags                         -- 44 in every corpus record
//! B    flag_c   B    flag_d   B    flag_e
//! CMC  identifier_color              BD   identifier_height
//! CMC  arrow_symbol_color            BD   arrow_symbol_size
//! TV   identifier_exclude_characters BD   identifier_offset
//! BS   cutting_plane_line_weight     CMC  cutting_plane_line_color
//! BS   end_line_weight               CMC  end_line_color
//! BD   end_line_length               BD   end_line_overshoot
//! CMC  view_label_color              BD   view_label_text_height
//! BS   view_label_position           BD   view_label_offset
//! BS   view_label_attachment         TV   view_label_pattern
//! CMC  hatch_color                   CMC  hatch_background_color
//! TV   hatch_pattern_name            RC   hatch_flags
//! BD   hatch_scale                   BS   hatch_transparency
//! BD   hatch_spacing
//! BS   hatch_angle_count             BS   unknown_tail
//! 5 x BD hatch_angles
//! ```
//!
//! `H` fields (text styles, arrow blocks, linetypes) live in the handle
//! stream and cost no data-stream bits, so they do not appear; this is
//! the data stream only.
//!
//! # Why this is not a guess
//!
//! Every record's data fields have to end exactly on the first bit of
//! its string stream (R2010+) or on its `RL` object-data-size (R2004) —
//! the boundary `objects::modern::ObjectStream::finish` enforces. The
//! list above closes **all eleven** ACDBSECTIONVIEWSTYLE records of the
//! corpus with delta 0:
//!
//! | Release | Records | Budget | Delta |
//! |---|---|---|---|
//! | R2004 | 3 (`{arc,circle,line}_2004.dwg` handle 105) | 2325 | 0 |
//! | R2010 | 3 (`*_2010.dwg` handle 105) | 1301 | 0 |
//! | R2013 | 3 (`*_2013.dwg` handle 105) | 1301 | 0 |
//! | R2018 | 2 (`sample_AC1032.dwg` handles 426, 923) | 1263 / 1367 | 0 |
//!
//! The R2004 record is the discriminating evidence for the string
//! placement: its four `TV` fields cost 82, 146, 730 and 66 data-stream
//! bits respectively, so a `TV` in the wrong slot moves everything
//! after it by hundreds of bits and cannot close. It is also what fixes
//! the order of `hatch_pattern_name` and `hatch_flags` — on R2010+ a
//! `TV` costs nothing and the two are interchangeable, but on R2004
//! only `TV` then `RC` lands the following `BD` on `2.5`.
//!
//! Corroboration from the decoded values:
//!
//! - the five trailing `BD`s decode `1.5707963267948966`,
//!   `0.2617993877991494`, `1.3089969389957472`, `-0.2617993877991494`
//!   and `1.8325957145940461` — exactly 90°, 15°, 75°, −15° and 105° in
//!   radians, the cutting-plane angle set AutoCAD offers for section
//!   views. Five consecutive full-width doubles all landing on whole
//!   degrees is not something a misaligned field list produces;
//! - `identifier_exclude_characters` decodes `I, O, Q, S, X, Z` — the
//!   identifier letters AutoCAD reserves and excludes by default;
//! - `hatch_pattern_name` decodes `ANSI31`;
//! - `hatch_scale` and `hatch_spacing` decode `2.5`, the metric preset;
//! - `identifier_height`, `arrow_symbol_size`, `end_line_length` and
//!   `view_label_text_height` decode `5` on the metric records and
//!   `0.24` on the imperial one; `view_label_offset` decodes `15` /
//!   `0.75`; `identifier_offset` decodes `10` / `0.48`;
//! - `cutting_plane_line_weight` decodes `25` and `end_line_weight`
//!   `50` — 0.25 mm and 0.50 mm in the DWG lineweight encoding;
//! - every `CMC` but one decodes true-colour word `0xC0000000`
//!   (ByLayer); the exception is `hatch_background_color`, which
//!   decodes `0xC8000000` — the "no colour" method — on every record,
//!   which is what a hatch with no background fill stores.
//!
//! # Naming
//!
//! The field **types, widths and order** are measured. The **names**
//! follow the vocabulary of AutoCAD's Section View Style dialog
//! (published product documentation — see `CLEANROOM.md`) applied in
//! wire order. The identifier, arrow, lineweight, view-label and hatch
//! slots are corroborated by the values above; the `unknown_*` and
//! `flag_*` slots are positional labels for slots whose layout is
//! proven and whose meaning is not.

use crate::error::Result;
use crate::objects::color::{self, ObjectColor};
use crate::objects::modern;
use crate::version::Version;

/// Number of cutting-plane angles the record's tail always carries.
const HATCH_ANGLE_COUNT: usize = 5;

/// A decoded ACDBSECTIONVIEWSTYLE record.
#[derive(Debug, Clone, PartialEq)]
pub struct AcadSectionViewStyle {
    /// Leading `BS`; `0` in every corpus record. Positional name.
    pub unknown_head: i16,
    /// Style name.
    pub name: String,
    /// Second name `TV`, R2018 only; repeats [`name`](Self::name) in
    /// both corpus records.
    pub name_alias: String,
    /// R2018-only leading flag. Positional name.
    pub unknown_flag_a: bool,
    /// R2018-only leading flag. Positional name.
    pub unknown_flag_b: bool,
    /// Style flag word; `44` in every corpus record.
    pub flags: i16,
    /// Positional name.
    pub flag_c: bool,
    /// Positional name.
    pub flag_d: bool,
    /// Positional name.
    pub flag_e: bool,
    /// Identifier text colour.
    pub identifier_color: ObjectColor,
    /// Identifier text height.
    pub identifier_height: f64,
    /// Cutting-plane arrow colour.
    pub arrow_symbol_color: ObjectColor,
    /// Cutting-plane arrow size.
    pub arrow_symbol_size: f64,
    /// Identifier letters excluded from automatic naming.
    pub identifier_exclude_characters: String,
    /// Identifier offset.
    pub identifier_offset: f64,
    /// Cutting-plane lineweight, hundredths of a millimetre.
    pub cutting_plane_line_weight: i16,
    /// Cutting-plane colour.
    pub cutting_plane_line_color: ObjectColor,
    /// End-line lineweight, hundredths of a millimetre.
    pub end_line_weight: i16,
    /// End-line colour.
    pub end_line_color: ObjectColor,
    /// End-line length.
    pub end_line_length: f64,
    /// End-line overshoot (positional name).
    pub end_line_overshoot: f64,
    /// View-label colour.
    pub view_label_color: ObjectColor,
    /// View-label text height.
    pub view_label_text_height: f64,
    /// View-label position selector (positional name).
    pub view_label_position: i16,
    /// View-label offset.
    pub view_label_offset: f64,
    /// View-label attachment selector (positional name).
    pub view_label_attachment: i16,
    /// View-label field expression.
    pub view_label_pattern: String,
    /// Cut-surface hatch colour.
    pub hatch_color: ObjectColor,
    /// Cut-surface hatch background colour; `0xC8______` — the "no
    /// colour" method — in every corpus record.
    pub hatch_background_color: ObjectColor,
    /// Cut-surface hatch pattern name; `ANSI31` in every corpus record.
    pub hatch_pattern_name: String,
    /// Hatch flag byte (positional name).
    pub hatch_flags: u8,
    /// Hatch scale.
    pub hatch_scale: f64,
    /// Hatch transparency (positional name).
    pub hatch_transparency: i16,
    /// Hatch spacing.
    pub hatch_spacing: f64,
    /// Angle-array selector (positional name); `6` in every corpus
    /// record, which is one more than the number of angles stored.
    pub hatch_angle_count: i16,
    /// Trailing `BS` before the angle array. Positional name.
    pub unknown_tail: i16,
    /// The five cutting-plane angles, radians.
    pub hatch_angles: [f64; HATCH_ANGLE_COUNT],
}

/// Decode an ACDBSECTIONVIEWSTYLE straight from its raw object payload,
/// taking its `TV` fields from the R2007+ string stream and checking
/// that the data fields end exactly on the data-stream boundary.
pub fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadSectionViewStyle> {
    let mut s = modern::open(payload, body_start, inline_data_end, version)?;
    let r2018 = matches!(version, Version::R2018);
    let unknown_head = s.data.read_bs()?;
    let name = modern::read_tv(&mut s.data, &mut s.strings, version)?;
    let (name_alias, unknown_flag_a, unknown_flag_b) = if r2018 {
        (
            modern::read_tv(&mut s.data, &mut s.strings, version)?,
            s.data.read_b()?,
            s.data.read_b()?,
        )
    } else {
        (String::new(), false, false)
    };
    let flags = s.data.read_bs()?;
    let flag_c = s.data.read_b()?;
    let flag_d = s.data.read_b()?;
    let flag_e = s.data.read_b()?;
    let identifier_color = color::read(&mut s.data, &mut s.strings, version)?;
    let identifier_height = s.data.read_bd()?;
    let arrow_symbol_color = color::read(&mut s.data, &mut s.strings, version)?;
    let arrow_symbol_size = s.data.read_bd()?;
    let identifier_exclude_characters = modern::read_tv(&mut s.data, &mut s.strings, version)?;
    let identifier_offset = s.data.read_bd()?;
    let cutting_plane_line_weight = s.data.read_bs()?;
    let cutting_plane_line_color = color::read(&mut s.data, &mut s.strings, version)?;
    let end_line_weight = s.data.read_bs()?;
    let end_line_color = color::read(&mut s.data, &mut s.strings, version)?;
    let end_line_length = s.data.read_bd()?;
    let end_line_overshoot = s.data.read_bd()?;
    let view_label_color = color::read(&mut s.data, &mut s.strings, version)?;
    let view_label_text_height = s.data.read_bd()?;
    let view_label_position = s.data.read_bs()?;
    let view_label_offset = s.data.read_bd()?;
    let view_label_attachment = s.data.read_bs()?;
    let view_label_pattern = modern::read_tv(&mut s.data, &mut s.strings, version)?;
    let hatch_color = color::read(&mut s.data, &mut s.strings, version)?;
    let hatch_background_color = color::read(&mut s.data, &mut s.strings, version)?;
    let hatch_pattern_name = modern::read_tv(&mut s.data, &mut s.strings, version)?;
    let hatch_flags = s.data.read_rc()?;
    let hatch_scale = s.data.read_bd()?;
    let hatch_transparency = s.data.read_bs()?;
    let hatch_spacing = s.data.read_bd()?;
    let hatch_angle_count = s.data.read_bs()?;
    let unknown_tail = s.data.read_bs()?;
    let mut hatch_angles = [0.0f64; HATCH_ANGLE_COUNT];
    for angle in &mut hatch_angles {
        *angle = s.data.read_bd()?;
    }
    s.finish("ACDBSECTIONVIEWSTYLE")?;
    Ok(AcadSectionViewStyle {
        unknown_head,
        name,
        name_alias,
        unknown_flag_a,
        unknown_flag_b,
        flags,
        flag_c,
        flag_d,
        flag_e,
        identifier_color,
        identifier_height,
        arrow_symbol_color,
        arrow_symbol_size,
        identifier_exclude_characters,
        identifier_offset,
        cutting_plane_line_weight,
        cutting_plane_line_color,
        end_line_weight,
        end_line_color,
        end_line_length,
        end_line_overshoot,
        view_label_color,
        view_label_text_height,
        view_label_position,
        view_label_offset,
        view_label_attachment,
        view_label_pattern,
        hatch_color,
        hatch_background_color,
        hatch_pattern_name,
        hatch_flags,
        hatch_scale,
        hatch_transparency,
        hatch_spacing,
        hatch_angle_count,
        unknown_tail,
        hatch_angles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use std::f64::consts::FRAC_PI_2;

    const EXCLUDE: &str = "I, O, Q, S, X, Z";
    const LABEL: &str = "%<\\AcVar ViewSectionStartId>%";
    const ANGLES: [f64; 5] = [
        FRAC_PI_2,
        0.2617993877991494,
        1.3089969389957472,
        -0.2617993877991494,
        1.8325957145940461,
    ];

    fn cmc(w: &mut BitWriter, rgb: u32) {
        w.write_bs_u(0);
        w.write_bl_u(rgb);
        w.write_rc(0);
    }

    fn write_body(w: &mut BitWriter, version: Version, inline_strings: bool) {
        let r2018 = matches!(version, Version::R2018);
        w.write_bs(0);
        if inline_strings {
            modern::tests::write_inline_tv(w, "Metric50");
        }
        if r2018 {
            w.write_b(false);
            w.write_b(true);
        }
        w.write_bs(44);
        w.write_b(true);
        w.write_b(true);
        w.write_b(false);
        cmc(w, 0xC000_0000);
        w.write_bd(5.0);
        cmc(w, 0xC000_0000);
        w.write_bd(5.0);
        if inline_strings {
            modern::tests::write_inline_tv(w, EXCLUDE);
        }
        w.write_bd(10.0);
        w.write_bs(25);
        cmc(w, 0xC000_0000);
        w.write_bs(50);
        cmc(w, 0xC000_0000);
        w.write_bd(5.0);
        w.write_bd(5.0);
        cmc(w, 0xC000_0000);
        w.write_bd(5.0);
        w.write_bs(0);
        w.write_bd(15.0);
        w.write_bs(1);
        if inline_strings {
            modern::tests::write_inline_tv(w, LABEL);
        }
        cmc(w, 0xC000_0000);
        cmc(w, 0xC800_0000);
        if inline_strings {
            modern::tests::write_inline_tv(w, "ANSI31");
        }
        w.write_rc(98);
        w.write_bd(2.5);
        w.write_bs(0);
        w.write_bd(2.5);
        w.write_bs(6);
        w.write_bs(0);
        for angle in ANGLES {
            w.write_bd(angle);
        }
    }

    fn build(version: Version) -> Vec<u8> {
        let mut w = modern::tests::r2018_object_prefix(1);
        write_body(&mut w, version, false);
        let bits = crate::string_stream::tests::bits_of(&w);
        let strings: Vec<&str> = if matches!(version, Version::R2018) {
            vec!["Metric50", "Metric50", EXCLUDE, LABEL, "ANSI31"]
        } else {
            vec!["Metric50", EXCLUDE, LABEL, "ANSI31"]
        };
        crate::string_stream::tests::build_payload(&bits, &strings)
    }

    #[test]
    fn r2018_section_view_style_closes_on_its_string_stream() {
        let payload = build(Version::R2018);
        let s = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(s.name, "Metric50");
        assert_eq!(s.name_alias, "Metric50");
        assert_eq!(s.flags, 44);
        assert_eq!(s.identifier_exclude_characters, EXCLUDE);
        assert_eq!(s.hatch_pattern_name, "ANSI31");
        assert_eq!(s.view_label_pattern, LABEL);
        assert_eq!(s.cutting_plane_line_weight, 25);
        assert_eq!(s.end_line_weight, 50);
        assert_eq!(s.hatch_background_color.method(), 0xC8);
        assert_eq!(s.hatch_color.method(), 0xC0);
        assert_eq!(s.hatch_scale, 2.5);
        assert_eq!(s.hatch_spacing, 2.5);
        assert_eq!(s.hatch_angle_count, 6);
        assert!((s.hatch_angles[0] - FRAC_PI_2).abs() < 1e-12);
        assert!((s.hatch_angles[4] - 1.8325957145940461).abs() < 1e-12);
    }

    #[test]
    fn r2013_section_view_style_has_no_r2018_head() {
        let payload = build(Version::R2013);
        let s = decode_object(&payload, 8, None, Version::R2013).unwrap();
        assert_eq!(s.name, "Metric50");
        assert_eq!(s.name_alias, "");
        assert_eq!(s.identifier_exclude_characters, EXCLUDE);
        assert!(decode_object(&payload, 8, None, Version::R2018).is_err());
    }

    /// The R2004 layout: four inline `TV` fields, closing on the `RL`
    /// object-data-size boundary.
    #[test]
    fn r2004_section_view_style_reads_its_strings_inline() {
        let mut w = modern::tests::r2004_object_prefix(1);
        write_body(&mut w, Version::R2004, true);
        let end = w.position_bits();
        let bytes = w.into_bytes();
        let s = decode_object(&bytes, 0, Some(end), Version::R2004).unwrap();
        assert_eq!(s.name, "Metric50");
        assert_eq!(s.identifier_exclude_characters, EXCLUDE);
        assert_eq!(s.hatch_pattern_name, "ANSI31");
        assert_eq!(s.hatch_scale, 2.5);
    }
}
