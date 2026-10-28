//! ACDBDETAILVIEWSTYLE object — the style record behind a model
//! documentation *detail view* (identifier symbol, detail boundary,
//! connection line, view label).
//!
//! # There is no spec prescription for this object
//!
//! The ODA *Open Design Specification for .dwg files* v5.4.1 has **no
//! §20.4 entry** for `ACDBDETAILVIEWSTYLE`; its object-prescription
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
//! BS   flags                         -- 32 in every corpus record
//! B    flag_c   B    flag_d   B    flag_e
//! CMC  identifier_color              BD   identifier_height
//! TV   identifier_symbol             BD   identifier_offset
//! RC   identifier_placement
//! CMC  arrow_symbol_color            BD   arrow_symbol_size
//! BS   boundary_line_weight          CMC  boundary_line_color
//! CMC  connection_line_color         BD   view_label_text_height
//! BS   view_label_position           BD   view_label_offset
//! BS   view_label_attachment         TV   view_label_pattern
//! BS   connection_line_weight        CMC  view_label_color
//! BS   model_edge_line_weight        CMC  model_edge_color
//! RC   trailing_flags
//! ```
//!
//! `H` fields (the text style, the arrow block, the linetypes) live in
//! the handle stream and cost no data-stream bits, so they do not
//! appear; this is the data stream only.
//!
//! # Why this is not a guess
//!
//! Every record's data fields have to end exactly on the first bit of
//! its string stream (R2010+) or on its `RL` object-data-size (R2004) —
//! the boundary `objects::modern::ObjectStream::finish` enforces. The
//! list above closes **all eleven** ACDBDETAILVIEWSTYLE records of the
//! corpus with delta 0:
//!
//! | Release | Records | Budget | Delta |
//! |---|---|---|---|
//! | R2004 | 3 (`{arc,circle,line}_2004.dwg` handle 107) | 1209 | 0 |
//! | R2010 | 3 (`*_2010.dwg` handle 107) | 667 | 0 |
//! | R2013 | 3 (`*_2013.dwg` handle 107) | 667 | 0 |
//! | R2018 | 2 (`sample_AC1032.dwg` handles 428, 924) | 677 / 605 | 0 |
//!
//! The two R2018 records are the discriminating evidence. `Imperial24`
//! (handle 428) and `Metric50` (handle 924) differ by 72 bits, and the
//! field list accounts for the difference exactly: `Imperial24` writes
//! `view_label_position` in the 10-bit `BS` byte form where `Metric50`
//! writes the 2-bit zero form (+10), and writes a full 66-bit `BD` in
//! the `identifier_offset` slot where `Metric50` writes the 2-bit zero
//! form (+64), less the 2 bits the zero form still costs. Only one
//! placement of the two extra R2018 head bits survives both records:
//! putting them *after* the leading `BS` keeps `flags` reading `32` —
//! the value R2004, R2010 and R2013 all carry — while putting them
//! before it turns `flags` into `72` and desynchronises the first
//! `CMC` on both records.
//!
//! Corroboration from the decoded values:
//!
//! - `identifier_height`, `arrow_symbol_size` and
//!   `view_label_text_height` decode `5` on every `Metric50` record and
//!   `0.24` on every `Imperial24` record — the shipped text heights of
//!   AutoCAD's two view-style presets;
//! - `view_label_offset` decodes `15` (metric) / `0.75` (imperial);
//! - `boundary_line_weight`, `connection_line_weight` and
//!   `model_edge_line_weight` all decode `25` — 0.25 mm in the DWG
//!   hundredths-of-a-millimetre lineweight encoding;
//! - all five `CMC` slots decode true-colour word `0xC0000000`, the
//!   ByLayer method octet;
//! - `view_label_pattern` decodes AutoCAD's field expression
//!   `%<\AcVar ViewDetailId>% (%<\AcVar ViewScale \f "%sn">%)` on the
//!   metric records, and the `ViewType`-prefixed imperial variant on
//!   the others — and it decodes there *because* the field list places
//!   an inline `TV` at exactly that offset on R2004, where the string
//!   costs 458 data-stream bits.
//!
//! # Naming
//!
//! The field **types, widths and order** are measured. The **names**
//! follow the vocabulary of AutoCAD's Detail View Style dialog
//! (published product documentation — see `CLEANROOM.md`) applied in
//! wire order. The identifier, arrow, lineweight and view-label slots
//! are corroborated by the values above; the `unknown_*` and `flag_*`
//! slots are positional labels for slots whose layout is proven and
//! whose meaning is not. Treat them accordingly.

