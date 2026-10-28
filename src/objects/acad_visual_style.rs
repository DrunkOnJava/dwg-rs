//! ACAD_VISUALSTYLE object — named display style (face lighting model,
//! edge rendering, silhouette, shadows). Every release this crate reads
//! ships the same 24 built-in styles, R14 included.
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
//! # The wire shape — measured, R14 through R2007 (the flag-less generation)
//!
//! `arc_2004.dwg` stores the same 24 shipped styles with **no**
//! per-property flags, one fewer property, and a visibly different
//! order. This is the list that lands all 24 records of each of
//! `arc_2004.dwg`, `circle_2004.dwg` and `line_2004.dwg` exactly on
//! their `RL` object-data-size boundary — 72 records, delta 0 — and,
//! with the two release-band adjustments described below, all 24
//! records of each of the six R14 / R2000 files and the three R2007
//! files as well. 216 further records, delta 0:
//!
//! ```text
//! TV   description                  -- inline to R2004, string stream on R2007
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
//! BS   display_unknown_short         -- R2007 only, 2 bits, always 0
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
//! ## R14 and R2000 — the same list, with §2.11's older colour form
//!
//! dwg-rs#73 opened as "a third generation exists": once #72 made the
//! R14 / R2000 / R2007 object walk work, their VISUALSTYLE records were
//! visible for the first time and neither shipped list closed on any of
//! them. The measured answer is narrower than a third generation. R14
//! and R2000 write the **same 30 fields in the same order** as R2004,
//! and the whole difference is §2.11: before R2004 a colour is the bare
//! `BS` index, without the `BL` true-colour word and `RC` colour byte
//! that R2004 added. Read the five colour slots that way and the list
//! lands **all 144 records** of `arc_R14.dwg`, `circle_R14.dwg`,
//! `line_R14.dwg`, `arc_2000.dwg`, `circle_2000.dwg` and
//! `line_2000.dwg` exactly on their boundary — delta 0, 144 of 144.
//! (On R14 that boundary is the `RL` object-data-size that release
//! writes inside the common object data rather than in the object
//! prologue; the crate-internal `objects::modern` already recovers it.)
//!
//! The two bands are bit-for-bit the same shape: the 24 records of an
//! R14 file and the 24 of the R2000 file built from the same drawing
//! give the *identical* budget list — 388, 476, 412, 500, 396, 444,
//! 428, 484, 508, 436, 420, 436, 420, 452, 452, 460, 540, 484, 412,
//! 524, 428, 436, 452, 460 bits.
//!
//! Every value cross-checks against the same style on R2004, where the
//! layout was already proven, with the two colour encodings mapping
//! one-to-one:
//!
//! - `internal_style_type` agrees on all 24 — `0` `Flat`, `1`
//!   `FlatWithEdges`, `2` `Gouraud`, `3` `GouraudWithEdges`, `4`
//!   `2dWireframe`, `7` `Basic`, `9` `Conceptual` … `27` `Shaded`;
//! - `face_opacity` is `-0.6` on twenty-three styles and `+0.5` on
//!   `X-Ray`, sign included, exactly as on R2004;
//! - `edge_crease_angle` is `40` on `Hidden`, `Shades of Gray` and
//!   `Sketchy`, `179` on `Conceptual`, `1` elsewhere;
//! - `display_brightness` is the same `BL` — `-50` on `Dim`, `50` on
//!   `Brighten`, `0` on the other twenty-two;
//! - `is_internal_use_only`, the record's final bit, splits the styles
//!   into the same ten the Visual Styles Manager lists and the same
//!   fourteen it hides;
//! - the colours agree *through* the encoding change. Where R2004
//!   writes the "none" method `0xC8000000`, R14 / R2000 write index
//!   `257`; where R2004 writes `0xC3000007` they write index `7`; and
//!   where `ColorChange` writes the grey `0xC2808080` for its face and
//!   edge colour and `Shaded` writes `0xC2787878` for its silhouette,
//!   R14 / R2000 write index `8` — the ACI dark grey — in exactly those
//!   three slots of exactly those two records. `edge_obscured_color` is
//!   `257` everywhere except `Shades of Gray` and `Sketchy`, which are
//!   `7`; R2004 is `0xC8000000` everywhere except those same two, which
//!   are `0xC3000007`.
//!
//! Because the pre-R2004 colour form *is* a `BS`, a width-only search
//! cannot tell a colour slot from a plain `BS` on this band, and the
//! `BS` / `BL` / `BD` codes coincide on the small-value forms these
//! records use. Running `examples/probe_visualstyle_layout` with
//! `--search` over the 144 records reports 64 one-token neighbours that
//! also close, every one of them such an alias or an `RC`-plus-short-form
//! rewrite of a single `BS`. The width evidence alone does not pick
//! between them; the value agreement above does, and it is the reason
//! this module reads the list it reads.
//!
//! ## R2007 — the same list plus one 2-bit slot
//!
//! R2007 keeps the R2004 colour form and adds exactly one slot. With
//! the R2004 list its records land **2 bits short** on all 72, and the
//! bits left over are `10` followed by `is_internal_use_only` — `101`
//! on the fourteen hidden styles and `100` on the ten listed ones, on
//! every record of all three R2007 files. Reading one more 2-bit slot
//! before that final `B` closes all 72, delta 0.
//!
//! The slot's position is pinned, not chosen. `Dim` is the one record
//! whose `display_brightness` is not the 2-bit zero form — it is the
//! full 34-bit `BL` holding `-50` — so it discriminates placements that
//! the other twenty-three cannot:
//!
//! - putting the slot **before** `display_brightness` closes 23 of 24
//!   records of `arc_2007.dwg` and fails on `Dim`;
//! - putting it in the head, after `internal_style_type`, fails on all
//!   24 — the colour words stop being colour words;
//! - putting it **after** `is_internal_use_only` fails on all 24;
//! - `--search` over the 72 records finds no insertion, deletion or
//!   substitution that removes the slot and still closes.
//!
//! What is *not* determined is the slot's token type or its side of
//! `display_shadow_type`. Both slots are two bits reading `0` on all 72
//! records, so `BS` / `BL` / `BD` all fit and the two are
//! interchangeable in the field list. The width is evidence; the
//! reading is a reading, so the value is surfaced as
//! [`AcadVisualStyle::display_unknown_short`] rather than given a
//! plausible-sounding name. R2010 has no counterpart in this position —
//! its head instead gained a `BS format_version` decoding `2`, which
//! this slot is not: it decodes `0` on every R2007 record measured.
//!
//! Everything else on R2007 agrees with R2004 value for value on all 24
//! styles, `ColorChange`'s `0xC2808080` face and edge colours and
//! `Shaded`'s `0xC2787878` silhouette included, and `face_specular` is
//! `+30` on exactly the seven face-shading styles and `-30` on the
//! other seventeen — the same sign rule R2004 shows.
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
//! | R14 / R2000 | 30 flag-less fields, §2.11 bare-`BS` colours — closes on all 144 records of the six `*_R14.dwg` / `*_2000.dwg` files |
//! | R2004 | the same 30 fields with `CMC` colours — closes on all 72 records of the three `*_2004.dwg` files |
//! | R2007 | the same list plus one 2-bit slot before `is_internal_use_only` — closes on all 72 records of the three `*_2007.dwg` files |
//! | R2010 | 28 `(value, flag)` properties — closes on all 24 records of `arc_2010.dwg` |
//! | R2013 / R2018 | 58 properties — closes on all 24 records of `arc_2013.dwg` and all 24 of `sample_AC1032.dwg` |
//!
//! Every corpus file holds 24 VISUALSTYLE records — R14 included, so
//! the built-in styles are not a 2007-era addition — and all 456 of the
//! 19-file corpus now decode. The 216 records of the R14 / R2000 /
//! R2007 bands were the single largest entry in the *unhandled* column
//! of `examples/coverage_report.rs` until dwg-rs#73.

