//! MULTILEADER entity (ODA Open Design Specification for .dwg files
//! v5.4 §20.4.48 "MLEADER") — multileader.
//!
//! # Every release (2026-10-26, #229)
//!
//! §20.4.48 says MULTILEADER "was introduced in version 21" (R2007),
//! but AutoCAD 2025 writes the record into R14, R2000 and R2004 saves
//! too, as a class. The R2010+ field list below has three kinds of
//! version branch, each read as §20.4.48 and §20.4.86 print them: the
//! R2010+ `BS 270` class version, the R2010+ style fields of each leader
//! line, leader root and context (`BS 170`, `CMC 92`, `BL 171`, `BD 40`,
//! `BL 93`; `BS 271`; `BS 273`, `BS 272`), and the `-R2007` tail after
//! the annotative flag ([`MLeaderPreR2010`]). Before R2004 a `CMC` is a
//! bare `BS` index (§2.11), and before R2007 the `TV`s are inline. With
//! that, all 105 MULTILEADER records of the public corpus close, R14 to
//! R2018, and on R2000 to R2018 every value compared equals AutoCAD's DXF
//! export of the same drawing (ARCHITECTURE.md §7ai,
//! `examples/probe_mleader.rs`).
//!
//! # Where the fields actually live
//!
//! Most of a multileader is not in the MLEADER record's own field list.
//! §20.4.48 says so directly: "A significant portion (content
//! block/text and leaders) of the multileader entity is stored in the
//! MLeaderAnnotContext object (see paragraph 20.4.86), which is
//! embedded into this object (stream)." So the record reads
//!
//! ```text
//! BS  270   version (expected 2)
//! ...       the whole MLeaderAnnotContext field list, inline
//! H   340   leader style
//! BL  90    override flags
//! ...       the style block
//! ```
//!
//! and the context block carries the leader roots, the leader lines,
//! their vertices, and the text or block content.
//!
//! # Measured: what the embedded context does *not* carry
//!
//! §20.4.86 lists the context as inheriting `AcDbAnnotScaleObjectContextData`
//! (§20.4.71) which inherits `AcDbObjectContextData` (§20.4.89) — a
//! `BS` version, two `B` flags and an `H` to the scale object. Embedded
//! in an MLEADER **none of those inherited fields are present**: the
//! `BL` leader-root count follows the `BS 270` version directly. On
//! `sample_AC1032.dwg` handle `0x7B8` the version reads `2` at bit 6885
//! and the next 10 bits are a `BL` of `1` — one leader root — after
//! which the two `B` flags, a full-double connection point and a
//! `(1, 0, 0)` direction all land. Reading the inherited prefix first
//! puts the connection point 12 bits late and it decodes as a reserved
//! `BD` pattern.
//!
//! # Measured: the R2007-and-earlier block is genuinely absent
//!
//! §20.4.48 tags the arrowhead list, the block-label list and the four
//! fields after them (`B 294`, `BS 178`, `BS 179`, `BD 45`) as
//! `-R2007`. All 15 MULTILEADER records of `sample_AC1032.dwg` close
//! exactly on their string-stream start bit with that block omitted and
//! with the R2010+ `BS 271 / BS 273 / BS 272` plus the R2013+ `B 295`
//! read after `B 293 is annotative`; the last 21 bits of every one of
//! them are bit-identical (`01 00001001`, `01 00001001`, `0` — top and
//! bottom attachment both `9`, leader-not-extended). In R2010+ the
//! per-arrowhead data moved into the leader line (`H 341 arrow symbol`
//! per line), which is why nothing is lost.
//!
//! # Measured: 17 undocumented bits before the R2010+ trailer
//!
//! Between `B 293 Is annotative` and the R2010+ `BS 271 Attachment
//! direction`, every MULTILEADER record of `sample_AC1032.dwg` carries
//! 17 bits that §20.4.48 does not list: one `MC` (two bytes on all 15
//! records — a continuation byte then a terminating byte) followed by
//! one `B`. Read that way all 15 records land exactly on their
//! string-stream start bit; with the 17 bits omitted every one of them
//! stops 17 bits short. The `MC` holds only three values across the
//! file — `274` (six records), `530` (one) and `786` (eight), i.e.
//! `18 + 128 · {2, 4, 6}` — and the `B` is `true` on all fifteen.
//!
//! What the two fields *mean* is not established, and neither is which
//! of the three `B`s in this run is `B 293`: `B, MC, B` and
//! `B, MC, B` read identically whichever end `is annotative` sits at.
//! This decoder keeps §20.4.48's order — `B 293` first — and surfaces
//! the other two as [`MLeader::undocumented_mc`] /
//! [`MLeader::undocumented_flag`] rather than inventing names for them.
//! A file whose multileaders are known to be annotative would settle
//! the assignment; this corpus cannot.
//!
//! The `MC` reading is not the only 17-bit shape that fits: `B` + `RS`,
//! or `B` + two `RC`s, consume the same bits on every record here,
//! because each record's byte pair happens to have the continuation bit
//! set on the first byte and clear on the second. `MC` is taken because
//! it is the only one of the three that stays correct if a record ever
//! carries a larger value.
//!
//! # Handles and strings
//!
//! Every `H` in the field list is an object reference, and from R2007
//! object references live in the record's handle stream, not its data
//! stream — so an `H` slot consumes **zero** data-stream bits here.
//! Likewise the one `TV` (the context's text label) comes from the
//! string stream. That is what makes the decode self-validating: the
//! data fields must end exactly on the string-stream start bit.

