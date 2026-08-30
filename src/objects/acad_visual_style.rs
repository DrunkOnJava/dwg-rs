//! ACAD_VISUALSTYLE object — named display style (face lighting model,
//! edge rendering, silhouette, shadows). R2010+ only.
//!
//! # There is no spec prescription for this object
//!
//! The ODA *Open Design Specification for .dwg files* v5.4.1 lists
//! `VISUALSTYLE` in §20.3's table of non-fixed object types but §20.4
//! carries **no prescription** for it — the object-prescription chapter
//! runs from `20.4.1 Common Entity Data` to `20.4.104 XRECORD` and stops.
//! (An earlier revision of this module cited "§19.6.10 (L6-17)"; no such
//! section exists.) Everything below was therefore derived by measuring
//! real records against the boundary the format itself provides.
//!
//! # The wire shape — measured
//!
//! ```text
//! TV   description                  -- from the string stream on R2007+
//! BS   internal_style_type          -- 0..27, one per built-in style
//! BS   format_version               -- 2 in every record measured
//! B    is_internal_use_only
//! then one (value, BS flag) pair per property, in this order:
//!   BS  face_lighting_model          BS  face_lighting_quality
//!   BS  face_color_mode              BS  face_modifier
//!   BD  face_opacity                 BD  face_specular
//!   CMC face_mono_color              BS  edge_model
//!   BS  edge_style                   CMC edge_intersection_color
//!   CMC edge_obscured_color          BS  edge_obscured_linetype
//!   BS  edge_intersection_linetype   BD  edge_crease_angle
//!   BS  edge_modifier                CMC edge_color
//!   BD  edge_opacity                 BS  edge_width
//!   BS  edge_overhang                BS  edge_jitter
//!   CMC edge_silhouette_color        BS  edge_silhouette_width
//!   BS  edge_halo_gap                BS  edge_isoline_count
//!   B   edge_hide_precision          BS  edge_style_apply
//!   BD  display_brightness           BS  display_shadow_type
//! R2013+ only: 30 further (value, flag) pairs, widths measured, names
//! not determined — see [`AcadVisualStyle::extended`].
//! ```
//!
//! `CMC` is the full colour form the crate-internal
//! `tables::modern::read_cmc_full` already reads for VIEW / VPORT:
//! `BS` index, `BL` true-colour word, `RC` colour byte. The true-colour words in these records carry the
//! usual method byte in the top octet — `0xC2RRGGBB` for an RGB colour,
//! `0xC3000000 | aci` for an index, `0xC0`/`0xC8` for ByLayer / none.
//!
//! # Why the `(value, flag)` pairing is not a guess
//!
//! Every record's data fields have to end exactly on the first bit of
//! its string stream — the boundary the crate-internal
//! `objects::modern::ObjectStream::finish` enforces for every
//! dispatched object decoder.
//! Searching for one token sequence that lands **all 24 VISUALSTYLE
//! records of a file** on that boundary — with every `BD` a plausible
//! double, every `CMC` true-colour word a real colour method, and every
//! flag a `BS` in `0..=7` — returns exactly **one** answer on
//! `arc_2010.dwg`, the 28-property list above. The same search on
//! `arc_2013.dwg` returns exactly one answer too: the identical 28
//! properties followed by 30 more.
//!
//! The pairing also explains the corpus's bit budgets. A flag of `1`
//! costs 10 bits (`BS` byte form), a flag of `0` costs 2, so records
//! differ by multiples of 8: the 24 records of `arc_2010.dwg` measure
//! 574 / 774 / 782 / 790 / 798 / 806 / 854 / 862 bits, and the three
//! internal styles (`JitterOff`, `OverhangOff`, `EdgeColorOff`) that
//! carry `0` in *every* flag are exactly the 574-bit ones.
//!
//! Corroboration from the decoded values themselves:
//!
//! - `face_opacity` decodes to `0.6` in every style but `X-Ray`, which
//!   decodes `0.5` — the one built-in style that is semi-transparent;
//! - `face_mono_color` decodes `0xC2FFFFFF` (white) everywhere except
//!   `ColorChange`, which decodes `0xC2808080`;
//! - `internal_style_type` decodes `0` for `Flat`, `1` for
//!   `FlatWithEdges`, `2` for `Gouraud`, `3` for `GouraudWithEdges`,
//!   `4` for `2dWireframe` … `27` for `Shaded` — a dense enumeration in
//!   the order AutoCAD ships the styles;
//! - `is_internal_use_only` — the lone `B` of the fixed head — decodes
//!   `false` for exactly ten of the 24 records: `2dWireframe`,
//!   `Wireframe`, `Hidden`, `Conceptual`, `Realistic`, `Shades of
//!   Gray`, `Sketchy`, `X-Ray`, `Shaded with edges`, `Shaded`. Those
//!   are precisely the ten styles AutoCAD's Visual Styles Manager
//!   lists; the fourteen it hides (`Flat`, `Gouraud`, `Basic`, `Dim`,
//!   `Brighten`, `Thicken`, `Linepattern`, `Facepattern`,
//!   `ColorChange`, `JitterOff`, …) all decode `true`. A field list
//!   off by one bit cannot reproduce that partition;
//! - `edge_crease_angle` decodes `40` on `Hidden`, `Sketchy` and
//!   `Shades of Gray`, `179` on `Conceptual` and `1` elsewhere —
//!   degrees, in the styles where a crease angle is meaningful;
//! - the sole `B`-typed property of the pair list falls where the
//!   conventional `AcDbVisualStyle` property order puts its sole
//!   boolean.
//!
//! # Naming
//!
//! The field **types, widths and order** above are measured. The
//! **names** follow the conventional `AcDbVisualStyle` property
//! ordering; the face group is corroborated by the decoded values as
//! described above, the edge and display groups are positional and are
//! not independently corroborated by this crate. Treat them as labels
//! for slots whose layout is proven, not as verified semantics.
//!
//! # Versions
//!
//! | Release | Status |
//! |---|---|
//! | R2010 | 28 properties — closes on all 24 records of `arc_2010.dwg` |
//! | R2013 / R2018 | 58 properties — closes on all 24 records of `arc_2013.dwg` and all 24 of `sample_AC1032.dwg` |
//! | R2004 / R2007 | layout differs and is **not** determined; [`Error::Unsupported`] |
//!
//! `arc_2004.dwg` stores the same styles without the per-property flags
//! and with a different property count; no token sequence over
//! `BS/BD/CMC/B` lands its 24 records on their `RL` object-data-size
//! boundary, so this module declines that band rather than guessing.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::version::Version;

