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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::entities::modeler::tests::write_sat_blob;

    #[test]
    fn roundtrip_extruded_surface_with_blob() {
        let mut w = BitWriter::new();
        let blob = SatBlob {
            empty: false,
            version: 2,
            bytes: b"DUMMY SAT BYTES".to_vec(),
        };
        write_sat_blob(&mut w, &blob);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(10.0); // sweep +Z, 10 units
        w.write_bd(0.1_f64); // draft
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert_eq!(s.sat, blob);
        assert_eq!(
            s.sweep_vector,
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: 10.0
            }
        );
        assert!((s.draft_angle - 0.1).abs() < 1e-12);
    }

    #[test]
    fn roundtrip_extruded_surface_empty_blob() {
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: true,
                version: 0,
                bytes: Vec::new(),
            },
        );
        w.write_bd(1.0);
        w.write_bd(2.0);
        w.write_bd(3.0);
        w.write_bd(0.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert!(s.sat.empty);
        assert_eq!(
            s.sweep_vector,
            Vec3D {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }
        );
    }
}
