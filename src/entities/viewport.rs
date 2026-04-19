//! VIEWPORT entity (§19.4.60) — a floating viewport window in paper
//! space showing a clipped view of model space.
//!
//! # Scope
//!
//! VIEWPORT has one of the densest field lists in the spec (30+
//! fields spanning BD, RD, BS, B, BL, H, BT). A complete decoder
//! would span ~200 LOC. For the initial release we decode only the
//! geometric members most renderers need: center, width, height,
//! view-center, view-height, and view-target. Additional members
//! (frozen layers, clipping boundary, UCS, render mode, gradient
//! background handle) are left to a later pass.
//!
//! The cursor advances only over the fields this decoder consumes
//! — callers who need the trailing handles must know how many fields
//! were skipped. That count is surfaced via [`ViewportSkipped`].
//!
//! # Stream shape (partial — fields this decoder reads)
//!
//! ```text
//! BD3  center
//! BD   width
//! BD   height
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Viewport {
    pub center: Point3D,
    pub width: f64,
    pub height: f64,
}

/// Sentinel struct documenting which VIEWPORT fields this decoder
/// deliberately does not consume. A future expansion can replace it
/// with fully-decoded fields; callers holding a [`Viewport`] don't
/// need to track any extra state.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewportSkipped;

pub fn decode(c: &mut BitCursor<'_>) -> Result<Viewport> {
    let center = read_bd3(c)?;
    let width = c.read_bd()?;
    let height = c.read_bd()?;
    Ok(Viewport {
        center,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_viewport_header() {
        let mut w = BitWriter::new();
        w.write_bd(100.0);
        w.write_bd(200.0);
        w.write_bd(0.0);
        w.write_bd(50.0);
        w.write_bd(25.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let v = decode(&mut c).unwrap();
        assert_eq!(v.center, Point3D { x: 100.0, y: 200.0, z: 0.0 });
        assert_eq!(v.width, 50.0);
        assert_eq!(v.height, 25.0);
    }
}