/// One `CMC` colour: `BS` index, `BL` true-colour word, `RC` colour byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VisualStyleColor {
    /// Colour index word.
    pub index: u16,
    /// True-colour word; the top octet is the colour method
    /// (`0xC2` RGB, `0xC3` ACI index, `0xC0` ByLayer, `0xC8` none).
    pub rgb: u32,
    /// Trailing colour byte; `1` and `2` would introduce name strings.
    pub color_byte: u8,
}

impl VisualStyleColor {
    /// The colour method octet — the top byte of the true-colour word.
    pub fn method(&self) -> u8 {
        (self.rgb >> 24) as u8
    }

    /// The 24-bit payload of the true-colour word: an RGB triple when
    /// [`method`](Self::method) is `0xC2`, an ACI index when it is `0xC3`.
    pub fn payload(&self) -> u32 {
        self.rgb & 0x00FF_FFFF
    }
}

/// One R2013+ tail property: a value of measured width, plus its flag.
#[derive(Debug, Clone, PartialEq)]
pub enum VisualStyleValue {
    /// A `BS` slot.
    Short(i16),
    /// A `BD` slot.
    Double(f64),
    /// A `B` slot.
    Bool(bool),
    /// A `CMC` slot.
    Color(VisualStyleColor),
}

/// A `(value, flag)` property pair.
#[derive(Debug, Clone, PartialEq)]
pub struct VisualStyleProperty {
    /// The property's value.
    pub value: VisualStyleValue,
    /// The trailing `BS` flag; `0` on the styles AutoCAD marks internal.
    pub flag: i16,
}