use crate::bitcursor::BitCursor;
use crate::error::Result;
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
    /// A `TV` slot: its text is in the string stream, so it costs no
    /// data-stream bits, only its flag.
    Text(String),
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
    /// R2007 only: the extra 2-bit slot R2007 writes between
    /// `display_shadow_type` and `is_internal_use_only`. `0` on every
    /// corpus record, with no counterpart on any other release — see
    /// the module docs. Always `0` on the releases that have no such
    /// slot.
    pub display_unknown_short: i16,
    /// The `BS` flag of each property above, in wire order.
    pub property_flags: Vec<i16>,
    /// R2013+ tail: 30 further `(value, flag)` pairs. Their widths are
    /// measured — the record does not close on its string stream
    /// without them — but their names are not determined, so they are
    /// surfaced positionally rather than mislabelled.
    pub extended: Vec<VisualStyleProperty>,
    /// String-stream entries the field list does not place. Empty on every
    /// corpus record: the one string R2013+ records carry
    /// (`strokes_ogs.tif`) is tail slot 26 of [`extended`](Self::extended).
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

    fn property(
        &mut self,
        kind: TailKind,
        strings: Option<&mut crate::string_stream::StringReader<'_>>,
    ) -> Result<VisualStyleProperty> {
        let value = match kind {
            TailKind::Text => VisualStyleValue::Text(match strings {
                Some(s) => s.read_tv()?,
                None => String::new(),
            }),
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
    Text,
}

/// The 30 R2013+ tail property types.
///
/// First measured as the token sequence over `BS`/`BD`/`B`/`CMC` that
/// lands all 24 records of `arc_2013.dwg` and `sample_AC1032.dwg` on their
/// string-stream start. That sequence was not unique: a `BS` and a `BD`
/// of 0 are the same two bits, and the run at slots 24 to 26 read as
/// `B`, `B`, `BS` takes the same bits as the `BD`, `BS`, `TV` it is.
/// AutoCAD's DXF export settles both (the DXF value oracle, #99): it
/// writes the 58 properties as a counted list of typed groups, and slot
/// 10 is a real (group 40, `0.0`), slot 24 a real (`1.0`), slot 25 an
/// integer (`2`) and slot 26 a string, `strokes_ogs.tif`: the `TV` this
/// module used to collect as an unplaced trailing string. With these
/// types every tail value and flag equals the export on all 240 R2013 and
/// R2018 records of the nextgis and ACadSharp corpora.
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
    TailKind::Double,
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
    TailKind::Double,
    TailKind::Short,
    TailKind::Text,
    TailKind::Bool,
    TailKind::Double,
    TailKind::Double,
];