use crate::error::Result;
use crate::objects::color::{self, ObjectColor};
use crate::objects::modern;
use crate::version::Version;

/// A decoded ACDBDETAILVIEWSTYLE record.
#[derive(Debug, Clone, PartialEq)]
pub struct AcadDetailViewStyle {
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
    /// Style flag word; `32` in every corpus record.
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
    /// Identifier symbol string; empty in every corpus record.
    pub identifier_symbol: String,
    /// Identifier offset from the boundary.
    pub identifier_offset: f64,
    /// Identifier placement selector.
    pub identifier_placement: u8,
    /// Arrow symbol colour.
    pub arrow_symbol_color: ObjectColor,
    /// Arrow symbol size.
    pub arrow_symbol_size: f64,
    /// Detail-boundary lineweight, hundredths of a millimetre.
    pub boundary_line_weight: i16,
    /// Detail-boundary colour.
    pub boundary_line_color: ObjectColor,
    /// Connection-line colour.
    pub connection_line_color: ObjectColor,
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
    /// Connection-line lineweight, hundredths of a millimetre.
    pub connection_line_weight: i16,
    /// View-label colour.
    pub view_label_color: ObjectColor,
    /// Model-edge lineweight, hundredths of a millimetre.
    pub model_edge_line_weight: i16,
    /// Model-edge colour.
    pub model_edge_color: ObjectColor,
    /// Trailing `RC`; `0` in every corpus record. Positional name.
    pub trailing_flags: u8,
}