/// A decoded ACAD_VISUALSTYLE record.
#[derive(Debug, Clone, PartialEq)]
pub struct AcadVisualStyle {
    /// Style name / description, from the R2007+ string stream.
    pub description: String,
    /// Dense per-style enumeration, `0` (`Flat`) … `27` (`Shaded`).
    pub internal_style_type: i16,
    /// `2` in every record measured.
    pub format_version: i16,
    /// Set on the styles AutoCAD does not offer in the UI.
    pub is_internal_use_only: bool,
    /// Face property: lighting model.
    pub face_lighting_model: i16,
    /// Face property: lighting quality.
    pub face_lighting_quality: i16,
    /// Face property: colour mode.
    pub face_color_mode: i16,
    /// Face property: modifier bits.
    pub face_modifier: i16,
    /// Face property: opacity, `0.5` on `X-Ray` and `0.6` elsewhere.
    pub face_opacity: f64,
    /// Face property: specular / highlight level.
    pub face_specular: f64,
    /// Face property: monochrome colour.
    pub face_mono_color: VisualStyleColor,
    /// Edge property (positional name): edge model.
    pub edge_model: i16,
    /// Edge property (positional name): edge style bits.
    pub edge_style: i16,
    /// Edge property (positional name): intersection colour.
    pub edge_intersection_color: VisualStyleColor,
    /// Edge property (positional name): obscured colour.
    pub edge_obscured_color: VisualStyleColor,
    /// Edge property (positional name): obscured linetype.
    pub edge_obscured_linetype: i16,
    /// Edge property (positional name): intersection linetype.
    pub edge_intersection_linetype: i16,
    /// Edge property (positional name): crease angle.
    pub edge_crease_angle: f64,
    /// Edge property (positional name): modifier bits.
    pub edge_modifier: i16,
    /// Edge property (positional name): edge colour.
    pub edge_color: VisualStyleColor,
    /// Edge property (positional name): edge opacity.
    pub edge_opacity: f64,
    /// Edge property (positional name): edge width.
    pub edge_width: i16,
    /// Edge property (positional name): overhang.
    pub edge_overhang: i16,
    /// Edge property (positional name): jitter.
    pub edge_jitter: i16,
    /// Edge property (positional name): silhouette colour.
    pub edge_silhouette_color: VisualStyleColor,
    /// Edge property (positional name): silhouette width.
    pub edge_silhouette_width: i16,
    /// Edge property (positional name): halo gap.
    pub edge_halo_gap: i16,
    /// Edge property (positional name): isoline count.
    pub edge_isoline_count: i16,
    /// The single `B`-typed property of the list.
    pub edge_hide_precision: bool,
    /// Edge property (positional name): style-apply bits.
    pub edge_style_apply: i16,
    /// Display property (positional name): brightness.
    pub display_brightness: f64,
    /// Display property (positional name): shadow type.
    pub display_shadow_type: i16,
    /// The `BS` flag of each property above, in wire order.
    pub property_flags: Vec<i16>,
    /// R2013+ tail: 30 further `(value, flag)` pairs. Their widths are
    /// measured — the record does not close on its string stream
    /// without them — but their names are not determined, so they are
    /// surfaced positionally rather than mislabelled.
    pub extended: Vec<VisualStyleProperty>,
    /// String-stream entries the field list does not place. Every
    /// corpus record carries exactly one (`strokes_ogs.tif`); which
    /// property slot owns it is not determined, because a `TV` costs no
    /// data-stream bits and so leaves no measurable trace.
    pub trailing_strings: Vec<String>,
}

/// Reads the `(value, flag)` pairs of one record, collecting the flags.
struct PropertyReader<'a, 'b> {
    cursor: &'b mut BitCursor<'a>,
    flags: Vec<i16>,
}

impl PropertyReader<'_, '_> {
    fn flag(&mut self) -> Result<i16> {
        let flag = self.cursor.read_bs()?;
        self.flags.push(flag);
        Ok(flag)
    }

    fn short(&mut self) -> Result<i16> {
        let v = self.cursor.read_bs()?;
        self.flag()?;
        Ok(v)
    }

    fn double(&mut self) -> Result<f64> {
        let v = self.cursor.read_bd()?;
        self.flag()?;
        Ok(v)
    }

    fn boolean(&mut self) -> Result<bool> {
        let v = self.cursor.read_b()?;
        self.flag()?;
        Ok(v)
    }