use crate::bitcursor::{BitCursor, Handle};
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};
use crate::string_stream::{self, StringReader};
use crate::tables::modern;
use crate::version::Version;

/// Maximum leader roots accepted in one MULTILEADER. A multileader
/// attaches at most one root per attachment side in AutoCAD; the cap is
/// well above that so a malformed count is rejected without allocating.
pub const MAX_LEADER_ROOTS: usize = 256;

/// Maximum leader lines per root.
pub const MAX_LEADER_LINES: usize = 1_024;

/// Maximum vertices per leader line.
pub const MAX_LEADER_POINTS: usize = 10_000;

/// Maximum break start/end point pairs in any one list.
pub const MAX_BREAK_PAIRS: usize = 1_024;

/// Maximum text-column sizes in the context's text content.
pub const MAX_COLUMN_SIZES: usize = 256;

/// A `CMC` colour (§2.11, R2004+): a colour index that is always zero,
/// a 32-bit RGB word, and a colour byte whose low two bits flag an
/// optional colour name and colour-book name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cmc {
    /// `BS` colour index. Per §2.11 this is always 0 in the CMC form.
    pub index: i16,
    /// `BL` RGB word.
    pub rgb: u32,
    /// `RC` colour byte (`& 1` ⇒ colour name follows, `& 2` ⇒ book name).
    pub color_byte: u8,
    /// Colour name, when the colour byte requested one.
    pub name: String,
    /// Colour-book name, when the colour byte requested one.
    pub book_name: String,
}

/// One vertex list plus per-line style of a leader (§20.4.86
/// "LEADER_LINE").
#[derive(Debug, Clone, PartialEq)]
pub struct LeaderLine {
    /// Vertices of this leader line, in order.
    pub points: Vec<Point3D>,
    /// `BL 91` index of the line within its root.
    pub index: i32,
    /// `BS 170` leader type (0 invisible, 1 straight, 2 spline).
    pub leader_type: i16,
    /// `CMC 92` line colour.
    pub color: Cmc,
    /// `H 340` line type — null on R2007+, where it lives in the
    /// handle stream.
    pub line_type_handle: Handle,
    /// `BL 171` line weight.
    pub line_weight: i32,
    /// `BD 40` arrow size.
    pub arrow_size: f64,
    /// `H 341` arrow symbol — null on R2007+.
    pub arrow_symbol_handle: Handle,
    /// `BL 93` per-line override flags.
    pub override_flags: u32,
}

/// One leader root (§20.4.86 "LEADER") — a connection point on the
/// content and the leader lines hanging off it.
#[derive(Debug, Clone, PartialEq)]
pub struct LeaderRoot {
    /// `B 290` — ODA writes true.
    pub content_valid: bool,
    /// `B 291` — undocumented; ODA writes true.
    pub unknown_flag: bool,
    /// `3BD 10` connection point.
    pub connection_point: Point3D,
    /// `3BD 11` direction.
    pub direction: Vec3D,
    /// `3BD 12` / `3BD 13` break start/end point pairs.
    pub break_points: Vec<(Point3D, Point3D)>,
    /// `BL 90` leader index.
    pub index: i32,
    /// `BD 40` landing distance.
    pub landing_distance: f64,
    /// The leader lines attached to this root.
    pub lines: Vec<LeaderLine>,
    /// `BS 271` attachment direction (0 horizontal, 1 vertical).
    pub attachment_direction: i16,
}

