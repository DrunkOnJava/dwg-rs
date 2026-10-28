//! DIMENSION (diameter) — L4-19 variant, ODA §19.4.22.
//!
//! Diameter dimensions share the bit layout of radius dimensions
//! (see [`crate::entities::dimension_radial`]) — only the object-type
//! code (0x1A vs 0x19) differs. This module re-exports the diameter
//! type and its dedicated decoder from `dimension_radial` so the
//! family can be navigated by spec-name without duplicating bit-level
//! decode logic.
//!
//! # Stream shape (suffix only)
//!
//! ```text
//! BD3  center_point      -- "def point 10"
//! BD3  first_arc_point   -- "def point 15"
//! BD   leader_length
//! ```

pub use crate::entities::dimension_radial::{
    DimensionDiameter, DimensionRadialSuffix, decode_diameter as decode,
};