    fn color(&mut self) -> Result<VisualStyleColor> {
        let (index, rgb, color_byte) = crate::tables::modern::read_cmc_full(self.cursor)?;
        self.flag()?;
        Ok(VisualStyleColor {
            index,
            rgb,
            color_byte,
        })
    }

    fn property(&mut self, kind: TailKind) -> Result<VisualStyleProperty> {
        let value = match kind {
            TailKind::Short => VisualStyleValue::Short(self.cursor.read_bs()?),
            TailKind::Double => VisualStyleValue::Double(self.cursor.read_bd()?),
            TailKind::Bool => VisualStyleValue::Bool(self.cursor.read_b()?),
            TailKind::Color => {
                let (index, rgb, color_byte) = crate::tables::modern::read_cmc_full(self.cursor)?;
                VisualStyleValue::Color(VisualStyleColor {
                    index,
                    rgb,
                    color_byte,
                })
            }
        };
        let flag = self.flag()?;
        Ok(VisualStyleProperty { value, flag })
    }
}

/// Token type of one R2013+ tail property.
#[derive(Debug, Clone, Copy)]
enum TailKind {
    Short,
    Double,
    Bool,
    Color,
}

/// The 30 R2013+ tail property widths, measured on `arc_2013.dwg` and
/// `sample_AC1032.dwg`. This is the only token sequence over
/// `BS`/`BD`/`B`/`CMC` that lands all 24 records of either file exactly
/// on their string-stream start.
const R2013_TAIL: [TailKind; 30] = [
    TailKind::Bool,
    TailKind::Bool,
    TailKind::Bool,
    TailKind::Bool,
    TailKind::Bool,
    TailKind::Bool,
    TailKind::Bool,
    TailKind::Bool,
    TailKind::Bool,
    TailKind::Short,
    TailKind::Short,
    TailKind::Double,
    TailKind::Short,
    TailKind::Color,
    TailKind::Short,
    TailKind::Short,
    TailKind::Color,
    TailKind::Bool,
    TailKind::Short,
    TailKind::Short,
    TailKind::Short,
    TailKind::Bool,
    TailKind::Short,
    TailKind::Color,
    TailKind::Bool,
    TailKind::Bool,
    TailKind::Short,
    TailKind::Bool,
    TailKind::Double,
    TailKind::Double,
];