/// Text content of a multileader (§20.4.86, `Has text contents` branch).
#[derive(Debug, Clone, PartialEq)]
pub struct MLeaderText {
    /// `TV 304` — the label, read from the record's string stream.
    pub label: String,
    /// `3BD 11` normal vector.
    pub normal: Vec3D,
    /// `H 340` text style — null on R2007+.
    pub text_style_handle: Handle,
    /// `3BD 12` location.
    pub location: Point3D,
    /// `3BD 13` direction.
    pub direction: Vec3D,
    /// `BD 42` rotation, radians.
    pub rotation: f64,
    /// `BD 43` boundary width.
    pub boundary_width: f64,
    /// `BD 44` boundary height.
    pub boundary_height: f64,
    /// `BD 45` line-spacing factor.
    pub line_spacing_factor: f64,
    /// `BS 170` line-spacing style (1 at least, 2 exactly).
    pub line_spacing_style: i16,
    /// `CMC 90` text colour.
    pub color: Cmc,
    /// `BS 171` alignment (1 left, 2 center, 3 right).
    pub alignment: i16,
    /// `BS 172` flow direction (1 horizontal, 3 vertical, 6 by style).
    pub flow_direction: i16,
    /// `CMC 91` background fill colour.
    pub background_color: Cmc,
    /// `BD 141` background scale factor.
    pub background_scale: f64,
    /// `BL 92` background transparency.
    pub background_transparency: i32,
    /// `B 291` background fill enabled.
    pub background_fill_enabled: bool,
    /// `B 292` background mask fill on.
    pub background_mask_fill_on: bool,
    /// `BS 173` column type.
    pub column_type: i16,
    /// `B 293` text height automatic.
    pub text_height_automatic: bool,
    /// `BD 142` column width.
    pub column_width: f64,
    /// `BD 143` column gutter.
    pub column_gutter: f64,
    /// `B 294` column flow reversed.
    pub column_flow_reversed: bool,
    /// `BD 144` column sizes.
    pub column_sizes: Vec<f64>,
    /// `B 295` word break.
    pub word_break: bool,
    /// Undocumented trailing `B` of the text branch.
    pub unknown_flag: bool,
}

/// Block content of a multileader (§20.4.86, `Has contents block` branch).
#[derive(Debug, Clone, PartialEq)]
pub struct MLeaderBlock {
    /// `H 341` block table record — null on R2007+.
    pub block_handle: Handle,
    /// `3BD 14` normal vector.
    pub normal: Vec3D,
    /// `3BD 15` location.
    pub location: Point3D,
    /// `3BD 16` scale vector.
    pub scale: Point3D,
    /// `BD 46` rotation, radians.
    pub rotation: f64,
    /// `CMC 93` block colour.
    pub color: Cmc,
    /// `BD 47` — the 16 doubles of the complete transformation matrix.
    pub transform: [f64; 16],
}

/// The embedded `MLeaderAnnotContext` (§20.4.86).
#[derive(Debug, Clone, PartialEq)]
pub struct MLeaderContext {
    /// The leader roots, each with its own leader lines.
    pub leader_roots: Vec<LeaderRoot>,
    /// `BD 40` overall scale.
    pub overall_scale: f64,
    /// `3BD 10` content base point.
    pub content_base_point: Point3D,
    /// `BD 41` text height.
    pub text_height: f64,
    /// `BD 140` arrow head size.
    pub arrow_head_size: f64,
    /// `BD 145` landing gap.
    pub landing_gap: f64,
    /// `BS 174` left text attachment type.
    pub left_attachment: i16,
    /// `BS 175` right text attachment type.
    pub right_attachment: i16,
    /// `BS 176` text align type (0 left, 1 center, 2 right).
    pub text_align_type: i16,
    /// `BS 177` attachment type (0 content extents, 1 insertion point).
    pub attachment_type: i16,
    /// Text content, when `B 290 has text contents` was set.
    pub text: Option<MLeaderText>,
    /// Block content, when `B 296 has contents block` was set.
    pub block: Option<MLeaderBlock>,
    /// `3BD 110` base point.
    pub base_point: Point3D,
    /// `3BD 111` base direction.
    pub base_direction: Vec3D,
    /// `3BD 112` base vertical.
    pub base_vertical: Vec3D,
    /// `B 297` is normal reversed.
    pub normal_reversed: bool,
    /// `BS 273` top attachment.
    pub top_attachment: i16,
    /// `BS 272` bottom attachment.
    pub bottom_attachment: i16,
}

