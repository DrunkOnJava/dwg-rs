//! WIPEOUT entity — opaque mask, stored as a raster-image record.
//!
//! WIPEOUT is a polygonal or rectangular mask that hides underlying
//! entities using the background colour. It is a custom-class entity
//! (looked up via `AcDb:Classes` under the DXF name `WIPEOUT` /
//! `ACDBWIPEOUT`, C++ class `AcDbWipeout`).
//!
//! # Stream shape — measured, identical to IMAGE
//!
//! `AcDbWipeout` writes the [`AcDbRasterImage`](super::image) record
//! verbatim; this module is a thin alias over [`super::image::decode`].
//!
//! The evidence is `sample_AC1032.dwg` (R2018), which holds one IMAGE
//! (handle `0x662`) and one WIPEOUT (handle `0x44D`). Both records
//! carry a 140-byte graphics-preview block sized by the same `BLL`, and
//! their data streams are **bit-identical for the first 175 bits** —
//! through the preview header and into its payload. Decoding the
//! WIPEOUT with the IMAGE field list yields insertion point
//! `(271.921…, 3.988…, 0)`, u/v vectors of length `31.7077…`, image
//! size `(1, 1)`, display flags `7`, clipping `true`, brightness `50`,
//! contrast `50`, fade `0`, clip mode `0`, clip boundary type `2` and
//! four clip vertices — and lands on bit 2185 of a record whose data
//! stream ends at 2186, one bit of byte padding later.
//!
//! The previous field list here (`BL clip_state`, `BS count`, `BD2`
//! vertices, `B show_clipped`, three `RC`s) matched no part of those
//! bytes; it decoded the graphics-preview payload as geometry.
//!
//! Only R2018 has been measured. The pre-R2010 branch is whatever
//! [`super::image::decode`] does for those versions.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::version::Version;

/// Decoded WIPEOUT payload — the raster-image record, measured
/// identical to [`super::image::Image`].
pub type Wipeout = super::image::Image;

/// Decode a WIPEOUT payload. The cursor must already be positioned
/// past the common entity preamble.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Wipeout> {
    super::image::decode(c, version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::entities::image::ClipBoundary;

    /// The measured `sample_AC1032.dwg` WIPEOUT body: a polygon clip
    /// with an explicit vertex count, decoded through the IMAGE layout.
    #[test]
    fn decodes_the_measured_polygon_wipeout_body() {
        let mut w = BitWriter::new();
        w.write_bl(0); // class_version
        w.write_bd(271.9211288808192); // insertion point
        w.write_bd(3.988123351850959);
        w.write_bd(0.0);
        w.write_bd(31.707742926072683); // u vector
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // v vector
        w.write_bd(31.707742926072683);
        w.write_bd(0.0);
        w.write_rd(1.0); // image size
        w.write_rd(1.0);
        w.write_bs(7); // display flags
        w.write_b(true); // clipping
        w.write_rc(50); // brightness
        w.write_rc(50); // contrast
        w.write_rc(0); // fade
        w.write_b(false); // clip mode (R2010+)
        w.write_bs(2); // polygon boundary
        w.write_bl(4); // vertex count
        for (x, y) in [(0.0f64, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            w.write_rd(x);
            w.write_rd(y);
        }
        let end = w.position_bits();

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let wp = decode(&mut c, Version::R2018).unwrap();
        assert_eq!(wp.brightness, 50);
        assert_eq!(wp.contrast, 50);
        assert_eq!(wp.fade, 0);
        assert!(wp.clipping);
        match &wp.clip_boundary {
            ClipBoundary::Polygon(pts) => assert_eq!(pts.len(), 4),
            other => panic!("expected a polygon boundary, got {other:?}"),
        }
        assert_eq!(c.position_bits(), end);
    }
}
