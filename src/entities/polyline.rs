//! POLYLINE entity (§19.4.45) — legacy 2D/3D/mesh polyline.
//!
//! A POLYLINE holds flags and global width/thickness fields; the
//! actual vertex data lives in a chain of [`crate::entities::vertex::Vertex`]
//! sub-entities referenced by handle in the object stream.
//!
//! Modern AutoCAD writes LWPOLYLINE instead of POLYLINE for 2D work;
//! this decoder covers the legacy path and POLYLINE_3D / PFACE /
//! POLYMESH variants that still appear in older files.
//!
//! # Stream shape (2D POLYLINE — common variant)
//!
//! ```text
//! BS   flag             -- bits: 0x01 closed, 0x02 curve-fit, 0x04 spline-fit,
//!                          0x08 3D polyline, 0x10 3D polymesh, 0x20 closed
//!                          in N direction, 0x40 polyface, 0x80 linetype
//!                          generated continuously
//! BS   curve_type        -- 5=quadratic B-spline, 6=cubic, 8=Bezier
//! BD   default_start_width
//! BD   default_end_width
//! BT   thickness
//! BD   elevation
//! BE   extrusion
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Vec3D, read_be, read_bt};
use crate::error::Result;
use crate::version::Version;

/// POLYLINE_3D (§19.4.46) — the 3D polyline header.
///
/// Unlike [`Polyline`] (the 2D form) it carries no widths, thickness,
/// elevation or extrusion; the geometry lives entirely in the
/// `VERTEX_3D` sub-entities the record owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Polyline3d {
    /// The two flag bytes, kept verbatim so a writer can re-emit them.
    pub flags: [u8; 2],
    /// R2004+ owned-object count — the `VERTEX_3D` sub-entities
    /// reachable through this record's handle stream. `None` before
    /// R2004, where the field is absent.
    pub num_owned_objects: Option<u32>,
}

/// Decode a POLYLINE_3D header from a cursor parked past the common
/// entity preamble.
///
/// # Stream shape — measured
///
/// ```text
/// RC   flags_1
/// RC   flags_2
/// BL   num_owned_objects   -- R2004+ only
/// ```
///
/// The one POLYLINE_3D record of `sample_AC1032.dwg` (handle `0x42B`)
/// has a 26-bit data-field budget, and this list lands on it exactly
/// (`delta 0`, `examples/probe_entity_field_list.rs`). Its owned-object
/// count decodes to **5**, and the five records that immediately follow
/// it in the object stream — handles `0x42C`..`0x430` — are exactly
/// five `VERTEX_3D` sub-entities, closed by a SEQEND at `0x431`. The
/// `BL` is the same R2004+ owned-object count
/// [`crate::tables::block_record`] reads; `BS` cannot be separated from
/// `BL` at a value of 5, so only the width and the value are claimed.
pub fn decode_3d(c: &mut BitCursor<'_>, version: Version) -> Result<Polyline3d> {
    let flags = [c.read_rc()?, c.read_rc()?];
    let num_owned_objects = if version.is_r2004_plus() {
        Some(c.read_bl()? as u32)
    } else {
        None
    };
    Ok(Polyline3d {
        flags,
        num_owned_objects,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    pub flag: u16,
    pub curve_type: i16,
    pub default_start_width: f64,
    pub default_end_width: f64,
    pub thickness: f64,
    pub elevation: f64,
    pub extrusion: Vec3D,
}

impl Polyline {
    /// Bit 0x01 of `flag`: the polyline is closed.
    pub fn is_closed(&self) -> bool {
        self.flag & 0x01 != 0
    }
    /// Bit 0x08 of `flag`: the polyline is a 3D polyline.
    pub fn is_3d(&self) -> bool {
        self.flag & 0x08 != 0
    }
    /// Bit 0x40 of `flag`: the polyline is a polyface mesh.
    pub fn is_polyface(&self) -> bool {
        self.flag & 0x40 != 0
    }
}

/// Decodes the `Polyline` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Polyline> {
    let flag = c.read_bs_u()?;
    let curve_type = c.read_bs()?;
    let default_start_width = c.read_bd()?;
    let default_end_width = c.read_bd()?;
    let thickness = read_bt(c)?;
    let elevation = c.read_bd()?;
    let extrusion = read_be(c)?;
    Ok(Polyline {
        flag,
        curve_type,
        default_start_width,
        default_end_width,
        thickness,
        elevation,
        extrusion,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_closed_2d_polyline() {
        let mut w = BitWriter::new();
        w.write_bs_u(0x01); // closed
        w.write_bs(0); // no curve fit
        w.write_bd(0.0); // start width
        w.write_bd(0.0); // end width
        w.write_b(true); // default thickness
        w.write_bd(0.0); // elevation
        w.write_b(true); // default extrusion
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c).unwrap();
        assert!(p.is_closed());
        assert!(!p.is_3d());
        assert_eq!(p.thickness, 0.0);
    }

    /// The measured POLYLINE_3D shape: two zero flag bytes and an
    /// owned-object count of 5, in exactly 26 bits.
    #[test]
    fn roundtrip_measured_polyline_3d() {
        let mut w = BitWriter::new();
        w.write_rc(0);
        w.write_rc(0);
        w.write_bl(5);
        assert_eq!(w.position_bits(), 26);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode_3d(&mut c, Version::R2018).unwrap();
        assert_eq!(p.flags, [0, 0]);
        assert_eq!(p.num_owned_objects, Some(5));
        assert_eq!(c.position_bits(), 26);
    }

    /// Before R2004 the owned-object count is absent.
    #[test]
    fn polyline_3d_has_no_owned_count_before_r2004() {
        let mut w = BitWriter::new();
        w.write_rc(0x08);
        w.write_rc(0x00);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode_3d(&mut c, Version::R2000).unwrap();
        assert_eq!(p.flags, [0x08, 0x00]);
        assert_eq!(p.num_owned_objects, None);
        assert_eq!(c.position_bits(), 16);
    }

    #[test]
    fn roundtrip_3d_polyline() {
        let mut w = BitWriter::new();
        w.write_bs_u(0x08); // 3D
        w.write_bs(6); // cubic B-spline
        w.write_bd(0.1);
        w.write_bd(0.2);
        w.write_b(false); // explicit thickness
        w.write_bd(1.5);
        w.write_bd(10.0); // elevation
        w.write_b(true); // default extrusion
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c).unwrap();
        assert!(p.is_3d());
        assert_eq!(p.curve_type, 6);
        assert_eq!(p.thickness, 1.5);
        assert_eq!(p.elevation, 10.0);
    }
}