/// Decoded MULTILEADER (§20.4.48).
#[derive(Debug, Clone, PartialEq)]
pub struct MLeader {
    /// `BS 270` version — expected to be 2.
    pub class_version: i16,
    /// The embedded `MLeaderAnnotContext` block.
    pub context: MLeaderContext,
    /// `H 340` leader style — null on R2007+.
    pub leader_style_handle: Handle,
    /// `BL 90` property-override bitset.
    pub override_flags: u32,
    /// `BS 170` leader type (0 invisible, 1 straight, 2 spline).
    pub leader_type: i16,
    /// `CMC 91` leader colour.
    pub leader_color: Cmc,
    /// `H 341` leader line type — null on R2007+.
    pub line_type_handle: Handle,
    /// `BL 171` line weight.
    pub line_weight: i32,
    /// `B 290` landing enabled.
    pub landing_enabled: bool,
    /// `B 291` dog-leg enabled.
    pub dogleg_enabled: bool,
    /// `BD 41` landing distance.
    pub landing_distance: f64,
    /// `H 342` arrow head — null on R2007+.
    pub arrow_head_handle: Handle,
    /// `BD 42` default arrow-head size.
    pub arrow_head_size: f64,
    /// `BS 172` style content type (0 none, 1 block, 2 mtext, 3 tolerance).
    pub content_type: i16,
    /// `H 343` style text style — null on R2007+.
    pub text_style_handle: Handle,
    /// `BS 173` style left text attachment type.
    pub left_attachment: i16,
    /// `BS 95` style right text attachment type.
    pub right_attachment: i16,
    /// `BS 174` style text angle type.
    pub text_angle_type: i16,
    /// `BS 175` — undocumented in §20.4.48 ("Unknown").
    pub unknown_175: i16,
    /// `CMC 92` style text colour.
    pub text_color: Cmc,
    /// `B 292` style text frame enabled.
    pub text_frame_enabled: bool,
    /// `H 344` style block — null on R2007+.
    pub block_handle: Handle,
    /// `CMC 93` style block colour.
    pub block_color: Cmc,
    /// `3BD 10` style block scale vector.
    pub block_scale: Point3D,
    /// `BD 43` style block rotation, radians.
    pub block_rotation: f64,
    /// `BS 176` style attachment type (0 center extents, 1 insertion point).
    pub block_attachment_type: i16,
    /// `B 293` is annotative.
    pub is_annotative: bool,
    /// An `MC` that §20.4.48 does not list, between `B 293` and the
    /// R2010+ trailer. See the module docs for the measurement — the
    /// 15 records of `sample_AC1032.dwg` hold only `274`, `530` and
    /// `786`.
    pub undocumented_mc: i64,
    /// The `B` that follows [`MLeader::undocumented_mc`] — also absent
    /// from §20.4.48, and `true` on every record measured.
    pub undocumented_flag: bool,
    /// `BS 271` attachment direction (0 horizontal, 1 vertical). R2010+.
    pub attachment_direction: i16,
    /// `BS 273` style top text attachment. R2010+.
    pub top_attachment: i16,
    /// `BS 272` style bottom text attachment. R2010+.
    pub bottom_attachment: i16,
    /// `B 295` leader extended to text. R2013+.
    pub leader_extended_to_text: bool,
    /// The fields §20.4.48 writes only up to R2007, after the annotative
    /// flag; `None` from R2010.
    pub legacy: Option<MLeaderPreR2010>,
}

