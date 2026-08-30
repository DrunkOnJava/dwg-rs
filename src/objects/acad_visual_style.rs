//! ACAD_VISUALSTYLE object — named display style (face lighting model,
//! edge rendering, silhouette, shadows). R2004 and newer.
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
//! # The wire shape — measured, R2010 and newer
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
//! # The wire shape — measured, R2004 (the flag-less generation)
//!
//! `arc_2004.dwg` stores the same 24 shipped styles with **no**
//! per-property flags, one fewer property, and a visibly different
//! order. This is the list that lands all 24 records of each of
//! `arc_2004.dwg`, `circle_2004.dwg` and `line_2004.dwg` exactly on
//! their `RL` object-data-size boundary — 72 records, delta 0:
//!
//! ```text
//! TV   description                  -- inline on R2004, string stream on R2007
//! BS   internal_style_type          BS  face_lighting_model
//! BS   face_lighting_quality        BS  face_color_mode
//! BD   face_opacity                 BD  face_specular
//! CMC  face_mono_color              BS  face_modifier
//! BS   edge_model                   BS  edge_style
//! CMC  edge_intersection_color      CMC edge_obscured_color
//! BS   edge_obscured_linetype       BD  edge_crease_angle
//! BS   edge_modifier                CMC edge_color
//! BD   edge_opacity                 BS  edge_width
//! BS   edge_overhang                BS  edge_jitter
//! CMC  edge_silhouette_color        BS  edge_silhouette_width
//! RC   edge_unknown_byte            BS  edge_halo_gap
//! B    edge_hide_precision          BS  edge_isoline_count
//! BS   edge_intersection_linetype   BS  edge_style_apply
//! BL   display_brightness           BS  display_shadow_type
//! B    is_internal_use_only
//! ```
//!
//! Three slots of the R2010 head are gone or moved: there is no
//! `format_version` at all, `face_modifier` sits after
//! `face_mono_color` rather than before `face_opacity`, and
//! `is_internal_use_only` is the record's **last bit** rather than part
//! of its head.
//!
//! # Why the R2004 list is not a guess either
//!
//! The same boundary rule arbitrates it — the record's `RL`
//! object-data-size from the object prologue — and every slot but two
//! is cross-checked against the value the *same style* decodes on
//! `arc_2010.dwg`, where the layout is independently proven:
//!
//! - `internal_style_type` agrees on all 24 (`0` `Flat` … `27`
//!   `Shaded`);
//! - `face_lighting_model`, `face_color_mode`, `face_modifier`,
//!   `edge_model`, `edge_style`, `edge_modifier`, `edge_width`,
//!   `edge_overhang`, `edge_jitter`, `edge_silhouette_width`,
//!   `edge_style_apply` and `display_shadow_type` agree on all 24 —
//!   including the values that vary style by style: `edge_modifier`
//!   `8`/`0`/`10`/`11`/`12`, `edge_silhouette_width` `3`/`5`/`6`,
//!   `edge_style_apply` `1`/`5`/`13`;
//! - all five `CMC`s agree on all 24, `ColorChange`'s grey
//!   `0xC2808080` face and edge colours and `Shaded`'s `0xC2787878`
//!   silhouette included;
//! - `edge_crease_angle` agrees on all 24: `40` on `Hidden`,
//!   `Shades of Gray` and `Sketchy`, `179` on `Conceptual`, `1`
//!   elsewhere;
//! - `edge_obscured_linetype` agrees on all 24 (`2` on `Hidden` and
//!   `Shaded with edges`, `7` on `Linepattern`, `1` elsewhere) and so
//!   does `edge_intersection_linetype` (`7` on `Linepattern`, `1`
//!   elsewhere) — the pair that pins the two ends of the trailing run;
//! - `is_internal_use_only`, the record's final bit, splits the 24
//!   styles into exactly the ten AutoCAD's Visual Styles Manager lists
//!   and the fourteen it hides, the same partition R2010 produces from
//!   a bit 500-odd positions earlier.
//!
//! The two slots that do not agree literally are informative rather
//! than worrying:
//!
//! - `display_brightness` is a **`BL`** here where R2010 spends a `BD`,
//!   and it decodes `-50`, `50` and `0` against R2010's `-50.0`, `50.0`
//!   and `0.0` — `Dim`, `Brighten` and the other 22. The `BL` is also
//!   what explains the corpus's bit budgets: `Dim` spends 32 more bits
//!   than its neighbours (full `BL` vs the `0` form) and `Brighten` 8
//!   more (byte `BL`), and nothing else in the record varies between
//!   those three styles.
//! - `face_opacity` and `face_specular` are **signed** here where
//!   R2010's are not. Their magnitudes agree with R2010 on all 24
//!   records; the sign tracks whether the property applies to the
//!   style. `face_specular` is positive on exactly the seven styles
//!   that shade faces (`Flat`, `FlatWithEdges`, `Gouraud`,
//!   `GouraudWithEdges`, `Realistic`, `Shaded`, `Shaded with edges`)
//!   and negative on the other seventeen; `face_opacity` is positive on
//!   exactly `X-Ray`, the one translucent style, and negative on the
//!   other twenty-three. The values are surfaced as measured, sign
//!   included.
//!
//! One further value differs for content rather than layout reasons:
//! `Realistic` decodes `face_lighting_quality` `2` on R2004 and `3` on
//! R2010. Every other field of that record agrees, so this is the style
//! being clamped when it is saved down, not a mis-read field.
//!
//! ## What the R2004 list does *not* determine
//!
//! Thirteen bits sit between `edge_silhouette_width` and
//! `edge_intersection_linetype`, and they hold the constant
//! `0b0_0000_0001_0010` on **every one of the 72 corpus records**. A
//! run that never varies cannot have its internal boundaries measured,
//! and several token splits fit it. This module reads it as `RC` `BS`
//! `B` `BS` — the only fit whose first token is a whole byte — and names
//! the last three for the three properties R2010 places in exactly this
//! position, all of which decode `0`, `false` and `0` there on the same
//! 24 styles. The leading byte has no R2010 counterpart and is
//! surfaced as [`AcadVisualStyle::edge_unknown_byte`] rather than given
//! a plausible-sounding name. Only the run's **total width** and its
//! constant contents are evidence; the boundaries inside it are a
//! reading, and a future R2004 file with a non-default halo gap or
//! isoline count would settle them.
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
//! | R2000 and earlier | `CMC` has no true-colour word; [`Error::Unsupported`] |
//! | R2004 | 30 flag-less fields — closes on all 72 records of the three `*_2004.dwg` files |
//! | R2007 | not measurable — [`Error::Unsupported`]; see below |
//! | R2010 | 28 `(value, flag)` properties — closes on all 24 records of `arc_2010.dwg` |
//! | R2013 / R2018 | 58 properties — closes on all 24 records of `arc_2013.dwg` and all 24 of `sample_AC1032.dwg` |
//!
//! **R2007 stays declined, and not because of this object.** Two
//! upstream gaps stand between an `AC1021` file and any decoder:
//! the container parse returns `SectionMapStatus::Deferred` for R2007
//! (the second Sec_Mask obfuscation layer is not implemented), and
//! `crate::string_stream::data_section_end` locates the split-stream
//! trailer from the leading `MC` handle-stream size that only R2010 and
//! newer write, so it declines R2007 outright. No R2007 object of any
//! type therefore reaches this module, the corpus cannot say whether
//! R2007 writes the flag-less list or the paired one, and this module
//! returns [`Error::Unsupported`] rather than shipping a reading no
//! byte has ever confirmed. The 24 VISUALSTYLE records in each of
//! `arc_2007.dwg`, `circle_2007.dwg` and `line_2007.dwg` are invisible
//! to `examples/coverage_report.rs` for the same reason.

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
    /// `2` in every R2010+ record measured. The R2004 layout has no
    /// such field, and leaves this `0`.
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
    /// R2004 only: the leading byte of the constant 13-bit run between
    /// `edge_silhouette_width` and `edge_intersection_linetype`. `0` on
    /// every corpus record, with no R2010 counterpart — see the module
    /// docs. Always `0` on R2010 and newer, which have no such slot.
    pub edge_unknown_byte: u8,
    /// Edge property (positional name): halo gap.
    pub edge_halo_gap: i16,
    /// Edge property (positional name): isoline count.
    pub edge_isoline_count: i16,
    /// The single `B`-typed property of the list.
    pub edge_hide_precision: bool,
    /// Edge property (positional name): style-apply bits.
    pub edge_style_apply: i16,
    /// Display property (positional name): brightness. A `BD` on
    /// R2010+ and a whole-number `BL` on R2004, widened here.
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
/// R2004 uses the flag-less field list, R2010 and newer the
/// `(value, flag)` one. Returns [`Error::Unsupported`] for R2007 and
/// earlier — see the module docs for why neither band can be measured.
pub fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadVisualStyle> {
    match version {
        Version::R2004 => decode_legacy(payload, body_start, inline_data_end, version),
        _ if version.is_r2010_plus() => {
            decode_paired(payload, body_start, inline_data_end, version)
        }
        _ => Err(Error::Unsupported {
            feature: format!(
                "ACAD_VISUALSTYLE layout is determined for R2004 and R2010 or newer; got {}",
                version.release()
            ),
        }),
    }
}

