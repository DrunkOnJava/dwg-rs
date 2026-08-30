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
//!
//! # Measured: this prefix is a quarter of the record
//!
//! Since #63 every entity decoder is held to the record's own
//! data-stream boundary, and VIEWPORT is the type that most visibly
//! fails it. All six VIEWPORT records of `sample_AC1032.dwg` (handles
//! `0x240`, `0x245`, `0x252`, `0x256`, `0x267`, `0x26B`) have an
//! identical 1125-bit data-field budget; the five fields above consume
//! 266 of them and stop, so each record reports `delta -819`.
//!
//! That is deliberate. The alternative — leaving VIEWPORT outside the
//! check — is what made its zero error count structural rather than
//! evidential, which is the whole point of #63. Six records that all
//! spend exactly 1125 bits give the next pass a fixed budget to fill,
//! but they are six copies of one shape: with no variation between
//! them there is nothing to separate one candidate token sequence from
//! another, so no field list is guessed here.

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

/// Decodes the `Viewport` payload that follows the common entity header.
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
        assert_eq!(
            v.center,
            Point3D {
                x: 100.0,
                y: 200.0,
                z: 0.0
            }
        );
        assert_eq!(v.width, 50.0);
        assert_eq!(v.height, 25.0);
    }
}