/// The `-R2007` tail of a MULTILEADER record (§20.4.48): arrow heads,
/// block labels and four scalar fields.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MLeaderPreR2010 {
    /// One `B` "is default" per arrow head (the handle is in the handle
    /// stream).
    pub arrow_heads_default: Vec<bool>,
    /// One `(TV label text, BS UI index, BD width)` per block label (the
    /// ATTDEF handle is in the handle stream).
    pub block_labels: Vec<(String, i16, f64)>,
    /// `B 294` text direction negative.
    pub text_direction_negative: bool,
    /// `BS 178` IPE align.
    pub ipe_align: i16,
    /// `BS 179` justification.
    pub justification: i16,
    /// `BD 45` scale factor.
    pub scale_factor: f64,
}

fn read_pre_r2010_tail(c: &mut BitCursor<'_>, s: &mut Src<'_>) -> Result<MLeaderPreR2010> {
    let num_arrows = c.read_bl_u()? as usize;
    bounds_check(
        num_arrows,
        "arrow heads",
        MAX_LEADER_LINES,
        c.remaining_bits(),
    )?;
    let mut arrow_heads_default = Vec::with_capacity(num_arrows);
    for _ in 0..num_arrows {
        arrow_heads_default.push(c.read_b()?);
    }
    let num_labels = c.read_bl_u()? as usize;
    bounds_check(
        num_labels,
        "block labels",
        MAX_LEADER_LINES,
        c.remaining_bits(),
    )?;
    let mut block_labels = Vec::with_capacity(num_labels);
    for _ in 0..num_labels {
        let text = s.tv(c)?;
        let ui_index = c.read_bs()?;
        let width = c.read_bd()?;
        block_labels.push((text, ui_index, width));
    }
    let text_direction_negative = c.read_b()?;
    let ipe_align = c.read_bs()?;
    let justification = c.read_bs()?;
    let scale_factor = c.read_bd()?;
    Ok(MLeaderPreR2010 {
        arrow_heads_default,
        block_labels,
        text_direction_negative,
        ipe_align,
        justification,
        scale_factor,
    })
}

/// A null handle — what an `H` slot yields on R2007+, where object
/// references live in the record's handle stream.
const NULL_HANDLE: Handle = Handle {
    code: 0,
    counter: 0,
    value: 0,
};

fn bounds_check(n: usize, field: &'static str, cap: usize, remaining_bits: usize) -> Result<()> {
    if n > cap || n > remaining_bits {
        Err(Error::SectionMap(format!(
            "MLEADER {field} count {n} exceeds cap ({cap}) or remaining_bits ({remaining_bits})"
        )))
    } else {
        Ok(())
    }
}

/// Where a MULTILEADER's `TV` fields live, and which release's branches
/// to read: the R2007+ string stream, or inline before R2007.
pub(crate) struct Src<'a> {
    strings: Option<StringReader<'a>>,
    version: Version,
}

impl<'a> Src<'a> {
    fn tv(&mut self, c: &mut BitCursor<'_>) -> Result<String> {
        modern::read_tv_field(c, self.version, self.strings.as_mut())
    }
}

/// Read a `CMC` colour (§2.11) — `BS` index, `BL` RGB, `RC` colour byte,
/// then the optional colour / book names the colour byte flags.
fn read_cmc(c: &mut BitCursor<'_>, s: &mut Src<'_>) -> Result<Cmc> {
    // §2.11: before R2004 a CMC is the `BS` colour index alone.
    if !s.version.is_r2004_plus() {
        return Ok(Cmc {
            index: c.read_bs()?,
            rgb: 0,
            color_byte: 0,
            name: String::new(),
            book_name: String::new(),
        });
    }
    let index = c.read_bs()?;
    let rgb = c.read_bl_u()?;
    let color_byte = c.read_rc()?;
    let name = if color_byte & 1 != 0 {
        s.tv(c)?
    } else {
        String::new()
    };
    let book_name = if color_byte & 2 != 0 {
        s.tv(c)?
    } else {
        String::new()
    };
    Ok(Cmc {
        index,
        rgb,
        color_byte,
        name,
        book_name,
    })
}

