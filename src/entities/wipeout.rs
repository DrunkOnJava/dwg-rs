//! WIPEOUT entity (§19.4.86, related to UNDERLAY) — opaque mask.
//!
//! WIPEOUT is a polygonal or rectangular mask that hides underlying
//! entities using the background color. It's implemented as a
//! custom-class entity (looked up via `AcDb:Classes` under the DXF
//! name `WIPEOUT` / `ACDBWIPEOUT`).
//!
//! # Stream shape
//!
//! ```text
//! BL    clip_state         -- 0=none, 1=rectangle, 2=polygon
//! BS    num_clip_vertices  -- capped at 100_000
//! BD2   × num_clip_vertices   clip_polygon
//! B     show_clipped
//! RC    brightness         -- 0..100
//! RC    contrast           -- 0..100
//! RC    fade               -- 0..100
//! ```
//!
//! Unlike IMAGE and UNDERLAY, WIPEOUT has no external file or
//! definition handle — the mask is fully described by its own payload.

use crate::bitcursor::BitCursor;
use crate::entities::Point2D;
use crate::error::{Error, Result};

/// Decoded WIPEOUT payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Wipeout {
    /// 0 = no clipping, 1 = rectangular clip, 2 = polygonal clip.
    pub clip_state: i32,
    pub clip_polygon: Vec<Point2D>,
    pub show_clipped: bool,
    pub brightness: u8,
    pub contrast: u8,
    pub fade: u8,
}

/// Real WIPEOUT clip polygons are a handful of vertices. The cap
/// matches IMAGE / UNDERLAY to catch pathological inputs.
const WIPEOUT_MAX_CLIP_VERTS: usize = 100_000;

/// Decode a WIPEOUT payload. The cursor must already be positioned
/// past the common entity preamble.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Wipeout> {
    let clip_state = c.read_bl()?;
    let num_clip_vertices = c.read_bs()? as usize;
    if num_clip_vertices > WIPEOUT_MAX_CLIP_VERTS || num_clip_vertices > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "WIPEOUT clip verts {num_clip_vertices} exceeds cap \
             ({WIPEOUT_MAX_CLIP_VERTS} or remaining_bits {})",
            c.remaining_bits()
        )));
    }
    let mut clip_polygon = Vec::with_capacity(num_clip_vertices);
    for _ in 0..num_clip_vertices {
        let x = c.read_bd()?;
        let y = c.read_bd()?;
        clip_polygon.push(Point2D { x, y });
    }
    let show_clipped = c.read_b()?;
    let brightness = c.read_rc()?;
    let contrast = c.read_rc()?;
    let fade = c.read_rc()?;
    Ok(Wipeout {
        clip_state,
        clip_polygon,
        show_clipped,
        brightness,
        contrast,
        fade,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_rect_wipeout() {
        let mut w = BitWriter::new();
        w.write_bl(1); // rect clip
        w.write_bs(2);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(10.0);
        w.write_bd(5.0);
        w.write_b(false); // don't show clipped
        w.write_rc(50);
        w.write_rc(75);
        w.write_rc(10);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let wp = decode(&mut c).unwrap();
        assert_eq!(wp.clip_state, 1);
        assert_eq!(wp.clip_polygon.len(), 2);
        assert!(!wp.show_clipped);
        assert_eq!(wp.brightness, 50);
        assert_eq!(wp.contrast, 75);
        assert_eq!(wp.fade, 10);
    }

    #[test]
    fn roundtrip_polygon_wipeout() {
        let mut w = BitWriter::new();
        w.write_bl(2); // polygon clip
        w.write_bs(4);
        for (x, y) in [(0.0f64, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
            w.write_bd(x);
            w.write_bd(y);
        }
        w.write_b(true);
        w.write_rc(0);
        w.write_rc(100);
        w.write_rc(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let wp = decode(&mut c).unwrap();
        assert_eq!(wp.clip_state, 2);
        assert_eq!(wp.clip_polygon.len(), 4);
        assert!(wp.show_clipped);
        assert_eq!(wp.contrast, 100);
    }

    #[test]
    fn roundtrip_wipeout_no_clip() {
        let mut w = BitWriter::new();
        w.write_bl(0);
        w.write_bs(0);
        w.write_b(true);
        w.write_rc(50);
        w.write_rc(50);
        w.write_rc(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let wp = decode(&mut c).unwrap();
        assert_eq!(wp.clip_state, 0);
        assert!(wp.clip_polygon.is_empty());
    }

    #[test]
    fn rejects_excessive_clip_verts() {
        let mut w = BitWriter::new();
        w.write_bl(2);
        w.write_bs(32767); // huge claimed count, no backing payload
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("WIPEOUT clip verts")),
            "err={err:?}"
        );
    }
}
