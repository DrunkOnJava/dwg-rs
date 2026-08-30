//! MULTILEADER entity (ODA Open Design Specification for .dwg files
//! v5.4 §20.4.48 "MLEADER") — multileader, R2010+ in this crate.
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

/// Read a `CMC` colour (§2.11) — `BS` index, `BL` RGB, `RC` colour byte,
/// then the optional colour / book names the colour byte flags.
fn read_cmc(c: &mut BitCursor<'_>, strings: &mut StringReader<'_>) -> Result<Cmc> {
    let index = c.read_bs()?;
    let rgb = c.read_bl_u()?;
    let color_byte = c.read_rc()?;
    let name = if color_byte & 1 != 0 {
        strings.read_tv()?
    } else {
        String::new()
    };
    let book_name = if color_byte & 2 != 0 {
        strings.read_tv()?
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

fn read_leader_line(c: &mut BitCursor<'_>, strings: &mut StringReader<'_>) -> Result<LeaderLine> {
    let num_points = c.read_bl_u()? as usize;
    bounds_check(num_points, "leader-line points", MAX_LEADER_POINTS, c.remaining_bits())?;
    let mut points = Vec::with_capacity(num_points);
    for _ in 0..num_points {
        points.push(read_bd3(c)?);
    }
    // `BL` break-info count, then that many `<BL segment index, BL pair
    // count, 3BD start, 3BD end ...>` groups.
    let num_breaks = c.read_bl_u()? as usize;
    bounds_check(num_breaks, "leader-line break info", MAX_BREAK_PAIRS, c.remaining_bits())?;
    for _ in 0..num_breaks {
        let _segment_index = c.read_bl()?;
        let pairs = c.read_bl_u()? as usize;
        bounds_check(pairs, "leader-line break pairs", MAX_BREAK_PAIRS, c.remaining_bits())?;
        for _ in 0..pairs {
            let _start = read_bd3(c)?;
            let _end = read_bd3(c)?;
        }
    }
    let index = c.read_bl()?;
    let leader_type = c.read_bs()?;
    let color = read_cmc(c, strings)?;
    let line_type_handle = NULL_HANDLE;
    let line_weight = c.read_bl()?;
    let arrow_size = c.read_bd()?;
    let arrow_symbol_handle = NULL_HANDLE;
    let override_flags = c.read_bl_u()?;
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

fn read_leader_root(c: &mut BitCursor<'_>, strings: &mut StringReader<'_>) -> Result<LeaderRoot> {
    let content_valid = c.read_b()?;
    let unknown_flag = c.read_b()?;
    let connection_point = read_bd3(c)?;
    let direction = read_bd3(c)?;
    let num_break_pairs = c.read_bl_u()? as usize;
    bounds_check(num_break_pairs, "leader-root break pairs", MAX_BREAK_PAIRS, c.remaining_bits())?;
    let mut break_points = Vec::with_capacity(num_break_pairs);
    for _ in 0..num_break_pairs {
        let start = read_bd3(c)?;
        let end = read_bd3(c)?;
        break_points.push((start, end));
    }
    let index = c.read_bl()?;
    let landing_distance = c.read_bd()?;
    let num_lines = c.read_bl_u()? as usize;
    bounds_check(num_lines, "leader lines", MAX_LEADER_LINES, c.remaining_bits())?;
    let mut lines = Vec::with_capacity(num_lines);
    for _ in 0..num_lines {
        lines.push(read_leader_line(c, strings)?);
    }
    let attachment_direction = c.read_bs()?;
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

fn read_text_content(c: &mut BitCursor<'_>, strings: &mut StringReader<'_>) -> Result<MLeaderText> {
    let label = strings.read_tv()?;
    let normal = read_bd3(c)?;
    let text_style_handle = NULL_HANDLE;
    let location = read_bd3(c)?;
    let direction = read_bd3(c)?;
    let rotation = c.read_bd()?;
    let boundary_width = c.read_bd()?;
    let boundary_height = c.read_bd()?;
    let line_spacing_factor = c.read_bd()?;
    let line_spacing_style = c.read_bs()?;
    let color = read_cmc(c, strings)?;
    let alignment = c.read_bs()?;
    let flow_direction = c.read_bs()?;
    let background_color = read_cmc(c, strings)?;
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
    bounds_check(num_sizes, "column sizes", MAX_COLUMN_SIZES, c.remaining_bits())?;
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

fn read_block_content(
    c: &mut BitCursor<'_>,
    strings: &mut StringReader<'_>,
) -> Result<MLeaderBlock> {
    let block_handle = NULL_HANDLE;
    let normal = read_bd3(c)?;
    let location = read_bd3(c)?;
    let scale = read_bd3(c)?;
    let rotation = c.read_bd()?;
    let color = read_cmc(c, strings)?;
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

fn read_context(c: &mut BitCursor<'_>, strings: &mut StringReader<'_>) -> Result<MLeaderContext> {
    let num_roots = c.read_bl_u()? as usize;
    bounds_check(num_roots, "leader roots", MAX_LEADER_ROOTS, c.remaining_bits())?;
    let mut leader_roots = Vec::with_capacity(num_roots);
    for _ in 0..num_roots {
        leader_roots.push(read_leader_root(c, strings)?);
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
        text = Some(read_text_content(c, strings)?);
    } else if c.read_b()? {
        block = Some(read_block_content(c, strings)?);
    }
    let base_point = read_bd3(c)?;
    let base_direction = read_bd3(c)?;
    let base_vertical = read_bd3(c)?;
    let normal_reversed = c.read_b()?;
    let top_attachment = c.read_bs()?;
    let bottom_attachment = c.read_bs()?;
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
    strings: &mut StringReader<'_>,
    version: Version,
) -> Result<MLeader> {
    let class_version = c.read_bs()?;
    let context = read_context(c, strings)?;
    let leader_style_handle = NULL_HANDLE;
    let override_flags = c.read_bl_u()?;
    let leader_type = c.read_bs()?;
    let leader_color = read_cmc(c, strings)?;
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
    let text_color = read_cmc(c, strings)?;
    let text_frame_enabled = c.read_b()?;
    let block_handle = NULL_HANDLE;
    let block_color = read_cmc(c, strings)?;
    let block_scale = read_bd3(c)?;
    let block_rotation = c.read_bd()?;
    let block_attachment_type = c.read_bs()?;
    let is_annotative = c.read_b()?;
    let undocumented_mc = c.read_mc()?;
    let undocumented_flag = c.read_b()?;
    let attachment_direction = c.read_bs()?;
    let top_attachment = c.read_bs()?;
    let bottom_attachment = c.read_bs()?;
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
    if !version.is_r2010_plus() {
        return Err(Error::Unsupported {
            feature: "MULTILEADER split-stream decode requires R2010+".into(),
        });
    }
    let (mut strings, string_start) = modern::open_entity(payload, version)?;
    let mut c = BitCursor::new(payload);
    string_stream::seek(&mut c, object_body_start)?;
    crate::common_entity::read_common_entity_data(&mut c, version)?;
    let mleader = read_body(&mut c, &mut strings, version)?;
    let at = c.position_bits();
    if at != string_start {
        return Err(modern::misaligned("MULTILEADER", at, string_start));
    }
    Ok(mleader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::string_stream::tests::build_payload;

    /// Append the bits of one BitWriter onto a bit vector.
    fn bits(w: BitWriter) -> Vec<bool> {
        crate::string_stream::tests::bits_of(&w)
    }

    /// Write a `CMC` with no name suffixes.
    fn write_cmc(w: &mut BitWriter, rgb: u32) {
        w.write_bs(0);
        w.write_bl(rgb as i32);
        w.write_rc(0);
    }

    /// A minimal but complete R2018 MULTILEADER body: one leader root
    /// with one two-point leader line, MTEXT content, no columns.
    fn synth_body() -> Vec<bool> {
        let mut w = BitWriter::new();
        w.write_bs(2); // class version (270)
        // -- context --
        w.write_bl(1); // one leader root
        w.write_b(true); // content valid
        w.write_b(true); // unknown
        w.write_bd(10.0); // connection point
        w.write_bd(20.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // direction
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bl(0); // no break pairs
        w.write_bl(0); // leader index
        w.write_bd(8.0); // landing distance
        w.write_bl(1); // one leader line
        w.write_bl(2); // two points
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(5.0);
        w.write_bd(5.0);
        w.write_bd(0.0);
        w.write_bl(0); // no break info
        w.write_bl(0); // leader line index
        w.write_bs(1); // leader type = straight
        write_cmc(&mut w, 0xC100_0000);
        w.write_bl(-2); // line weight
        w.write_bd(0.0); // arrow size
        w.write_bl(0); // override flags
        w.write_bs(0); // attachment direction (root)
        w.write_bd(1.0); // overall scale
        w.write_bd(1.0); // content base point
        w.write_bd(2.0);
        w.write_bd(0.0);
        w.write_bd(4.0); // text height
        w.write_bd(4.0); // arrow head size
        w.write_bd(2.0); // landing gap
        w.write_bs(1); // left attachment
        w.write_bs(1); // right attachment
        w.write_bs(0); // text align type
        w.write_bs(0); // attachment type
        w.write_b(true); // has text contents
        // text branch: the TV slot consumes no data bits
        w.write_bd(0.0); // normal
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(3.0); // location
        w.write_bd(4.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // direction
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // rotation
        w.write_bd(0.0); // boundary width
        w.write_bd(0.0); // boundary height
        w.write_bd(1.0); // line spacing factor
        w.write_bs(1); // line spacing style
        write_cmc(&mut w, 0xC000_0000); // text colour
        w.write_bs(1); // alignment
        w.write_bs(5); // flow direction
        write_cmc(&mut w, 0xC000_0000); // background colour
        w.write_bd(0.0); // background scale
        w.write_bl(0); // background transparency
        w.write_b(false); // background fill
        w.write_b(false); // background mask fill
        w.write_bs(0); // column type
        w.write_b(false); // text height automatic
        w.write_bd(0.0); // column width
        w.write_bd(0.0); // column gutter
        w.write_b(false); // column flow reversed
        w.write_bl(0); // no column sizes
        w.write_b(false); // word break
        w.write_b(false); // unknown
        w.write_bd(0.0); // base point
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // base direction
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // base vertical
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_b(false); // normal reversed
        w.write_bs(9); // top attachment
        w.write_bs(9); // bottom attachment
        // -- style block --
        w.write_bl(279552); // override flags
        w.write_bs(1); // leader type
        write_cmc(&mut w, 0xC100_0000); // leader colour
        w.write_bl(-2); // line weight
        w.write_b(true); // landing enabled
        w.write_b(true); // dogleg enabled
        w.write_bd(8.0); // landing distance
        w.write_bd(4.0); // arrow head size
        w.write_bs(2); // content type = MTEXT
        w.write_bs(1); // left attachment
        w.write_bs(1); // right attachment
        w.write_bs(1); // text angle type
        w.write_bs(0); // unknown 175
        write_cmc(&mut w, 0xC100_0000); // text colour
        w.write_b(false); // text frame enabled
        write_cmc(&mut w, 0xC100_0000); // block colour
        w.write_bd(1.0); // block scale
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(0.0); // block rotation
        w.write_bs(0); // block attachment type
        w.write_b(false); // is annotative
        w.write_bs(0); // attachment direction
        w.write_bs(9); // top attachment
        w.write_bs(9); // bottom attachment
        w.write_b(false); // leader extended to text (R2013+)
        bits(w)
    }

    /// Round-trip a BitWriter-built R2018 MULTILEADER through the
    /// split-stream decoder, including the exact-boundary check.
    #[test]
    fn roundtrip_minimal_r2018_multileader() {
        // The record body is the common entity preamble followed by the
        // MULTILEADER field list; `build_payload` frames it with the
        // string stream and its trailer.
        let mut pre = BitWriter::new();
        pre.write_bs_u(0); // no XDATA
        pre.write_b(false); // no graphics preview
        pre.write_bb(0b10); // entmode
        pre.write_bl(0); // num_reactors
        pre.write_b(true); // no xdictionary
        pre.write_b(false); // no AcDs binary data
        pre.write_bs_u(0x0100); // colour
        pre.write_bd(1.0); // linetype scale
        pre.write_bb(0b00); // ltype flags
        pre.write_bb(0b00); // plotstyle
        pre.write_bb(0b00); // material
        pre.write_rc(0); // shadow
        pre.write_b(false);
        pre.write_b(false);
        pre.write_b(false);
        pre.write_bs(0); // invisibility
        pre.write_rc(0x1D); // lineweight
        let mut body = bits(pre);
        body.extend(synth_body());

        let payload = build_payload(&body, &["MULTILEADER TEST"]);
        let m = decode_modern_split_stream(&payload, 8, Version::R2018).expect("decodes");
        assert_eq!(m.class_version, 2);
        assert_eq!(m.context.leader_roots.len(), 1);
        let root = &m.context.leader_roots[0];
        assert_eq!(root.landing_distance, 8.0);
        assert_eq!(root.lines.len(), 1);
        assert_eq!(root.lines[0].points.len(), 2);
        assert_eq!(
            root.lines[0].points[1],
            Point3D {
                x: 5.0,
                y: 5.0,
                z: 0.0
            }
        );
        assert_eq!(root.lines[0].line_weight, -2);
        let text = m.context.text.as_ref().expect("text content");
        assert_eq!(text.label, "MULTILEADER TEST");
        assert_eq!(text.flow_direction, 5);
        assert!(m.context.block.is_none());
        assert_eq!(m.context.top_attachment, 9);
        assert_eq!(m.override_flags, 279552);
        assert_eq!(m.content_type, 2);
        assert_eq!(
            m.block_scale,
            Point3D {
                x: 1.0,
                y: 1.0,
                z: 1.0
            }
        );
        assert!(!m.is_annotative);
        assert_eq!(m.bottom_attachment, 9);
        assert!(!m.leader_extended_to_text);
    }

    /// A wrong field list must error on the boundary, not return a
    /// plausible struct: dropping the R2013+ `B 295` leaves the data
    /// fields one bit short of the string stream.
    #[test]
    fn misaligned_field_list_errors() {
        let mut pre = BitWriter::new();
        pre.write_bs_u(0);
        pre.write_b(false);
        pre.write_bb(0b10);
        pre.write_bl(0);
        pre.write_b(true);
        pre.write_b(false);
        pre.write_bs_u(0x0100);
        pre.write_bd(1.0);
        pre.write_bb(0b00);
        pre.write_bb(0b00);
        pre.write_bb(0b00);
        pre.write_rc(0);
        pre.write_b(false);
        pre.write_b(false);
        pre.write_b(false);
        pre.write_bs(0);
        pre.write_rc(0x1D);
        let mut body = bits(pre);
        let mut synth = synth_body();
        synth.pop(); // drop the R2013+ trailing bit
        body.extend(synth);

        let payload = build_payload(&body, &["MULTILEADER TEST"]);
        let err = decode_modern_split_stream(&payload, 8, Version::R2018)
            .expect_err("a short field list must be rejected");
        assert!(
            matches!(&err, Error::SectionMap(m) if m.contains("MULTILEADER data fields ended")),
            "err={err:?}"
        );
    }

    #[test]
    fn pre_r2010_is_unsupported() {
        let payload = build_payload(&[false; 8], &[]);
        let err = decode_modern_split_stream(&payload, 8, Version::R2007)
            .expect_err("pre-R2010 must error");
        assert!(matches!(&err, Error::Unsupported { feature } if feature.contains("R2010")));
    }
}