fn read_leader_line(c: &mut BitCursor<'_>, s: &mut Src<'_>) -> Result<LeaderLine> {
    let num_points = c.read_bl_u()? as usize;
    bounds_check(
        num_points,
        "leader-line points",
        MAX_LEADER_POINTS,
        c.remaining_bits(),
    )?;
    let mut points = Vec::with_capacity(num_points);
    for _ in 0..num_points {
        points.push(read_bd3(c)?);
    }
    // `BL` break-info count, then that many `<BL segment index, BL pair
    // count, 3BD start, 3BD end ...>` groups.
    let num_breaks = c.read_bl_u()? as usize;
    bounds_check(
        num_breaks,
        "leader-line break info",
        MAX_BREAK_PAIRS,
        c.remaining_bits(),
    )?;
    for _ in 0..num_breaks {
        let _segment_index = c.read_bl()?;
        let pairs = c.read_bl_u()? as usize;
        bounds_check(
            pairs,
            "leader-line break pairs",
            MAX_BREAK_PAIRS,
            c.remaining_bits(),
        )?;
        for _ in 0..pairs {
            let _start = read_bd3(c)?;
            let _end = read_bd3(c)?;
        }
    }
    let index = c.read_bl()?;
    let line_type_handle = NULL_HANDLE;
    let arrow_symbol_handle = NULL_HANDLE;
    // §20.4.86: a leader line's own style fields are R2010+.
    let (leader_type, color, line_weight, arrow_size, override_flags) = if s.version.is_r2010_plus()
    {
        let leader_type = c.read_bs()?;
        let color = read_cmc(c, s)?;
        let line_weight = c.read_bl()?;
        let arrow_size = c.read_bd()?;
        let override_flags = c.read_bl_u()?;
        (leader_type, color, line_weight, arrow_size, override_flags)
    } else {
        (0, Cmc::default(), 0, 0.0, 0)
    };
    Ok(LeaderLine {
        points,
        index,
        leader_type,
        color,
        line_type_handle,
        line_weight,
        arrow_size,
        arrow_symbol_handle,
        override_flags,
    })
}

fn read_leader_root(c: &mut BitCursor<'_>, s: &mut Src<'_>) -> Result<LeaderRoot> {
    let content_valid = c.read_b()?;
    let unknown_flag = c.read_b()?;
    let connection_point = read_bd3(c)?;
    let direction = read_bd3(c)?;
    let num_break_pairs = c.read_bl_u()? as usize;
    bounds_check(
        num_break_pairs,
        "leader-root break pairs",
        MAX_BREAK_PAIRS,
        c.remaining_bits(),
    )?;
    let mut break_points = Vec::with_capacity(num_break_pairs);
    for _ in 0..num_break_pairs {
        let start = read_bd3(c)?;
        let end = read_bd3(c)?;
        break_points.push((start, end));
    }
    let index = c.read_bl()?;
    let landing_distance = c.read_bd()?;
    let num_lines = c.read_bl_u()? as usize;
    bounds_check(
        num_lines,
        "leader lines",
        MAX_LEADER_LINES,
        c.remaining_bits(),
    )?;
    let mut lines = Vec::with_capacity(num_lines);
    for _ in 0..num_lines {
        lines.push(read_leader_line(c, s)?);
    }
    // §20.4.86: R2010+.
    let attachment_direction = if s.version.is_r2010_plus() {
        c.read_bs()?
    } else {
        0
    };
    Ok(LeaderRoot {
        content_valid,
        unknown_flag,
        connection_point,
        direction,
        break_points,
        index,
        landing_distance,
        lines,
        attachment_direction,
    })
}