/// Decode an ACAD_VISUALSTYLE straight from its raw object payload,
/// taking its `TV` fields from the R2007+ string stream and checking
/// that the data fields end exactly on the data-stream boundary.
///
/// Returns [`Error::Unsupported`] for R2007 and earlier, whose layout
/// this crate has not matched against real bytes.
pub fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadVisualStyle> {
    if !version.is_r2010_plus() {
        return Err(Error::Unsupported {
            feature: format!(
                "ACAD_VISUALSTYLE layout is only determined for R2010 or newer; got {}",
                version.release()
            ),
        });
    }
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let description = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let internal_style_type = split.data.read_bs()?;
    let format_version = split.data.read_bs()?;
    let is_internal_use_only = split.data.read_b()?;

    let mut reader = PropertyReader {
        cursor: &mut split.data,
        flags: Vec::with_capacity(58),
    };
    let face_lighting_model = reader.short()?;
    let face_lighting_quality = reader.short()?;
    let face_color_mode = reader.short()?;
    let face_modifier = reader.short()?;
    let face_opacity = reader.double()?;
    let face_specular = reader.double()?;
    let face_mono_color = reader.color()?;
    let edge_model = reader.short()?;
    let edge_style = reader.short()?;
    let edge_intersection_color = reader.color()?;
    let edge_obscured_color = reader.color()?;
    let edge_obscured_linetype = reader.short()?;
    let edge_intersection_linetype = reader.short()?;
    let edge_crease_angle = reader.double()?;
    let edge_modifier = reader.short()?;
    let edge_color = reader.color()?;
    let edge_opacity = reader.double()?;
    let edge_width = reader.short()?;
    let edge_overhang = reader.short()?;
    let edge_jitter = reader.short()?;
    let edge_silhouette_color = reader.color()?;
    let edge_silhouette_width = reader.short()?;
    let edge_halo_gap = reader.short()?;
    let edge_isoline_count = reader.short()?;
    let edge_hide_precision = reader.boolean()?;
    let edge_style_apply = reader.short()?;
    let display_brightness = reader.double()?;
    let display_shadow_type = reader.short()?;

    let mut extended = Vec::new();
    if matches!(version, Version::R2013 | Version::R2018) {
        extended.reserve(R2013_TAIL.len());
        for kind in R2013_TAIL {
            extended.push(reader.property(kind)?);
        }
    }
    let property_flags = reader.flags;

    split.finish("VISUALSTYLE")?;

    let mut trailing_strings = Vec::new();
    if let Some(strings) = split.strings.as_mut() {
        while !strings.is_exhausted() {
            match strings.read_tv() {
                Ok(text) => trailing_strings.push(text),
                Err(_) => break,
            }
        }
    }

    Ok(AcadVisualStyle {
        description,
        internal_style_type,
        format_version,
        is_internal_use_only,
        face_lighting_model,
        face_lighting_quality,
        face_color_mode,
        face_modifier,
        face_opacity,
        face_specular,
        face_mono_color,
        edge_model,
        edge_style,
        edge_intersection_color,
        edge_obscured_color,
        edge_obscured_linetype,
        edge_intersection_linetype,
        edge_crease_angle,
        edge_modifier,
        edge_color,
        edge_opacity,
        edge_width,
        edge_overhang,
        edge_jitter,
        edge_silhouette_color,
        edge_silhouette_width,
        edge_halo_gap,
        edge_isoline_count,
        edge_hide_precision,
        edge_style_apply,
        display_brightness,
        display_shadow_type,
        property_flags,
        extended,
        trailing_strings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Write one `(value, flag)` pair's flag.
    fn flag(w: &mut BitWriter, v: i16) {
        w.write_bs(v);
    }

    fn cmc(w: &mut BitWriter, index: u16, rgb: u32, color_byte: u8) {
        w.write_bs_u(index);
        w.write_bl_u(rgb);
        w.write_rc(color_byte);
    }

    /// Build the 28-property R2010 body every release shares.
    fn write_common_properties(w: &mut BitWriter) {
        w.write_bs(2); // face_lighting_model
        flag(w, 1);
        w.write_bs(1); // face_lighting_quality
        flag(w, 1);
        w.write_bs(1); // face_color_mode
        flag(w, 1);
        w.write_bs(2); // face_modifier
        flag(w, 1);
        w.write_bd(0.6); // face_opacity
        flag(w, 1);
        w.write_bd(30.0); // face_specular
        flag(w, 1);
        cmc(w, 0, 0xC2FF_FFFF, 0); // face_mono_color
        flag(w, 1);
        w.write_bs(0); // edge_model
        flag(w, 1);
        w.write_bs(0); // edge_style
        flag(w, 1);
        cmc(w, 0, 0xC300_0007, 0); // edge_intersection_color
        flag(w, 1);
        cmc(w, 0, 0xC800_0000, 0); // edge_obscured_color
        flag(w, 1);
        w.write_bs(1); // edge_obscured_linetype
        flag(w, 1);
        w.write_bs(1); // edge_intersection_linetype
        flag(w, 1);
        w.write_bd(1.0); // edge_crease_angle
        flag(w, 1);
        w.write_bs(8); // edge_modifier
        flag(w, 1);
        cmc(w, 0, 0xC300_0007, 0); // edge_color
        flag(w, 1);
        w.write_bd(1.0); // edge_opacity
        flag(w, 1);
        w.write_bs(1); // edge_width
        flag(w, 1);
        w.write_bs(6); // edge_overhang
        flag(w, 1);
        w.write_bs(2); // edge_jitter
        flag(w, 1);
        cmc(w, 0, 0xC300_0007, 0); // edge_silhouette_color
        flag(w, 1);
        w.write_bs(5); // edge_silhouette_width
        flag(w, 1);
        w.write_bs(0); // edge_halo_gap
        flag(w, 1);
        w.write_bs(0); // edge_isoline_count
        flag(w, 1);
        w.write_b(false); // edge_hide_precision
        flag(w, 1);
        w.write_bs(13); // edge_style_apply
        flag(w, 1);
        w.write_bd(0.0); // display_brightness
        flag(w, 1);
        w.write_bs(0); // display_shadow_type
        flag(w, 1);
    }

    fn write_r2013_tail(w: &mut BitWriter) {
        for kind in R2013_TAIL {
            match kind {
                TailKind::Short => w.write_bs(50),
                TailKind::Double => w.write_bd(1.0),
                TailKind::Bool => w.write_b(true),
                TailKind::Color => cmc(w, 0, 0xC200_0000, 0),
            }
            flag(w, 1);
        }
    }

    /// The common object data an R2010 non-entity object leads with:
    /// empty EED chain, `BL` reactor count, no xdictionary. R2010
    /// predates the R2013+ AcDs binary-data bit.
    fn r2010_object_prefix(num_reactors: i32) -> BitWriter {
        let mut w = BitWriter::new();
        w.write_bs_u(0);
        w.write_bl(num_reactors);
        w.write_b(true);
        w
    }

    fn build(version: Version) -> Vec<u8> {
        let mut body = if matches!(version, Version::R2013 | Version::R2018) {
            modern::tests::r2018_object_prefix(1)
        } else {
            r2010_object_prefix(1)
        };
        body.write_bs(0); // internal_style_type
        body.write_bs(2); // format_version
        body.write_b(true); // is_internal_use_only
        write_common_properties(&mut body);
        if matches!(version, Version::R2013 | Version::R2018) {
            write_r2013_tail(&mut body);
        }
        let bits = crate::string_stream::tests::bits_of(&body);
        crate::string_stream::tests::build_payload(&bits, &["Flat", "strokes_ogs.tif"])
    }

    #[test]
    fn rejects_pre_r2010() {
        let payload = build(Version::R2018);
        let err = decode_object(&payload, 8, None, Version::R2004).unwrap_err();
        assert!(
            matches!(&err, Error::Unsupported { feature } if feature.contains("ACAD_VISUALSTYLE"))
        );
    }

    #[test]
    fn r2018_split_stream_visual_style_closes_on_its_string_stream() {
        let payload = build(Version::R2018);
        let style = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(style.description, "Flat");
        assert_eq!(style.internal_style_type, 0);
        assert_eq!(style.format_version, 2);
        assert!(style.is_internal_use_only);
        assert_eq!(style.face_lighting_model, 2);
        assert!((style.face_opacity - 0.6).abs() < 1e-12);
        assert!((style.face_specular - 30.0).abs() < 1e-12);
        assert_eq!(style.face_mono_color.method(), 0xC2);
        assert_eq!(style.face_mono_color.payload(), 0x00FF_FFFF);
        assert_eq!(style.edge_intersection_color.method(), 0xC3);
        assert_eq!(style.edge_intersection_color.payload(), 7);
        assert!(!style.edge_hide_precision);
        assert_eq!(style.display_shadow_type, 0);
        assert_eq!(style.property_flags.len(), 58);
        assert_eq!(style.extended.len(), 30);
        assert_eq!(style.trailing_strings, vec!["strokes_ogs.tif".to_string()]);
    }

    #[test]
    fn r2010_visual_style_has_no_r2013_tail() {
        let payload = build(Version::R2010);
        let style = decode_object(&payload, 8, None, Version::R2010).unwrap();
        assert_eq!(style.description, "Flat");
        assert_eq!(style.property_flags.len(), 28);
        assert!(style.extended.is_empty());
    }

    /// The R2013 tail is not optional: reading an R2013 record with the
    /// R2010 field list leaves 30 properties unread, and the boundary
    /// check has to reject that rather than return a plausible struct.
    #[test]
    fn r2013_body_rejected_by_the_r2010_field_list() {
        let payload = build(Version::R2018);
        assert!(decode_object(&payload, 8, None, Version::R2010).is_err());
    }

    /// …and the converse: an R2010-length body must not satisfy the
    /// R2013 field list.
    #[test]
    fn r2010_body_rejected_by_the_r2013_field_list() {
        let payload = build(Version::R2010);
        assert!(decode_object(&payload, 8, None, Version::R2018).is_err());
    }
}
