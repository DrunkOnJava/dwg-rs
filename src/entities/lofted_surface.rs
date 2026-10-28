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

/// Decodes the `LoftedSurface` payload that follows the common entity header.
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