fn read_text_content(c: &mut BitCursor<'_>, s: &mut Src<'_>) -> Result<MLeaderText> {
    let label = s.tv(c)?;
    let normal = read_bd3(c)?;
    let text_style_handle = NULL_HANDLE;
    let location = read_bd3(c)?;
    let direction = read_bd3(c)?;
    let rotation = c.read_bd()?;
    let boundary_width = c.read_bd()?;
    let boundary_height = c.read_bd()?;
    let line_spacing_factor = c.read_bd()?;
    let line_spacing_style = c.read_bs()?;
    let color = read_cmc(c, s)?;
    let alignment = c.read_bs()?;
    let flow_direction = c.read_bs()?;
    let background_color = read_cmc(c, s)?;
    let background_scale = c.read_bd()?;
    let background_transparency = c.read_bl()?;
    let background_fill_enabled = c.read_b()?;
    let background_mask_fill_on = c.read_b()?;
    let column_type = c.read_bs()?;
    let text_height_automatic = c.read_b()?;
    let column_width = c.read_bd()?;
    let column_gutter = c.read_bd()?;
    let column_flow_reversed = c.read_b()?;
    let num_sizes = c.read_bl_u()? as usize;
    bounds_check(
        num_sizes,
        "column sizes",
        MAX_COLUMN_SIZES,
        c.remaining_bits(),
    )?;
    let mut column_sizes = Vec::with_capacity(num_sizes);
    for _ in 0..num_sizes {
        column_sizes.push(c.read_bd()?);
    }
    let word_break = c.read_b()?;
    let unknown_flag = c.read_b()?;
    Ok(MLeaderText {
        label,
        normal,
        text_style_handle,
        location,
        direction,
        rotation,
        boundary_width,
        boundary_height,
        line_spacing_factor,
        line_spacing_style,
        color,
        alignment,
        flow_direction,
        background_color,
        background_scale,
        background_transparency,
        background_fill_enabled,
        background_mask_fill_on,
        column_type,
        text_height_automatic,
        column_width,
        column_gutter,
        column_flow_reversed,
        column_sizes,
        word_break,
        unknown_flag,
    })
}

fn read_block_content(c: &mut BitCursor<'_>, s: &mut Src<'_>) -> Result<MLeaderBlock> {
    let block_handle = NULL_HANDLE;
    let normal = read_bd3(c)?;
    let location = read_bd3(c)?;
    let scale = read_bd3(c)?;
    let rotation = c.read_bd()?;
    let color = read_cmc(c, s)?;
    let mut transform = [0.0f64; 16];
    for slot in transform.iter_mut() {
        *slot = c.read_bd()?;
    }
    Ok(MLeaderBlock {
        block_handle,
        normal,
        location,
        scale,
        rotation,
        color,
        transform,
    })
}

fn read_context(c: &mut BitCursor<'_>, s: &mut Src<'_>) -> Result<MLeaderContext> {
    let num_roots = c.read_bl_u()? as usize;
    bounds_check(
        num_roots,
        "leader roots",
        MAX_LEADER_ROOTS,
        c.remaining_bits(),
    )?;
    let mut leader_roots = Vec::with_capacity(num_roots);
    for _ in 0..num_roots {
        leader_roots.push(read_leader_root(c, s)?);
    }
    let overall_scale = c.read_bd()?;
    let content_base_point = read_bd3(c)?;
    let text_height = c.read_bd()?;
    let arrow_head_size = c.read_bd()?;
    let landing_gap = c.read_bd()?;
    let left_attachment = c.read_bs()?;
    let right_attachment = c.read_bs()?;
    let text_align_type = c.read_bs()?;
    let attachment_type = c.read_bs()?;
    let has_text = c.read_b()?;
    let mut text = None;
    let mut block = None;
    if has_text {
        text = Some(read_text_content(c, s)?);
    } else if c.read_b()? {
        block = Some(read_block_content(c, s)?);
    }
    let base_point = read_bd3(c)?;
    let base_direction = read_bd3(c)?;
    let base_vertical = read_bd3(c)?;
    let normal_reversed = c.read_b()?;
    // §20.4.86: R2010+.
    let (top_attachment, bottom_attachment) = if s.version.is_r2010_plus() {
        (c.read_bs()?, c.read_bs()?)
    } else {
        (0, 0)
    };
    Ok(MLeaderContext {
        leader_roots,
        overall_scale,
        content_base_point,
        text_height,
        arrow_head_size,
        landing_gap,
        left_attachment,
        right_attachment,
        text_align_type,
        attachment_type,
        text,
        block,
        base_point,
        base_direction,
        base_vertical,
        normal_reversed,
        top_attachment,
        bottom_attachment,
    })
}

