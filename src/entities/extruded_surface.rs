//! EXTRUDEDSURFACE entity — ODA Open Design Specification v5.4.1
//! §19.4.78 (L4-37 in the entity inventory).
//!
//! An extruded surface is a 2D or 3D profile swept along a direction
//! vector by a fixed sweep extent. Autodesk caches the resulting
//! analytic surface as an ACIS SAT blob and stores the parametric
//! inputs alongside so a regenerator can reconstruct the surface
//! without round-tripping through the ACIS parser.
//!
//! # Stream shape
//!
//! ```text
//! <SAT blob>            -- see crate::entities::modeler::decode_sat_blob
//! BD3 sweep_vector      -- world-space extrusion direction * magnitude
//! BD  draft_angle        -- spec §19.4.78, radians
//! ```
//!
//! Real drawings also include a large number of parametric flags
//! (whether the profile is a wire vs region, whether the surface is
//! trimmed, an optional pre-extrusion transform matrix, etc.). This
//! decoder captures the load-bearing direction + extent fields and
//! leaves the secondary parameters in the raw bytes. Callers that
//! need full extrusion geometry should parse the SAT blob; the
//! direction + extent pair here is sufficient to re-derive the
//! visible silhouette when combined with the profile entity it wraps.

use crate::bitcursor::BitCursor;
use crate::entities::Vec3D;
use crate::entities::modeler::{SatBlob, decode_sat_blob};
use crate::entities::read_bd3;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudedSurface {
    /// Opaque ACIS SAT body — may be empty if the drawing stored
    /// only the parametric definition.
    pub sat: SatBlob,
    /// Extrusion direction + magnitude in WCS. The sweep extent is
    /// `sweep_vector.norm()`; the unit direction is
    /// `sweep_vector / sweep_vector.norm()`.
    pub sweep_vector: Vec3D,
    /// Draft angle (radians). Positive values taper outward along
    /// the sweep direction.
    pub draft_angle: f64,
}

/// Decodes the `ExtrudedSurface` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>) -> Result<ExtrudedSurface> {
    let sat = decode_sat_blob(c)?;
    let sweep_vector = read_bd3(c)?;
    let draft_angle = c.read_bd()?;
    Ok(ExtrudedSurface {
        sat,
        sweep_vector,
        draft_angle,
    })
}
