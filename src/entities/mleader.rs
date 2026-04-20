//! MLEADER entity (§19.4.85) — multileader (R2010+).
//!
//! MLEADER is the modern successor to LEADER, added in R2008 and
//! substantially revised in R2010. It encodes a leader line (possibly
//! spline-based), an optional block or text attachment, dogleg
//! geometry, arrowhead + line style references, and per-arrowhead /
//! per-block-label overrides. The full stream in the ODA Open Design
//! Specification v5.4.1 §19.4.85 runs ~60 fields with deep nesting
//! (per-leader-line parameters, per-dogleg parameters, per-block-
//! transform parameters).
//!
//! # 30-field cutoff
//!
//! This decoder implements the **first ~30 fields** of the R2010+
//! MLEADER layout — every field through `is_annotative` inclusive,
//! plus the two variable-length override blocks (`arrowhead_overrides`,
//! `block_labels`) and the two trailing attachment-enable booleans.
//! Trailing fields past that point (leader-node blocks, property
//! overrides, per-line-segment dogleg info, etc.) are read-and-ignored:
//! the cursor is simply advanced past the entity body by the object
//! stream's outer bit-size envelope — this decoder does not attempt
//! to surface those fields.
//!
//! Rationale: the ~30 covered fields cover everything a viewer needs
//! to classify a multileader, render its leader geometry references,
//! and identify its attached text or block. The trailing fields
//! govern overrides that a round-trip writer would need, which is
//! out of scope for the current read-only pipeline.
//!
//! # Version gate
//!
//! Only R2010 and later have this layout (spec §19.4.85 introduces
//! the MLEADER object class in the R2010 revision). Calling this
//! decoder on a pre-R2010 stream returns
//! [`Error::Unsupported`] — the older R2008 / R2009 MLEADER layout
//! is not implemented here.
//!
//! # Stream shape (R2010+, first 30 fields)
//!
//! ```text
//! BS    class_version                  -- currently 2
//! BS    content_type                   -- 1=mtext, 2=block, 3=none
//! BS    draw_mlearner_order_type       -- 0..=2
//! BS    draw_leader_order_type         -- 0..=2
//! BL    max_leader_segments_points     -- cap 1000
//! BD    first_seg_angle_constraint
//! BD    second_seg_angle_constraint
//! BS    leader_type                    -- 0..=3
//! BS    line_color                     -- ACI index (CMC simplified)
//! H     linetype_handle
//! BL    line_weight
//! B     enable_landing
//! B     enable_dogleg
//! BD    landing_distance
//! BD    arrow_head_size
//! BS    content_type_again             -- mirror of content_type
//! TV    mtext_default_text             -- may be empty
//! BD3   text_normal_direction
//! H     text_style_handle
//! BS    text_left_attachment_type
//! BS    text_angle_type
//! BS    text_alignment_type
//! BS    text_color                     -- ACI index
//! BD    text_height
//! B     text_frame_enabled
//! B     use_default_mtext_text
//! BD3   block_content_normal
//! H     block_content_handle
//! BS    block_content_color            -- ACI index
//! BD3   block_content_scale
//! BD    block_content_rotation
//! BS    block_content_connection
//! B     is_annotative
//! BS    num_arrowhead_overrides        -- cap 64
//! for each override: BL index + H handle
//! BS    num_block_labels               -- cap 1024
//! for each label:    BL ui_index + BL ui_unit_type
//! B     enable_text_attachment_to_leader
//! B     enable_text_attachment_to_dogleg
//! ```

use crate::bitcursor::{BitCursor, Handle};
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

/// Maximum leader-segment points per MLEADER — mirror of
/// [`crate::limits::ParseLimits::max_leader_points`] but scoped to the
/// in-field `max_leader_segments_points` count so an adversarial file
/// cannot request a multi-gigabyte allocation via this field.
pub const MAX_LEADER_POINTS: usize = 1_000;

/// Maximum `num_arrowhead_overrides` entries accepted. Real MLEADERs
/// use a handful of arrowheads at most.
pub const MAX_ARROWHEAD_OVERRIDES: usize = 64;

