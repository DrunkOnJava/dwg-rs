//! MLINE entity (§19.4.71) — multi-line entity.
//!
//! MLINE renders as N parallel line elements that share a set of
//! vertex positions along one path. Each vertex carries per-element
//! sub-records (segment parameters + area-fill parameters) that
//! describe how each parallel line continues through the joint.
//!
//! The top-level structure is well-defined; the per-vertex × per-line
//! inner loops are preserved here as raw bit-length-tagged sub-record
//! bundles rather than fully decoded per-element payloads. This is an
//! honest partial decode — spec §19.4.71 documents the inner shape
//! but observed real-world files exercise it rarely enough that a
//! fully-typed decode here would ship untested code for code's sake.
//!
//! # Top-level stream shape
//!
//! ```text
//! BD    scale_factor
//! RC    justification        -- 0 = top, 1 = zero, 2 = bottom
//! BD3   scale_point          -- insertion point (WCS)
//! BD3   extrusion
//! BS    open_closed_flags    -- bit 0 = closed, bit 1 = suppressed_start,
//!                               bit 2 = suppressed_end (et al.)
//! RC    num_lines            -- parallel elements at each vertex (≤ 16 per spec)
//! BS    num_verts            -- vertices along the path
//! // For each vertex:
//! BD3     vertex_point
//! BD3     segment_direction
//! BD3     miter_direction
//! // For each (vertex × line):
//! BS       segment_parameter_count
//! BD × N   segment_parameters
//! BS       area_fill_parameter_count
//! BD × N   area_fill_parameters
//! H     mline_style_handle
//! ```
//!
//! # `num_lines` is an `RC`, not a `BS` — measured
//!
//! This module read `BS` there until the exact-boundary check reached
//! MLINE (#63). With `BS` all three MLINE records of
//! `sample_AC1032.dwg` (handles `0x38B`, `0x3A5`, `0x3A6`) decode
//! nonsense immediately — `lines = 521`, `verts = -11716` on two of
//! them — because the `BS` swallows the byte after it. Reading one
//! `RC` instead is the single-token change that lands all three
//! records exactly on their data-stream boundary (`delta 0`).
//!
//! The values corroborate the width rather than merely fitting it: all
//! three read `num_lines = 2`, which is what a two-element multiline
//! style produces, with `num_verts` 8 / 2 / 2, `justification`
//! Zero / Top / Zero and `scale_factor` 1.5 / 2.13 / 1.598 — every one
//! a value a drawing would plausibly hold, where the `BS` reading
//! produced a negative vertex count.
//!
//! # Honest partial decode
//!
//! This decoder fully parses the top-level fields and vertex point
//! triples (`vertex_point`, `segment_direction`, `miter_direction`).
//! The per-vertex × per-line sub-record bundles are NOT further
//! expanded into named fields — they are stored as
//! [`MlineVertexSubrecord`] instances containing the parameter counts
//! and raw `Vec<f64>` parameter arrays. Spec §19.4.71 is authoritative
//! for how to interpret those parameters when a caller needs them.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};

// ========================================================================
// Defensive caps — derived from the ODA spec's practical-limits guidance
// cross-checked against observed real-world MLINEs. Real drawings top out
// at 2–4 parallel elements and a few dozen vertices; the caps here leave
// four orders of magnitude of headroom before rejecting.
// ========================================================================
const CAP_LINES: usize = 1_024;
const CAP_VERTS: usize = 100_000;
const CAP_SEGMENT_PARAMS: usize = 1_024;
const CAP_AREA_FILL_PARAMS: usize = 1_024;

/// Justification type (§19.4.71 — MLINEJUST in AutoCAD terminology).
///
/// Values outside `{0, 1, 2}` land in [`Justification::Unknown`]
/// rather than rejecting — the spec reserves the top 6 bits for
/// future extension, and we prefer honest pass-through to a hard error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Justification {
    Top,
    Zero,
    Bottom,
    Unknown(u8),
}

impl Justification {
    /// Maps the MLINE justification code (0 top, 1 zero, 2 bottom); unknown codes are preserved as `Unknown`.
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Top,
            1 => Self::Zero,
            2 => Self::Bottom,
            other => Self::Unknown(other),
        }
    }
}

/// Per-element sub-record carried at each (vertex × line) combination.
///
/// Holds the literal `Vec<f64>` parameter arrays — the spec defines
/// their semantics but consumer code rarely needs the interpretation,
/// so the typed form is kept honest-partial here.
#[derive(Debug, Clone, PartialEq)]
pub struct MlineVertexSubrecord {
    pub segment_parameters: Vec<f64>,
    pub area_fill_parameters: Vec<f64>,
}

