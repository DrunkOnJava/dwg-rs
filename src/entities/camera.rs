//! CAMERA entity (§19.4.92) — R2007+.
//!
//! CAMERA is largely an anchor for an associated named view: most of
//! what a user thinks of as "the camera" (target, up vector, lens
//! length, clipping) lives on a NAMEDVIEW object referenced by
//! `view_handle`. The entity itself carries just enough geometry to
//! place the camera glyph in a drawing.
//!
//! This decoder surfaces the minimal placement fields observed on
//! real R2010+ drawings:
//!
//! ```text
//! BD3  origin              -- camera position in WCS
//! BD3  direction            -- unit viewing direction
//! BD   focal_length         -- mm; default 50.0 when BD encodes 1.0
//! BD   field_of_view        -- radians
//! H    view_handle          -- deferred — raw handle only
//! ```
//!
//! The `view_handle` is parsed but not dereferenced; the associated
//! named view is looked up from the object graph later, when a caller
//! has the full handle map.
//!
//! CAMERA did not exist prior to R2007, so [`decode`] rejects older
//! versions with [`Error::Unsupported`] rather than silently mis-
//! decoding whatever happens to follow.

use crate::bitcursor::{BitCursor, Handle};
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

/// Decoded CAMERA entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    pub origin: Point3D,
    pub direction: Vec3D,
    pub focal_length: f64,
    pub field_of_view: f64,
    /// Handle reference to the associated NAMEDVIEW / VIEW object.
    /// Not dereferenced at decode time.
    pub view_handle: Handle,
}

/// Decode a CAMERA payload.
///
/// The cursor must already be positioned past the common entity
/// preamble. Returns [`Error::Unsupported`] for pre-R2007 versions.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Camera> {
    if !version.is_r2007_plus() {
        return Err(Error::Unsupported {
            feature: "CAMERA requires R2007+".into(),
        });
    }
    let origin = read_bd3(c)?;
    let direction = read_bd3(c)?;
    let focal_length = c.read_bd()?;
    let field_of_view = c.read_bd()?;
    let view_handle = c.read_handle()?;
    Ok(Camera {
        origin,
        direction,
        focal_length,
        field_of_view,
        view_handle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_camera_default_focal() {
        let mut w = BitWriter::new();
        // origin (1.0, 2.0, 3.0)
        w.write_bd(1.0);
        w.write_bd(2.0);
        w.write_bd(3.0);
        // direction (0.0, 0.0, -1.0)
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(-1.0);
        // focal length — BD 1.0 short-form represents 1.0, so use an
        // explicit double for 50.0mm.
        w.write_bd(50.0);
        // FoV ≈ 0.6 rad (about 34° full-angle, normal lens)
        w.write_bd(0.6);
        // view handle 5.1.0x42
        w.write_handle(5, 0x42);

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let cam = decode(&mut c, Version::R2010).unwrap();
        assert_eq!(
            cam.origin,
            Point3D {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }
        );
        assert_eq!(
            cam.direction,
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: -1.0
            }
        );
        assert_eq!(cam.focal_length, 50.0);
        assert!((cam.field_of_view - 0.6).abs() < 1e-12);
        assert_eq!(cam.view_handle.code, 5);
        assert_eq!(cam.view_handle.value, 0x42);
    }

    #[test]
    fn rejects_pre_r2007() {
        let bytes = [0u8; 4];
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2004).unwrap_err();
        assert!(
            matches!(err, Error::Unsupported { feature } if feature.contains("CAMERA")),
            "err={err:?}"
        );
    }
}