/// Maximum `num_block_labels` entries accepted. Per the spec the label
/// list sizes with the block attribute definitions; 1024 is a
/// conservative cap above the practical ceiling.
pub const MAX_BLOCK_LABELS: usize = 1_024;

/// A single arrowhead override entry: which leader node index the
/// arrowhead applies to and a handle reference to the block-table
/// entry that defines the arrow geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrowheadOverride {
    pub index: u32,
    pub handle: Handle,
}

/// A single block-label entry: UI index + unit-type classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockLabel {
    pub ui_index: u32,
    pub ui_unit_type: u32,
}

/// Decoded MLEADER (R2010+, first ~30 fields — see module docs for the cutoff).
#[derive(Debug, Clone, PartialEq)]
pub struct MLeader {
    pub class_version: i16,
    pub content_type: i16,
    pub draw_mlearner_order_type: i16,
    pub draw_leader_order_type: i16,
    pub max_leader_segments_points: u32,
    pub first_seg_angle_constraint: f64,
    pub second_seg_angle_constraint: f64,
    pub leader_type: i16,
    pub line_color: i16,
    pub linetype_handle: Handle,
    pub line_weight: i32,
    pub enable_landing: bool,
    pub enable_dogleg: bool,
    pub landing_distance: f64,
    pub arrow_head_size: f64,
    pub content_type_again: i16,
    pub mtext_default_text: String,
    pub text_normal_direction: Vec3D,
    pub text_style_handle: Handle,
    pub text_left_attachment_type: i16,
    pub text_angle_type: i16,
    pub text_alignment_type: i16,
    pub text_color: i16,
    pub text_height: f64,
    pub text_frame_enabled: bool,
    pub use_default_mtext_text: bool,
    pub block_content_normal: Vec3D,
    pub block_content_handle: Handle,
    pub block_content_color: i16,
    pub block_content_scale: Point3D,
    pub block_content_rotation: f64,
    pub block_content_connection: i16,
    pub is_annotative: bool,
    pub arrowhead_overrides: Vec<ArrowheadOverride>,
    pub block_labels: Vec<BlockLabel>,
    pub enable_text_attachment_to_leader: bool,
    pub enable_text_attachment_to_dogleg: bool,
}

