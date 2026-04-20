//! LOFTEDSURFACE entity — ODA Open Design Specification v5.4.1
//! §19.4.81 (L4-40 in the entity inventory).
//!
//! A lofted surface is built from a sequence of cross-section
//! profiles (each a separate entity — circle, polyline, spline, …),
//! interpolated along an implied or explicit rail. The lofted body
//! is cached as an ACIS SAT blob; the cross-section handles and the
//! start / end tangent magnitudes are preserved so a regenerator
//! can re-loft without going through ACIS.
//!
//! # Stream shape
//!
//! ```text
//! <SAT blob>                          -- crate::entities::modeler
//! BL      num_cross_sections          -- count; 0 is legal (degenerate loft)
//! <handle>* cross_section_handles     -- num_cross_sections refs, deferred
//! BD      start_tangent_mag           -- magnitude at first cross-section
//! BD      end_tangent_mag             -- magnitude at last cross-section
//! ```
//!
//! # Handle storage
//!
//! As with [`swept_surface`](super::swept_surface) the cross-section
//! references are raw handles — resolution against the object stream
//! is deferred to a later pass. Cross-section order matches stream
//! order, which matches loft interpolation order.

use crate::bitcursor::{BitCursor, Handle};
use crate::entities::modeler::{SatBlob, decode_sat_blob};
use crate::error::{Error, Result};

/// Upper bound on cross-section count. Real lofted surfaces almost
/// always have 2–10 cross-sections; 10K is absurdly generous while
/// still capping adversarial inputs.
pub const MAX_LOFT_CROSS_SECTIONS: usize = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub struct LoftedSurface {
    /// Opaque ACIS SAT body — may be empty.
    pub sat: SatBlob,
    /// Raw handle references to each cross-section entity (ordered).
    pub cross_section_handles: Vec<Handle>,
    /// Tangent magnitude at the starting cross-section (controls
    /// blend smoothness).
    pub start_tangent_mag: f64,
    /// Tangent magnitude at the ending cross-section.
    pub end_tangent_mag: f64,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<LoftedSurface> {
    let sat = decode_sat_blob(c)?;
    let num_cross_sections = c.read_bl()?;
    if num_cross_sections < 0 {
        return Err(Error::SectionMap(format!(
            "LOFTEDSURFACE cross-section count {num_cross_sections} is negative"
        )));
    }
    let count = num_cross_sections as usize;

    // Cross-check: each handle is at least 8 bits (4-bit code +
    // 4-bit zero counter). A count larger than the remaining payload
    // cannot be valid.
    let remaining = c.remaining_bits();
    if count > MAX_LOFT_CROSS_SECTIONS || count.saturating_mul(8) > remaining {
        return Err(Error::SectionMap(format!(
            "LOFTEDSURFACE cross-section count {count} exceeds cap \
             ({MAX_LOFT_CROSS_SECTIONS}) or remaining bits ({remaining})"
        )));
    }

    let mut cross_section_handles = Vec::with_capacity(count);
    for _ in 0..count {
        cross_section_handles.push(c.read_handle()?);
    }
    let start_tangent_mag = c.read_bd()?;
    let end_tangent_mag = c.read_bd()?;

    Ok(LoftedSurface {
        sat,
        cross_section_handles,
        start_tangent_mag,
        end_tangent_mag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::entities::modeler::tests::write_sat_blob;

    #[test]
    fn roundtrip_loft_three_sections() {
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: false,
                version: 2,
                bytes: b"LOFT".to_vec(),
            },
        );
        w.write_bl(3);
        w.write_handle(2, 0x10);
        w.write_handle(2, 0x11);
        w.write_handle(2, 0x12);
        w.write_bd(1.0);
        w.write_bd(1.5);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert_eq!(s.cross_section_handles.len(), 3);
        assert_eq!(s.cross_section_handles[0].value, 0x10);
        assert_eq!(s.cross_section_handles[2].value, 0x12);
        assert!((s.start_tangent_mag - 1.0).abs() < 1e-12);
        assert!((s.end_tangent_mag - 1.5).abs() < 1e-12);
    }

    #[test]
    fn roundtrip_loft_zero_sections() {
        // Degenerate but legal: a loft stub with no cross-sections.
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: true,
                version: 0,
                bytes: Vec::new(),
            },
        );
        w.write_bl(0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert!(s.cross_section_handles.is_empty());
    }

    #[test]
    fn rejects_negative_count() {
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: true,
                version: 0,
                bytes: Vec::new(),
            },
        );
        w.write_bl(-1);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c).unwrap_err();
        assert!(matches!(err, Error::SectionMap(_)));
    }

    #[test]
    fn rejects_implausible_count() {
        // Claim more cross-sections than the cap allows.
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: true,
                version: 0,
                bytes: Vec::new(),
            },
        );
        w.write_bl(i32::MAX / 2);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c).unwrap_err();
        assert!(matches!(err, Error::SectionMap(_)));
    }
}