/// Decode an ACDBDETAILVIEWSTYLE straight from its raw object payload,
/// taking its `TV` fields from the R2007+ string stream and checking
/// that the data fields end exactly on the data-stream boundary.
pub fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadDetailViewStyle> {
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
    let identifier_symbol = modern::read_tv(&mut s.data, &mut s.strings, version)?;
    let identifier_offset = s.data.read_bd()?;
    let identifier_placement = s.data.read_rc()?;
    let arrow_symbol_color = color::read(&mut s.data, &mut s.strings, version)?;
    let arrow_symbol_size = s.data.read_bd()?;
    let boundary_line_weight = s.data.read_bs()?;
    let boundary_line_color = color::read(&mut s.data, &mut s.strings, version)?;
    let connection_line_color = color::read(&mut s.data, &mut s.strings, version)?;
    let view_label_text_height = s.data.read_bd()?;
    let view_label_position = s.data.read_bs()?;
    let view_label_offset = s.data.read_bd()?;
    let view_label_attachment = s.data.read_bs()?;
    let view_label_pattern = modern::read_tv(&mut s.data, &mut s.strings, version)?;
    let connection_line_weight = s.data.read_bs()?;
    let view_label_color = color::read(&mut s.data, &mut s.strings, version)?;
    let model_edge_line_weight = s.data.read_bs()?;
    let model_edge_color = color::read(&mut s.data, &mut s.strings, version)?;
    let trailing_flags = s.data.read_rc()?;
    s.finish("ACDBDETAILVIEWSTYLE")?;
    Ok(AcadDetailViewStyle {
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
        identifier_symbol,
        identifier_offset,
        identifier_placement,
        arrow_symbol_color,
        arrow_symbol_size,
        boundary_line_weight,
        boundary_line_color,
        connection_line_color,
        view_label_text_height,
        view_label_position,
        view_label_offset,
        view_label_attachment,
        view_label_pattern,
        connection_line_weight,
        view_label_color,
        model_edge_line_weight,
        model_edge_color,
        trailing_flags,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    const LABEL: &str = "%<\\AcVar ViewDetailId>%";

    fn cmc(w: &mut BitWriter) {
        w.write_bs_u(0);
        w.write_bl_u(0xC000_0000);
        w.write_rc(0);
    }

    /// Write the `Metric50` field list; `inline_strings` selects the
    /// pre-R2007 layout, where the three `TV` slots consume data bits.
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
        w.write_bs(32);
        w.write_b(false);
        w.write_b(true);
        w.write_b(true);
        cmc(w);
        w.write_bd(5.0);
        if inline_strings {
            modern::tests::write_inline_tv(w, "");
        }
        w.write_bd(0.36);
        w.write_rc(1);
        cmc(w);
        w.write_bd(5.0);
        w.write_bs(25);
        cmc(w);
        cmc(w);
        w.write_bd(5.0);
        w.write_bs(0);
        w.write_bd(15.0);
        w.write_bs(1);
        if inline_strings {
            modern::tests::write_inline_tv(w, LABEL);
        }
        w.write_bs(25);
        cmc(w);
        w.write_bs(25);
        cmc(w);
        w.write_rc(0);
    }

    fn build(version: Version) -> Vec<u8> {
        let mut w = modern::tests::r2018_object_prefix(1);
        write_body(&mut w, version, false);
        let bits = crate::string_stream::tests::bits_of(&w);
        let strings: Vec<&str> = if matches!(version, Version::R2018) {
            vec!["Metric50", "Metric50", "", LABEL]
        } else {
            vec!["Metric50", "", LABEL]
        };
        crate::string_stream::tests::build_payload(&bits, &strings)
    }

    #[test]
    fn r2018_detail_view_style_closes_on_its_string_stream() {
        let payload = build(Version::R2018);
        let s = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(s.name, "Metric50");
        assert_eq!(s.name_alias, "Metric50");
        assert_eq!(s.flags, 32);
        assert_eq!(s.identifier_height, 5.0);
        assert_eq!(s.arrow_symbol_size, 5.0);
        assert_eq!(s.view_label_text_height, 5.0);
        assert_eq!(s.view_label_offset, 15.0);
        assert_eq!(s.boundary_line_weight, 25);
        assert_eq!(s.connection_line_weight, 25);
        assert_eq!(s.model_edge_line_weight, 25);
        assert_eq!(s.identifier_color.method(), 0xC0);
        assert_eq!(s.model_edge_color.method(), 0xC0);
        assert_eq!(s.view_label_pattern, LABEL);
        assert_eq!(s.trailing_flags, 0);
    }

    #[test]
    fn r2013_detail_view_style_has_no_r2018_head() {
        let payload = build(Version::R2013);
        let s = decode_object(&payload, 8, None, Version::R2013).unwrap();
        assert_eq!(s.name, "Metric50");
        assert_eq!(s.name_alias, "");
        assert!(!s.unknown_flag_a && !s.unknown_flag_b);
        assert_eq!(s.flags, 32);
        // The two extra R2018 head bits are not optional.
        assert!(decode_object(&payload, 8, None, Version::R2018).is_err());
    }

    /// The R2004 layout: three inline `TV` fields, closing on the `RL`
    /// object-data-size boundary.
    #[test]
    fn r2004_detail_view_style_reads_its_strings_inline() {
        let mut w = modern::tests::r2004_object_prefix(1);
        write_body(&mut w, Version::R2004, true);
        let end = w.position_bits();
        let bytes = w.into_bytes();
        let s = decode_object(&bytes, 0, Some(end), Version::R2004).unwrap();
        assert_eq!(s.name, "Metric50");
        assert_eq!(s.view_label_pattern, LABEL);
        assert_eq!(s.identifier_height, 5.0);
    }
}