/// Decode an ACAD_VISUALSTYLE straight from its raw object payload,
/// taking its `TV` fields from the R2007+ string stream and checking
/// that the data fields end exactly on the data-stream boundary.
///
/// R14 through R2007 use the flag-less field list, R2010 and newer the
/// `(value, flag)` one. Every release this crate reads has a measured
/// layout, so no version is declined.
///
/// The flag-less list is one field order across four releases, with two
/// band adjustments the module docs derive: colours are the §2.11 bare
/// `BS` index before R2004 and the `CMC` triple from R2004 on, and
/// R2007 alone writes one further 2-bit slot before the record's final
/// `B`. It closes on all 288 flag-less records of the corpus.
pub fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadVisualStyle> {
    if version.is_r2010_plus() {
        return decode_paired(payload, body_start, inline_data_end, version);
    }
    decode_legacy(payload, body_start, inline_data_end, version)
}

/// Read one colour slot of the flag-less list.
///
/// §2.11 gives the colour two forms and the release picks one: from
/// R2004 it is the `CMC` triple `BS` index, `BL` true-colour word, `RC`
/// colour byte; before R2004 it is the bare `BS` index alone, and the
/// other two members stay `0`. See the module docs for the 144 R14 /
/// R2000 records that measure the older form and the value agreement
/// that maps one encoding onto the other.
fn read_color(c: &mut BitCursor<'_>, version: Version) -> Result<VisualStyleColor> {
    if !version.is_r2004_plus() {
        return Ok(VisualStyleColor {
            index: c.read_bs_u()?,
            rgb: 0,
            color_byte: 0,
        });
    }
    let (index, rgb, color_byte) = crate::tables::modern::read_cmc_full(c)?;
    Ok(VisualStyleColor {
        index,
        rgb,
        color_byte,
    })
}

/// The flag-less field list — 30 fields, no per-property flags, R14
/// through R2007.
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
    let face_mono_color = read_color(c, version)?;
    let face_modifier = c.read_bs()?;
    let edge_model = c.read_bs()?;
    let edge_style = c.read_bs()?;
    let edge_intersection_color = read_color(c, version)?;
    let edge_obscured_color = read_color(c, version)?;
    let edge_obscured_linetype = c.read_bs()?;
    let edge_crease_angle = c.read_bd()?;
    let edge_modifier = c.read_bs()?;
    let edge_color = read_color(c, version)?;
    let edge_opacity = c.read_bd()?;
    let edge_width = c.read_bs()?;
    let edge_overhang = c.read_bs()?;
    let edge_jitter = c.read_bs()?;
    let edge_silhouette_color = read_color(c, version)?;
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
    // R2007 alone writes one more 2-bit slot here. It is the whole
    // difference between the R2004 and R2007 records.
    let display_unknown_short = if matches!(version, Version::R2007) {
        c.read_bs()?
    } else {
        0
    };
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
        display_unknown_short,
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
            extended.push(reader.property(kind, split.strings.as_mut())?);
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
        display_unknown_short: 0,
        property_flags,
        extended,
        trailing_strings,
    })
}
