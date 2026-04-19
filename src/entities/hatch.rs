//! HATCH entity (§19.4.33) — filled hatch region.
//!
//! HATCH is among the densest entity types in DWG: every instance
//! encodes its boundary path tree (which can contain LINE / ARC /
//! ELLIPSE / SPLINE sub-edges, or an explicit polyline), its pattern
//! definition (line angles + offsets), gradient settings, and seed
//! points — typically 30-100 fields total. Here we decode the
//! *header* portion of the entity, which covers the fields most
//! downstream consumers need: gradient flag, pattern name,
//! solid-fill flag, pattern type, rotation, scale, pixel size.
//!
//! The boundary path tree + pattern line definitions are *skipped*
//! via the leading `num_paths` / `num_pattern_lines` fields; callers
//! needing to reconstruct path geometry should access the raw
//! object bytes directly.
//!
//! # Stream shape (partial — what this decoder reads)
//!
//! ```text
//! (R2004+)
//!   BL    gradient_flag       -- 0 = solid, 1 = gradient
//!   BL    reserved
//!   BD    gradient_angle
//!   BD    gradient_shift
//!   BL    single_color_gradient
//!   BD    gradient_tint_factor
//!   BL    num_gradient_stops
//!   for each stop:
//!     BD  value (0..=1)
//!     CMC color
//!   TV    gradient_name
//! BD3   extrusion
//! BD    elevation
//! TV    name
//! B     solid_fill_flag
//! B     associative_flag
//! BL    num_paths
//! (boundary path tree — skipped)
//! BS    style                 -- 0 = odd parity, 1 = outermost, 2 = entire
//! BS    pattern_type          -- 0 = user, 1 = predefined, 2 = custom
//! (if !solid_fill_flag)
//!   BD   rotation
//!   BD   scale_or_spacing
//!   B    double_flag
//!   BS   num_pattern_lines
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Vec3D, read_bd3};
use crate::error::Result;
use crate::tables::read_tv;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Hatch {
    pub gradient: Option<GradientInfo>,
    pub extrusion: Vec3D,
    pub elevation: f64,
    pub name: String,
    pub solid_fill: bool,
    pub associative: bool,
    pub num_paths: u32,
    pub style: i16,
    pub pattern_type: i16,
    pub pattern: Option<PatternInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GradientInfo {
    pub gradient_flag: u32,
    pub angle: f64,
    pub shift: f64,
    pub single_color: u32,
    pub tint_factor: f64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PatternInfo {
    pub rotation: f64,
    pub scale_or_spacing: f64,
    pub double_flag: bool,
    pub num_pattern_lines: u16,
}

pub fn decode_header(c: &mut BitCursor<'_>, version: Version) -> Result<Hatch> {
    let gradient = if version.is_r2004_plus() {
        let gradient_flag = c.read_bl()? as u32;
        if gradient_flag != 0 {
            let _reserved = c.read_bl()?;
            let angle = c.read_bd()?;
            let shift = c.read_bd()?;
            let single_color = c.read_bl()? as u32;
            let tint_factor = c.read_bd()?;
            let _num_stops = c.read_bl()?;
            // Stops are skipped — we don't decode CMC colors here.
            let name = read_tv(c, version)?;
            Some(GradientInfo {
                gradient_flag,
                angle,
                shift,
                single_color,
                tint_factor,
                name,
            })
        } else {
            None
        }
    } else {
        None
    };

    let extrusion = read_bd3(c)?;
    let elevation = c.read_bd()?;
    let name = read_tv(c, version)?;
    let solid_fill = c.read_b()?;
    let associative = c.read_b()?;
    let num_paths = c.read_bl()? as u32;
    // (boundary path tree skipped by caller)
    let style = c.read_bs()?;
    let pattern_type = c.read_bs()?;
    let pattern = if !solid_fill {
        Some(PatternInfo {
            rotation: c.read_bd()?,
            scale_or_spacing: c.read_bd()?,
            double_flag: c.read_b()?,
            num_pattern_lines: c.read_bs_u()?,
        })
    } else {
        None
    };
    Ok(Hatch {
        gradient,
        extrusion,
        elevation,
        name,
        solid_fill,
        associative,
        num_paths,
        style,
        pattern_type,
        pattern,
    })
}

/// Decode the full HATCH header + advance past the path tree. This
/// is an alias — path parsing is deferred to a later pass.
pub use decode_header as decode;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_solid_fill_hatch_r2000() {
        let mut w = BitWriter::new();
        // R2000 has no gradient block
        w.write_bd(0.0); w.write_bd(0.0); w.write_bd(1.0); // extrusion
        w.write_bd(0.0); // elevation
        let s = b"SOLID";
        w.write_bs_u(s.len() as u16);
        for b in s { w.write_rc(*b); }
        w.write_b(true); // solid fill
        w.write_b(false); // not associative
        w.write_bl(1); // 1 path (not decoded here)
        w.write_bs(0); // odd parity
        w.write_bs(1); // predefined
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(h.name, "SOLID");
        assert!(h.solid_fill);
        assert!(!h.associative);
        assert_eq!(h.num_paths, 1);
        assert!(h.pattern.is_none());
        assert!(h.gradient.is_none());
    }

    #[test]
    fn roundtrip_patterned_hatch_r2004() {
        // R2004 (R2004+ for gradient header, but not UTF-16 strings).
        let mut w = BitWriter::new();
        w.write_bl(0); // not a gradient
        w.write_bd(0.0); w.write_bd(0.0); w.write_bd(1.0); // extrusion
        w.write_bd(0.0); // elevation
        let s = b"ANSI31";
        w.write_bs_u(s.len() as u16);
        for b in s { w.write_rc(*b); }
        w.write_b(false); // not solid
        w.write_b(false); // not associative
        w.write_bl(1); // 1 path
        w.write_bs(0); w.write_bs(1);
        w.write_bd(0.0); // rotation
        w.write_bd(1.0); // scale
        w.write_b(false); // not doubled
        w.write_bs_u(1); // 1 pattern line
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = decode(&mut c, Version::R2004).unwrap();
        assert_eq!(h.name, "ANSI31");
        assert!(!h.solid_fill);
        let pat = h.pattern.unwrap();
        assert_eq!(pat.scale_or_spacing, 1.0);
        assert_eq!(pat.num_pattern_lines, 1);
    }
}