/// Decode an MLEADER entity body from the current cursor position.
///
/// The caller is expected to have already consumed the object header
/// (type code, handle, object-size-in-bits for R2000) and the common
/// entity preamble (spec §19.4.1) before invoking this function.
///
/// # Version
///
/// This decoder implements the R2010+ MLEADER layout per spec
/// §19.4.85. For pre-R2010 input the function returns
/// [`Error::Unsupported`] — the older R2008 / R2009 layout is not
/// implemented here.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<MLeader> {
    if !version.is_r2010_plus() {
        return Err(Error::Unsupported {
            feature: "MLEADER requires R2010+".into(),
        });
    }

    let class_version = c.read_bs()?;
    let content_type = c.read_bs()?;
    let draw_mlearner_order_type = c.read_bs()?;
    let draw_leader_order_type = c.read_bs()?;
    let max_leader_segments_points = c.read_bl()? as u32;
    if (max_leader_segments_points as usize) > MAX_LEADER_POINTS {
        return Err(Error::SectionMap(format!(
            "MLEADER max_leader_segments_points {max_leader_segments_points} \
             exceeds cap {MAX_LEADER_POINTS}"
        )));
    }
    let first_seg_angle_constraint = c.read_bd()?;
    let second_seg_angle_constraint = c.read_bd()?;
    let leader_type = c.read_bs()?;
    // CMC simplified to BS color index (AutoCAD color index / ACI),
    // matching the pattern used by LAYER, LIGHT, SUN, MATERIAL, and
    // MLINESTYLE decoders in this crate. Full §2.9 CMC parsing is
    // deferred.
    let line_color = c.read_bs()?;
    let linetype_handle = c.read_handle()?;
    let line_weight = c.read_bl()?;
    let enable_landing = c.read_b()?;
    let enable_dogleg = c.read_b()?;
    let landing_distance = c.read_bd()?;
    let arrow_head_size = c.read_bd()?;
    let content_type_again = c.read_bs()?;
    let mtext_default_text = read_tv(c, version)?;
    let text_normal_direction = read_bd3(c)?;
    let text_style_handle = c.read_handle()?;
    let text_left_attachment_type = c.read_bs()?;
    let text_angle_type = c.read_bs()?;
    let text_alignment_type = c.read_bs()?;
    let text_color = c.read_bs()?;
    let text_height = c.read_bd()?;
    let text_frame_enabled = c.read_b()?;
    let use_default_mtext_text = c.read_b()?;
    let block_content_normal = read_bd3(c)?;
    let block_content_handle = c.read_handle()?;
    let block_content_color = c.read_bs()?;
    let block_content_scale = read_bd3(c)?;
    let block_content_rotation = c.read_bd()?;
    let block_content_connection = c.read_bs()?;
    let is_annotative = c.read_b()?;

    // Variable-length: arrowhead overrides.
    let num_arrowhead_overrides = c.read_bs()? as i32;
    if num_arrowhead_overrides < 0 || (num_arrowhead_overrides as usize) > MAX_ARROWHEAD_OVERRIDES {
        return Err(Error::SectionMap(format!(
            "MLEADER num_arrowhead_overrides {num_arrowhead_overrides} \
             out of range [0, {MAX_ARROWHEAD_OVERRIDES}]"
        )));
    }
    let num_arrowhead_overrides = num_arrowhead_overrides as usize;
    if num_arrowhead_overrides > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "MLEADER num_arrowhead_overrides {num_arrowhead_overrides} \
             exceeds remaining_bits {}",
            c.remaining_bits()
        )));
    }
    let mut arrowhead_overrides = Vec::with_capacity(num_arrowhead_overrides);
    for _ in 0..num_arrowhead_overrides {
        let index = c.read_bl()? as u32;
        let handle = c.read_handle()?;
        arrowhead_overrides.push(ArrowheadOverride { index, handle });
    }

    // Variable-length: block labels.
    let num_block_labels = c.read_bs()? as i32;
    if num_block_labels < 0 || (num_block_labels as usize) > MAX_BLOCK_LABELS {
        return Err(Error::SectionMap(format!(
            "MLEADER num_block_labels {num_block_labels} \
             out of range [0, {MAX_BLOCK_LABELS}]"
        )));
    }
    let num_block_labels = num_block_labels as usize;
    if num_block_labels > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "MLEADER num_block_labels {num_block_labels} \
             exceeds remaining_bits {}",
            c.remaining_bits()
        )));
    }
    let mut block_labels = Vec::with_capacity(num_block_labels);
    for _ in 0..num_block_labels {
        let ui_index = c.read_bl()? as u32;
        let ui_unit_type = c.read_bl()? as u32;
        block_labels.push(BlockLabel {
            ui_index,
            ui_unit_type,
        });
    }

    let enable_text_attachment_to_leader = c.read_b()?;
    let enable_text_attachment_to_dogleg = c.read_b()?;

    // Trailing fields (leader-node blocks, property overrides, per-
    // line-segment dogleg info, etc.) are intentionally NOT parsed
    // here. See module docs for the 30-field cutoff rationale.

    Ok(MLeader {
        class_version,
        content_type,
        draw_mlearner_order_type,
        draw_leader_order_type,
        max_leader_segments_points,
        first_seg_angle_constraint,
        second_seg_angle_constraint,
        leader_type,
        line_color,
        linetype_handle,
        line_weight,
        enable_landing,
        enable_dogleg,
        landing_distance,
        arrow_head_size,
        content_type_again,
        mtext_default_text,
        text_normal_direction,
        text_style_handle,
        text_left_attachment_type,
        text_angle_type,
        text_alignment_type,
        text_color,
        text_height,
        text_frame_enabled,
        use_default_mtext_text,
        block_content_normal,
        block_content_handle,
        block_content_color,
        block_content_scale,
        block_content_rotation,
        block_content_connection,
        is_annotative,
        arrowhead_overrides,
        block_labels,
        enable_text_attachment_to_leader,
        enable_text_attachment_to_dogleg,
    })
}