/// Read one `CMC` outside the `(value, flag)` reader.
fn read_color(c: &mut BitCursor<'_>) -> Result<VisualStyleColor> {
    let (index, rgb, color_byte) = crate::tables::modern::read_cmc_full(c)?;
    Ok(VisualStyleColor {
        index,
        rgb,
        color_byte,
    })
}

/// The R2004 field list — 30 fields, no per-property flags.
fn decode_legacy(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadVisualStyle> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let description = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let c = &mut split.data;
    let internal_style_type = c.read_bs()?;
    let face_lighting_model = c.read_bs()?;
    let face_lighting_quality = c.read_bs()?;
    let face_color_mode = c.read_bs()?;
    let face_opacity = c.read_bd()?;
    let face_specular = c.read_bd()?;
    let face_mono_color = read_color(c)?;
    let face_modifier = c.read_bs()?;
    let edge_model = c.read_bs()?;
    let edge_style = c.read_bs()?;
    let edge_intersection_color = read_color(c)?;
    let edge_obscured_color = read_color(c)?;
    let edge_obscured_linetype = c.read_bs()?;
    let edge_crease_angle = c.read_bd()?;
    let edge_modifier = c.read_bs()?;
    let edge_color = read_color(c)?;
    let edge_opacity = c.read_bd()?;
    let edge_width = c.read_bs()?;
    let edge_overhang = c.read_bs()?;
    let edge_jitter = c.read_bs()?;
    let edge_silhouette_color = read_color(c)?;
    let edge_silhouette_width = c.read_bs()?;
    // The constant 13-bit run — only its total width is evidence.
    let edge_unknown_byte = c.read_rc()?;
    let edge_halo_gap = c.read_bs()?;
    let edge_hide_precision = c.read_b()?;
    let edge_isoline_count = c.read_bs()?;
    let edge_intersection_linetype = c.read_bs()?;
    let edge_style_apply = c.read_bs()?;
    let display_brightness = f64::from(c.read_bl()?);
    let display_shadow_type = c.read_bs()?;
    let is_internal_use_only = c.read_b()?;

    split.finish("VISUALSTYLE")?;

    Ok(AcadVisualStyle {
        description,
        internal_style_type,
        format_version: 0,
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
        edge_unknown_byte,
        edge_halo_gap,
        edge_isoline_count,
        edge_hide_precision,
        edge_style_apply,
        display_brightness,
        display_shadow_type,
        property_flags: Vec::new(),
        extended: Vec::new(),
        trailing_strings: trailing_strings(&mut split),
    })
}

