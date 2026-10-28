//! UNDERLAY entity family (§19.4.86) — PDF / DWF / DGN underlays.
//!
//! UNDERLAY places an external document (PDF page, DWF sheet, or DGN
//! design) beneath a drawing so it can be traced or referenced. The
//! three variants share one payload shape and one decoder; they differ
//! only in the DXF class name used to look them up from
//! `AcDb:Classes`:
//!
//! - `PDFUNDERLAY` (also `ACDBPDFUNDERLAY`)
//! - `DWFUNDERLAY` (also `ACDBDWFUNDERLAY`)
//! - `DGNUNDERLAY` (also `ACDBDGNUNDERLAY`)
//!
//! The [`UnderlayKind`] discriminator records which one the dispatcher
//! matched.
//!
//! # Stream shape
//!
//! ```text
//! 3BD  normal
//! 3BD  insertion_point
//! BD   rotation
//! 3BD  scale
//! RC   flags              -- 0x01 clip_on, 0x02 underlay_on,
//!                            0x04 monochrome, 0x08 adjust_for_background
//! RC   contrast           -- 0..100
//! RC   fade               -- 0..100
//! BS   num_clip_vertices  -- capped at 100_000
//! 2BD  × num_clip_vertices   clip_polygon
//! ```
//!
//! The reference to the underlay definition (an `H`) is not in this list: on every
//! release it sits past the data-stream boundary, in the handle stream, and is
//! not decoded.
//!
//! # Measured, not specified
//!
//! The ODA Open Design Specification v5.4 has no UNDERLAY section — the
//! class post-dates it — so this field list comes from bytes alone.
//! The one PDFUNDERLAY of `sample_AC1032.dwg` (handle `0xD07`) closes
//! exactly on its data-stream boundary at bit 381 with the order above,
//! and every value it yields is self-consistent: a unit normal
//! `(0, 0, 1)`, an insertion point `(16.2869…, 15, 0)`, a zero
//! rotation, a unit scale `(1, 1, 1)`, flags `3` (clip on + underlay
//! on), contrast `100`, fade `0`, and no clip vertices. The previous
//! order — insertion, scale, rotation, normal — put the scale on
//! `(0, 0, 1)` and the normal on `(1, 1, 1)`, and then read the
//! definition handle inline, which lives in the handle stream; the
//! record ran off its end.
//!
//! The dispatcher routes the class on every release (ARCHITECTURE.md §7bk). The
//! corpus holds one PDFUNDERLAY per release, R14 to R2018, and all seven
//! close exactly on their data-stream boundary; the six with a DXF twin (R2000
//! onward) equal it on the insertion point, flags, contrast and fade by handle.
//! An earlier version of this decoder also read an inline definition handle
//! before R2007, which overshot the boundary on R14, R2000 and R2004 by the width
//! of that handle (8, 8 and 16 bits): the handle is not in the data stream on
//! any release.
//!
//! **Not established by value:** the scale, rotation and normal (the DXF omits
//! them at their defaults, so no twin shows them) and the encoding of a clip
//! vertex. No record in the corpus carries a clip vertex, so `2BD` here is
//! unverified — the sibling IMAGE entity uses `2RD` for its clip boundary
//! (§20.4.80).
//!
//! The underlay definition handle points at an `ACDB_UNDERLAY_DEFINITION`
//! dictionary entry that carries the file path + page/layer selection;
//! looking that up is deferred to a later pass over the full object
//! graph.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};

/// Which underlay family a given [`Underlay`] record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnderlayKind {
    Pdf,
    Dwf,
    Dgn,
}

/// Decoded UNDERLAY payload. One shape covers all three variants; the
/// [`kind`](Self::kind) field preserves which DXF class the dispatcher
/// matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Underlay {
    pub kind: UnderlayKind,
    pub insertion_point: Point3D,
    pub scale: Vec3D,
    pub rotation: f64,
    pub normal: Vec3D,
    pub flags: u8,
    pub contrast: u8,
    pub fade: u8,
    pub clip_polygon: Vec<Point2D>,
}

impl Underlay {
    /// Bit 0x01 of `flags`: clipping is enabled.
    pub fn is_clip_on(&self) -> bool {
        self.flags & 0x01 != 0
    }
    /// Bit 0x02 of `flags`: the underlay is displayed.
    pub fn is_underlay_on(&self) -> bool {
        self.flags & 0x02 != 0
    }
    /// Bit 0x04 of `flags`: the underlay is drawn monochrome.
    pub fn is_monochrome(&self) -> bool {
        self.flags & 0x04 != 0
    }
    /// Bit 0x08 of `flags`: colors are adjusted for the background.
    pub fn is_adjust_for_background(&self) -> bool {
        self.flags & 0x08 != 0
    }
}

/// Real UNDERLAY clip polygons are a handful of vertices. The cap
/// matches the IMAGE entity (§19.4.35) to catch pathological inputs
/// without rejecting any realistic drawing.
const UNDERLAY_MAX_CLIP_VERTS: usize = 100_000;

/// Decode an UNDERLAY payload. The cursor must already be positioned
/// past the common entity preamble. The reference to the underlay definition is
/// in the handle stream, past the data-stream boundary, and is not read.
pub fn decode(c: &mut BitCursor<'_>, kind: UnderlayKind) -> Result<Underlay> {
    let normal = read_bd3(c)?;
    let insertion_point = read_bd3(c)?;
    let rotation = c.read_bd()?;
    let scale = read_bd3(c)?;
    let flags = c.read_rc()?;
    let contrast = c.read_rc()?;
    let fade = c.read_rc()?;
    let num_clip_vertices = c.read_bs()? as usize;
    if num_clip_vertices > UNDERLAY_MAX_CLIP_VERTS || num_clip_vertices > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "UNDERLAY clip verts {num_clip_vertices} exceeds cap \
             ({UNDERLAY_MAX_CLIP_VERTS} or remaining_bits {})",
            c.remaining_bits()
        )));
    }
    let mut clip_polygon = Vec::with_capacity(num_clip_vertices);
    for _ in 0..num_clip_vertices {
        let x = c.read_bd()?;
        let y = c.read_bd()?;
        clip_polygon.push(Point2D { x, y });
    }
    Ok(Underlay {
        kind,
        insertion_point,
        scale,
        rotation,
        normal,
        flags,
        contrast,
        fade,
        clip_polygon,
    })
}
