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
//! BD3  insertion_point
//! BD3  scale
//! BD   rotation
//! BD3  normal
//! RC   flags              -- bits per spec §19.4.86:
//!                             0x01 clip_on
//!                             0x02 underlay_on
//!                             0x04 monochrome
//!                             0x08 adjust_for_background
//! RC   contrast           -- 0..100
//! RC   fade               -- 0..100
//! BS   num_clip_vertices  -- capped at 100_000
//! BD2  × num_clip_vertices   clip_polygon
//! H    underlay_definition_handle   -- parsed but not dereferenced
//! ```
//!
//! The underlay definition handle points at an `ACDB_UNDERLAY_DEFINITION`
//! dictionary entry that carries the file path + page/layer selection;
//! looking that up is deferred to a later pass over the full object
//! graph.

use crate::bitcursor::{BitCursor, Handle};
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
    /// Handle to the `ACDB_UNDERLAY_DEFINITION` dictionary entry. Not
    /// dereferenced at decode time.
    pub definition_handle: Handle,
}

impl Underlay {
    pub fn is_clip_on(&self) -> bool {
        self.flags & 0x01 != 0
    }
    pub fn is_underlay_on(&self) -> bool {
        self.flags & 0x02 != 0
    }
    pub fn is_monochrome(&self) -> bool {
        self.flags & 0x04 != 0
    }
    pub fn is_adjust_for_background(&self) -> bool {
        self.flags & 0x08 != 0
    }
}

/// Real UNDERLAY clip polygons are a handful of vertices. The cap
/// matches the IMAGE entity (§19.4.35) to catch pathological inputs
/// without rejecting any realistic drawing.
const UNDERLAY_MAX_CLIP_VERTS: usize = 100_000;

/// Decode an UNDERLAY payload. The cursor must already be positioned
/// past the common entity preamble.
pub fn decode(c: &mut BitCursor<'_>, kind: UnderlayKind) -> Result<Underlay> {
    let insertion_point = read_bd3(c)?;
    let scale = read_bd3(c)?;
    let rotation = c.read_bd()?;
    let normal = read_bd3(c)?;
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
    let definition_handle = c.read_handle()?;
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
        definition_handle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn write_minimal_underlay(flags: u8, clip_verts: &[(f64, f64)]) -> Vec<u8> {
        let mut w = BitWriter::new();
        // insertion_point
        w.write_bd(1.0);
        w.write_bd(2.0);
        w.write_bd(0.0);
        // scale
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(1.0);
        // rotation
        w.write_bd(0.0);
        // normal
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        // flags / contrast / fade
        w.write_rc(flags);
        w.write_rc(75);
        w.write_rc(20);
        // clip polygon
        w.write_bs(clip_verts.len() as i16);
        for (x, y) in clip_verts {
            w.write_bd(*x);
            w.write_bd(*y);
        }
        // definition handle — reference code 5, value 0x0A
        w.write_handle(5, 0x0A);
        w.into_bytes()
    }

    #[test]
    fn roundtrip_pdf_underlay_with_rect_clip() {
        let bytes = write_minimal_underlay(0x03, &[(0.0, 0.0), (10.0, 10.0)]);
        let mut c = BitCursor::new(&bytes);
        let u = decode(&mut c, UnderlayKind::Pdf).unwrap();
        assert_eq!(u.kind, UnderlayKind::Pdf);
        assert_eq!(
            u.insertion_point,
            Point3D {
                x: 1.0,
                y: 2.0,
                z: 0.0
            }
        );
        assert_eq!(u.contrast, 75);
        assert_eq!(u.fade, 20);
        assert_eq!(u.clip_polygon.len(), 2);
        assert!(u.is_clip_on());
        assert!(u.is_underlay_on());
        assert!(!u.is_monochrome());
        assert!(!u.is_adjust_for_background());
        assert_eq!(u.definition_handle.code, 5);
        assert_eq!(u.definition_handle.value, 0x0A);
    }

    #[test]
    fn roundtrip_dwf_underlay_no_clip() {
        let bytes = write_minimal_underlay(0x02, &[]);
        let mut c = BitCursor::new(&bytes);
        let u = decode(&mut c, UnderlayKind::Dwf).unwrap();
        assert_eq!(u.kind, UnderlayKind::Dwf);
        assert!(u.clip_polygon.is_empty());
        assert!(!u.is_clip_on());
        assert!(u.is_underlay_on());
    }

    #[test]
    fn roundtrip_dgn_underlay_preserves_flags() {
        let bytes = write_minimal_underlay(0x0F, &[(0.0, 0.0)]);
        let mut c = BitCursor::new(&bytes);
        let u = decode(&mut c, UnderlayKind::Dgn).unwrap();
        assert_eq!(u.kind, UnderlayKind::Dgn);
        assert_eq!(u.flags, 0x0F);
        assert!(u.is_monochrome());
        assert!(u.is_adjust_for_background());
    }

    #[test]
    fn rejects_excessive_clip_verts() {
        // Write a payload that *claims* far more verts than fit, via a
        // BS that encodes a large positive value. We stop after the BS
        // since decode should reject before reading the polygon.
        let mut w = BitWriter::new();
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_rc(0);
        w.write_rc(0);
        w.write_rc(0);
        // A BS that reads as 32767 (max i16). 32767 BD2s need ~16 million
        // bits of payload but only a handful remain.
        w.write_bs(32767);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, UnderlayKind::Pdf).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("UNDERLAY clip verts")),
            "err={err:?}"
        );
    }
}
