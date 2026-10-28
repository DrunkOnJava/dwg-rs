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