/// Read a variable-length text field (TV per spec §2.8). R2007+ encodes
/// UTF-16LE bit-streams; R2004 and earlier use 8-bit bytes.
///
/// Duplicates the small helper used by `mtext.rs` and `tables::read_tv`
/// — kept local so the MLEADER decoder remains self-contained and
/// doesn't take a crate-private dependency on the tables module.
fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    // Defensive cap: each char is ≥ 8 bits for ASCII, 16 for UTF-16.
    // Reject lengths larger than the remaining byte budget.
    if len > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "MLEADER TV length {len} exceeds remaining_bits {}",
            c.remaining_bits()
        )));
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
            .map_err(|_| Error::SectionMap("MLEADER TV is not valid UTF-16".into()))
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

    /// Append a synthetic TV field to the writer. Matches the
    /// `read_tv` decoder shape above.
    fn write_tv(w: &mut BitWriter, s: &str, version: Version) {
        if s.is_empty() {
            w.write_bs_u(0);
            return;
        }
        if version.is_r2007_plus() {
            // Encode as UTF-16 LE; the decoder reads `len` u16 units.
            let units: Vec<u16> = s.encode_utf16().collect();
            w.write_bs_u(units.len() as u16);
            for u in units {
                w.write_rc((u & 0xFF) as u8);
                w.write_rc((u >> 8) as u8);
            }
        } else {
            let bytes = s.as_bytes();
            w.write_bs_u(bytes.len() as u16);
            for b in bytes {
                w.write_rc(*b);
            }
        }
    }

    /// Write a minimal R2010+ MLEADER with content_type=none, no
    /// arrowhead overrides, and no block labels. Returns the encoded
    /// bit stream.
    fn synth_minimal_mleader(version: Version) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.write_bs(2); // class_version
        w.write_bs(3); // content_type = none
        w.write_bs(0); // draw_mlearner_order_type
        w.write_bs(0); // draw_leader_order_type
        w.write_bl(10); // max_leader_segments_points (under cap)
        w.write_bd(0.0); // first_seg_angle_constraint
        w.write_bd(0.0); // second_seg_angle_constraint
        w.write_bs(1); // leader_type
        w.write_bs(256); // line_color = ByLayer (ACI)
        w.write_handle(0, 0); // linetype_handle null
        w.write_bl(-1); // line_weight (ByLayer sentinel)
        w.write_b(true); // enable_landing
        w.write_b(true); // enable_dogleg
        w.write_bd(0.5); // landing_distance
        w.write_bd(0.18); // arrow_head_size
        w.write_bs(3); // content_type_again (mirror)
        write_tv(&mut w, "", version); // mtext_default_text empty
        // text_normal_direction (0, 0, 1)
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_handle(0, 0); // text_style_handle null
        w.write_bs(1); // text_left_attachment_type
        w.write_bs(0); // text_angle_type
        w.write_bs(0); // text_alignment_type
        w.write_bs(256); // text_color ByLayer
        w.write_bd(2.5); // text_height
        w.write_b(false); // text_frame_enabled
        w.write_b(true); // use_default_mtext_text
        // block_content_normal (0, 0, 1)
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_handle(0, 0); // block_content_handle null
        w.write_bs(256); // block_content_color ByLayer
        // block_content_scale (1, 1, 1)
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(0.0); // block_content_rotation
        w.write_bs(0); // block_content_connection
        w.write_b(false); // is_annotative
        w.write_bs(0); // num_arrowhead_overrides
        w.write_bs(0); // num_block_labels
        w.write_b(true); // enable_text_attachment_to_leader
        w.write_b(true); // enable_text_attachment_to_dogleg
        w.into_bytes()
    }

    #[test]
    fn roundtrip_minimal_mleader() {
        let version = Version::R2018;
        let bytes = synth_minimal_mleader(version);
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c, version).expect("minimal MLEADER decodes");
        assert_eq!(m.class_version, 2);
        assert_eq!(m.content_type, 3); // none
        assert_eq!(m.draw_mlearner_order_type, 0);
        assert_eq!(m.draw_leader_order_type, 0);
        assert_eq!(m.max_leader_segments_points, 10);
        assert_eq!(m.first_seg_angle_constraint, 0.0);
        assert_eq!(m.second_seg_angle_constraint, 0.0);
        assert_eq!(m.leader_type, 1);
        assert_eq!(m.line_color, 256);
        assert_eq!(m.line_weight, -1);
        assert!(m.enable_landing);
        assert!(m.enable_dogleg);
        assert_eq!(m.landing_distance, 0.5);
        assert_eq!(m.arrow_head_size, 0.18);
        assert_eq!(m.content_type_again, 3);
        assert!(m.mtext_default_text.is_empty());
        assert_eq!(
            m.text_normal_direction,
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0
            }
        );
        assert_eq!(m.text_left_attachment_type, 1);
        assert_eq!(m.text_color, 256);
        assert_eq!(m.text_height, 2.5);
        assert!(!m.text_frame_enabled);
        assert!(m.use_default_mtext_text);
        assert_eq!(
            m.block_content_normal,
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0
            }
        );
        assert_eq!(m.block_content_color, 256);
        assert_eq!(
            m.block_content_scale,
            Point3D {
                x: 1.0,
                y: 1.0,
                z: 1.0
            }
        );
        assert_eq!(m.block_content_rotation, 0.0);
        assert_eq!(m.block_content_connection, 0);
        assert!(!m.is_annotative);
        assert!(m.arrowhead_overrides.is_empty());
        assert!(m.block_labels.is_empty());
        assert!(m.enable_text_attachment_to_leader);
        assert!(m.enable_text_attachment_to_dogleg);
    }

    #[test]
    fn decode_errors_on_pre_r2010() {
        // Synthesize a minimal R2018 body; the version-gate should
        // reject it even though the bits would parse if we allowed
        // R2007. The byte content is irrelevant — we should bail
        // BEFORE consuming any bits.
        let bytes = synth_minimal_mleader(Version::R2018);
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2007).expect_err("pre-R2010 must error");
        assert!(
            matches!(&err, Error::Unsupported { feature } if feature.contains("R2010")),
            "expected Unsupported(R2010+); got {err:?}"
        );
        // The error must surface BEFORE any bits are consumed, so the
        // cursor is still at position zero.
        assert_eq!(c.position_bits(), 0);
    }

    #[test]
    fn decode_errors_on_oversized_arrowheads() {
        // Walk the stream up to num_arrowhead_overrides, then emit
        // 1000 — well above MAX_ARROWHEAD_OVERRIDES (64).
        let version = Version::R2018;
        let mut w = BitWriter::new();
        w.write_bs(2);
        w.write_bs(3);
        w.write_bs(0);
        w.write_bs(0);
        w.write_bl(10);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bs(1);
        w.write_bs(256);
        w.write_handle(0, 0);
        w.write_bl(-1);
        w.write_b(true);
        w.write_b(true);
        w.write_bd(0.5);
        w.write_bd(0.18);
        w.write_bs(3);
        write_tv(&mut w, "", version);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_handle(0, 0);
        w.write_bs(1);
        w.write_bs(0);
        w.write_bs(0);
        w.write_bs(256);
        w.write_bd(2.5);
        w.write_b(false);
        w.write_b(true);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_handle(0, 0);
        w.write_bs(256);
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bs(0);
        w.write_b(false);
        // Oversized count — should trigger the guard.
        w.write_bs(1000);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, version).expect_err("oversized arrowheads must error");
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("num_arrowhead_overrides")),
            "expected SectionMap(num_arrowhead_overrides); got {err:?}"
        );
    }
}