/// One vertex along the MLINE path.
#[derive(Debug, Clone, PartialEq)]
pub struct MlineVertex {
    pub vertex_point: Point3D,
    pub segment_direction: Vec3D,
    pub miter_direction: Vec3D,
    /// Per-element sub-records. `sub_records.len() == num_lines`.
    pub sub_records: Vec<MlineVertexSubrecord>,
}

/// Fully-decoded MLINE entity.
#[derive(Debug, Clone, PartialEq)]
pub struct MLine {
    pub scale_factor: f64,
    pub justification: Justification,
    pub scale_point: Point3D,
    pub extrusion: Vec3D,
    pub open_closed_flags: i16,
    pub num_lines: u16,
    pub vertices: Vec<MlineVertex>,
}

/// Decodes the `MLine` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>) -> Result<MLine> {
    let scale_factor = c.read_bd()?;
    let justification = Justification::from_code(c.read_rc()?);
    let scale_point = read_bd3(c)?;
    let extrusion = read_bd3(c)?;
    let open_closed_flags = c.read_bs()?;
    let num_lines = c.read_rc()? as usize;
    let num_verts_signed = c.read_bs()?;

    // Defensive: a negative vertex count is nonsense — the spec emits
    // an unsigned value here but `read_bs` returns i16.
    if num_verts_signed < 0 {
        return Err(Error::SectionMap(format!(
            "MLINE negative vertex count {num_verts_signed}"
        )));
    }
    let num_verts = num_verts_signed as usize;

    let remaining = c.remaining_bits();
    if num_lines > CAP_LINES
        || num_verts > CAP_VERTS
        // Minimum per-vertex cost: 3 × BD3 = 3 × 3 × 2-bit-prefix = 18 bits,
        // plus num_lines × (2 × 16-bit count + 0 params) ⇒ 18 + 32 × num_lines
        // bits. Use a conservative 18-bit floor here; pairs well with the
        // per-subrecord checks further down.
        || num_verts.saturating_mul(18) > remaining
    {
        return Err(Error::SectionMap(format!(
            "MLINE implausible counts (lines={num_lines}, verts={num_verts}; \
             remaining_bits={remaining})"
        )));
    }

    let mut vertices = Vec::with_capacity(num_verts);
    for _ in 0..num_verts {
        let vertex_point = read_bd3(c)?;
        let segment_direction = read_bd3(c)?;
        let miter_direction = read_bd3(c)?;

        let mut sub_records = Vec::with_capacity(num_lines);
        for _ in 0..num_lines {
            let seg_count_signed = c.read_bs()?;
            if seg_count_signed < 0 {
                return Err(Error::SectionMap(format!(
                    "MLINE negative segment parameter count {seg_count_signed}"
                )));
            }
            let seg_count = seg_count_signed as usize;
            let rem_now = c.remaining_bits();
            if seg_count > CAP_SEGMENT_PARAMS
                // Each BD is ≥ 2 bits; cheap floor.
                || seg_count.saturating_mul(2) > rem_now
            {
                return Err(Error::SectionMap(format!(
                    "MLINE segment param count {seg_count} exceeds cap \
                     ({CAP_SEGMENT_PARAMS} or remaining_bits {rem_now})"
                )));
            }
            let mut segment_parameters = Vec::with_capacity(seg_count);
            for _ in 0..seg_count {
                segment_parameters.push(c.read_bd()?);
            }

            let af_count_signed = c.read_bs()?;
            if af_count_signed < 0 {
                return Err(Error::SectionMap(format!(
                    "MLINE negative area-fill parameter count {af_count_signed}"
                )));
            }
            let af_count = af_count_signed as usize;
            let rem_now = c.remaining_bits();
            if af_count > CAP_AREA_FILL_PARAMS || af_count.saturating_mul(2) > rem_now {
                return Err(Error::SectionMap(format!(
                    "MLINE area-fill param count {af_count} exceeds cap \
                     ({CAP_AREA_FILL_PARAMS} or remaining_bits {rem_now})"
                )));
            }
            let mut area_fill_parameters = Vec::with_capacity(af_count);
            for _ in 0..af_count {
                area_fill_parameters.push(c.read_bd()?);
            }

            sub_records.push(MlineVertexSubrecord {
                segment_parameters,
                area_fill_parameters,
            });
        }

        vertices.push(MlineVertex {
            vertex_point,
            segment_direction,
            miter_direction,
            sub_records,
        });
    }

    Ok(MLine {
        scale_factor,
        justification,
        scale_point,
        extrusion,
        open_closed_flags,
        num_lines: num_lines as u16,
        vertices,
    })
}