/// Read the MULTILEADER body (§20.4.48) from a data cursor already
/// positioned past the common entity preamble, taking `TV` fields from
/// `strings`.
pub(crate) fn read_body(
    c: &mut BitCursor<'_>,
    s: &mut Src<'_>,
    version: Version,
) -> Result<MLeader> {
    // §20.4.48: the class version is R2010+.
    let class_version = if version.is_r2010_plus() {
        c.read_bs()?
    } else {
        0
    };
    let context = read_context(c, s)?;
    let leader_style_handle = NULL_HANDLE;
    let override_flags = c.read_bl_u()?;
    let leader_type = c.read_bs()?;
    let leader_color = read_cmc(c, s)?;
    let line_type_handle = NULL_HANDLE;
    let line_weight = c.read_bl()?;
    let landing_enabled = c.read_b()?;
    let dogleg_enabled = c.read_b()?;
    let landing_distance = c.read_bd()?;
    let arrow_head_handle = NULL_HANDLE;
    let arrow_head_size = c.read_bd()?;
    let content_type = c.read_bs()?;
    let text_style_handle = NULL_HANDLE;
    let left_attachment = c.read_bs()?;
    let right_attachment = c.read_bs()?;
    let text_angle_type = c.read_bs()?;
    let unknown_175 = c.read_bs()?;
    let text_color = read_cmc(c, s)?;
    let text_frame_enabled = c.read_b()?;
    let block_handle = NULL_HANDLE;
    let block_color = read_cmc(c, s)?;
    let block_scale = read_bd3(c)?;
    let block_rotation = c.read_bd()?;
    let block_attachment_type = c.read_bs()?;
    let is_annotative = c.read_b()?;
    let mut legacy = None;
    let (
        undocumented_mc,
        undocumented_flag,
        attachment_direction,
        top_attachment,
        bottom_attachment,
    );
    if version.is_r2010_plus() {
        undocumented_mc = c.read_mc()?;
        undocumented_flag = c.read_b()?;
        attachment_direction = c.read_bs()?;
        top_attachment = c.read_bs()?;
        bottom_attachment = c.read_bs()?;
    } else {
        legacy = Some(read_pre_r2010_tail(c, s)?);
        undocumented_mc = 0;
        undocumented_flag = false;
        attachment_direction = 0;
        top_attachment = 0;
        bottom_attachment = 0;
    }
    let leader_extended_to_text = if matches!(version, Version::R2013 | Version::R2018) {
        c.read_b()?
    } else {
        false
    };
    Ok(MLeader {
        class_version,
        context,
        leader_style_handle,
        override_flags,
        leader_type,
        leader_color,
        line_type_handle,
        line_weight,
        landing_enabled,
        dogleg_enabled,
        landing_distance,
        arrow_head_handle,
        arrow_head_size,
        content_type,
        text_style_handle,
        left_attachment,
        right_attachment,
        text_angle_type,
        unknown_175,
        text_color,
        text_frame_enabled,
        block_handle,
        block_color,
        block_scale,
        block_rotation,
        block_attachment_type,
        is_annotative,
        undocumented_mc,
        undocumented_flag,
        attachment_direction,
        top_attachment,
        bottom_attachment,
        leader_extended_to_text,
        legacy,
    })
}

/// Decode an R2010+ MULTILEADER from its record payload (§20.4.48).
///
/// `object_body_start` is the bit just past the object header, i.e.
/// where the common entity preamble begins. The decoder reads the
/// preamble itself, then the body, then checks that the data fields
/// ended exactly on the record's string-stream start bit — so a wrong
/// field list errors instead of returning a plausible-looking struct.
pub fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<MLeader> {
    if !version.is_r2007_plus() {
        return Err(Error::Unsupported {
            feature: "MULTILEADER split-stream decode requires R2007+".into(),
        });
    }
    let (strings, string_start) = modern::open_entity(payload, version)?;
    let mut s = Src {
        strings: Some(strings),
        version,
    };
    let mut c = BitCursor::new(payload);
    string_stream::seek(&mut c, object_body_start)?;
    crate::common_entity::read_common_entity_data(&mut c, version)?;
    let mleader = read_body(&mut c, &mut s, version)?;
    let at = c.position_bits();
    if at != string_start {
        return Err(modern::misaligned("MULTILEADER", at, string_start));
    }
    Ok(mleader)
}

/// Decode a MULTILEADER from a cursor past its common entity data, with
/// its `TV` fields inline (R13 to R2004).
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<MLeader> {
    let mut s = Src {
        strings: None,
        version,
    };
    read_body(c, &mut s, version)
}