/// Drain whatever the record's string stream still holds.
fn trailing_strings(split: &mut modern::ObjectStream<'_>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(strings) = split.strings.as_mut() {
        while !strings.is_exhausted() {
            match strings.read_tv() {
                Ok(text) => out.push(text),
                Err(_) => break,
            }
        }
    }
    out
}

/// The R2010+ field list — 28 (R2010) or 58 (R2013+) `(value, flag)`
/// property pairs behind a three-field head.
fn decode_paired(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadVisualStyle> {
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

    let trailing_strings = trailing_strings(&mut split);

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
        edge_unknown_byte: 0,
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

    /// The R2004 field list: 30 fields, no flags, `is_internal_use_only`
    /// last. Mirrors the module's measured table.
    fn write_legacy_properties(w: &mut BitWriter) {
        w.write_bs(0); // internal_style_type
        w.write_bs(2); // face_lighting_model
        w.write_bs(1); // face_lighting_quality
        w.write_bs(1); // face_color_mode
        w.write_bd(-0.6); // face_opacity — signed on R2004
        w.write_bd(30.0); // face_specular
        cmc(w, 0, 0xC2FF_FFFF, 0); // face_mono_color
        w.write_bs(2); // face_modifier
        w.write_bs(0); // edge_model
        w.write_bs(0); // edge_style
        cmc(w, 0, 0xC300_0007, 0); // edge_intersection_color
        cmc(w, 0, 0xC800_0000, 0); // edge_obscured_color
        w.write_bs(1); // edge_obscured_linetype
        w.write_bd(1.0); // edge_crease_angle
        w.write_bs(8); // edge_modifier
        cmc(w, 0, 0xC300_0007, 0); // edge_color
        w.write_bd(1.0); // edge_opacity
        w.write_bs(1); // edge_width
        w.write_bs(6); // edge_overhang
        w.write_bs(2); // edge_jitter
        cmc(w, 0, 0xC300_0007, 0); // edge_silhouette_color
        w.write_bs(5); // edge_silhouette_width
        w.write_rc(0); // the constant run's leading byte
        w.write_bs(0); // edge_halo_gap
        w.write_b(false); // edge_hide_precision
        w.write_bs(0); // edge_isoline_count
        w.write_bs(1); // edge_intersection_linetype
        w.write_bs(13); // edge_style_apply
        w.write_bl(-50); // display_brightness — a BL on R2004
        w.write_bs(0); // display_shadow_type
        w.write_b(true); // is_internal_use_only
    }

    /// An R2004 record: inline `TV`, inline layout, `RL`-style boundary.
    fn build_r2004() -> (Vec<u8>, usize) {
        let mut body = modern::tests::r2004_object_prefix(1);
        modern::tests::write_inline_tv(&mut body, "Flat");
        write_legacy_properties(&mut body);
        let end = body.position_bits();
        (body.into_bytes(), end)
    }

    #[test]
    fn rejects_pre_r2004() {
        let payload = build(Version::R2018);
        let err = decode_object(&payload, 8, None, Version::R2000).unwrap_err();
        assert!(
            matches!(&err, Error::Unsupported { feature } if feature.contains("ACAD_VISUALSTYLE"))
        );
    }

    #[test]
    fn r2004_inline_visual_style_closes_on_its_object_data_size() {
        let (payload, end) = build_r2004();
        let style = decode_object(&payload, 0, Some(end), Version::R2004).unwrap();
        assert_eq!(style.description, "Flat");
        assert_eq!(style.internal_style_type, 0);
        // R2004 has no format_version slot at all.
        assert_eq!(style.format_version, 0);
        assert!(style.is_internal_use_only);
        assert_eq!(style.face_lighting_model, 2);
        assert!((style.face_opacity + 0.6).abs() < 1e-12);
        assert!((style.face_specular - 30.0).abs() < 1e-12);
        assert_eq!(style.face_modifier, 2);
        assert_eq!(style.face_mono_color.method(), 0xC2);
        assert_eq!(style.edge_obscured_color.method(), 0xC8);
        assert_eq!(style.edge_silhouette_width, 5);
        assert_eq!(style.edge_unknown_byte, 0);
        assert_eq!(style.edge_intersection_linetype, 1);
        assert_eq!(style.edge_style_apply, 13);
        assert!((style.display_brightness + 50.0).abs() < 1e-12);
        // No per-property flags exist on this release.
        assert!(style.property_flags.is_empty());
        assert!(style.extended.is_empty());
    }

    /// The flag-less body is 28 `BS` flags shorter than the paired one,
    /// so the boundary check has to reject it under the R2010 list.
    #[test]
    fn r2004_body_rejected_by_the_r2010_field_list() {
        let (payload, end) = build_r2004();
        assert!(decode_object(&payload, 0, Some(end), Version::R2010).is_err());
    }

    /// R2007 is declined, and the decline is an `Unsupported` — so a
    /// dispatched R2007 record lands in the Unhandled bucket rather than
    /// being reported as a broken record.
    #[test]
    fn rejects_r2007() {
        let (payload, end) = build_r2004();
        let err = decode_object(&payload, 0, Some(end), Version::R2007).unwrap_err();
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
