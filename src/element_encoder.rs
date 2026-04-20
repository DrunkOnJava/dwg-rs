//! `ElementEncoder` trait — entity-to-bitstream encoders (L12-05, task #378).
//!
//! Inverse of the per-entity `decode` functions in [`crate::entities`]. Each
//! implementor writes the typed-field order documented in the ODA Open Design
//! Specification v5.4.1 §19.4.x that the matching decoder cites. The trait
//! writes the *type-specific payload only* — the common entity preamble and
//! object header are the caller's responsibility, mirroring the decoder
//! convention (see [`crate::entities::line::decode`] et al.).
//!
//! # Round-trip invariant
//!
//! For every implementation, the composition
//!
//! ```text
//!   BitWriter → encode → BitCursor → decode
//! ```
//!
//! must recover the original struct. Tests below verify this for each of the
//! four entity types shipped in this round (LINE, CIRCLE, ARC, POINT).
//!
//! # Versioning
//!
//! `encode` takes a [`Version`] parameter to match the decoder ABI even when
//! the current implementations are version-agnostic. Later entity types
//! (TEXT, INSERT, DIMENSION family) branch on version to emit the correct
//! sub-type fields.

use crate::bitwriter::BitWriter;
use crate::entities::{Vec3D, arc::Arc, circle::Circle, line::Line, point::Point};
use crate::error::Result;
use crate::version::Version;

/// Encode a typed DWG entity into a [`BitWriter`].
///
/// Each implementation writes the spec-defined field order for one entity
/// kind. Invariant: the bytes produced by [`Self::encode`] round-trip
/// through the matching `entities::*::decode` to yield an equal struct.
pub trait ElementEncoder {
    /// Append this entity's type-specific payload to `writer`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error`] if any sub-encoder fails. Current
    /// implementations never fail (they call infallible [`BitWriter`]
    /// primitives), but the signature preserves future compatibility with
    /// entity types that need fallible encoders (e.g. variable-length text
    /// fields with unencodable lengths).
    fn encode(&self, writer: &mut BitWriter, version: Version) -> Result<()>;
}

/// Write a BE (bit-extrusion) per spec §2.11 — single bit flag plus three
/// BDs when non-default.
///
/// `(0, 0, 1)` is the overwhelmingly common case and collapses to one
/// `true` bit; any other vector expands to `false` + 3 BDs. Inverse of
/// [`crate::entities::read_be`].
fn write_be(w: &mut BitWriter, v: Vec3D) {
    if v.x == 0.0 && v.y == 0.0 && v.z == 1.0 {
        w.write_b(true);
    } else {
        w.write_b(false);
        w.write_bd(v.x);
        w.write_bd(v.y);
        w.write_bd(v.z);
    }
}

/// Write a BT (bit-thickness) per spec §2.12 — one bit flag, defaults 0.0.
///
/// Inverse of [`crate::entities::read_bt`].
fn write_bt(w: &mut BitWriter, v: f64) {
    if v == 0.0 {
        w.write_b(true);
    } else {
        w.write_b(false);
        w.write_bd(v);
    }
}

/// LINE entity encoder (spec §19.4.20).
///
/// Writes the z-flag 2D shortcut: when [`Line::is_2d`] is true, both Z
/// coordinates are omitted (decoder defaults them to 0.0). End coordinates
/// are delta-encoded relative to start coordinates (BD over the delta).
///
/// Inverse of [`crate::entities::line::decode`].
impl ElementEncoder for Line {
    fn encode(&self, w: &mut BitWriter, _version: Version) -> Result<()> {
        w.write_b(self.is_2d);
        w.write_rd(self.start.x);
        w.write_bd(self.end.x - self.start.x);
        w.write_rd(self.start.y);
        w.write_bd(self.end.y - self.start.y);
        if !self.is_2d {
            w.write_rd(self.start.z);
            w.write_bd(self.end.z - self.start.z);
        }
        write_bt(w, self.thickness);
        write_be(w, self.extrusion);
        Ok(())
    }
}

/// CIRCLE entity encoder (spec §19.4.8).
///
/// Inverse of [`crate::entities::circle::decode`].
impl ElementEncoder for Circle {
    fn encode(&self, w: &mut BitWriter, _version: Version) -> Result<()> {
        w.write_bd(self.center.x);
        w.write_bd(self.center.y);
        w.write_bd(self.center.z);
        w.write_bd(self.radius);
        write_bt(w, self.thickness);
        write_be(w, self.extrusion);
        Ok(())
    }
}

/// ARC entity encoder (spec §19.4.2).
///
/// Field order matches the decoder: center, radius, thickness, extrusion,
/// start_angle, end_angle. Angles are in radians.
///
/// Inverse of [`crate::entities::arc::decode`].
impl ElementEncoder for Arc {
    fn encode(&self, w: &mut BitWriter, _version: Version) -> Result<()> {
        w.write_bd(self.center.x);
        w.write_bd(self.center.y);
        w.write_bd(self.center.z);
        w.write_bd(self.radius);
        write_bt(w, self.thickness);
        write_be(w, self.extrusion);
        w.write_bd(self.start_angle);
        w.write_bd(self.end_angle);
        Ok(())
    }
}

/// POINT entity encoder (spec §19.4.27).
///
/// Inverse of [`crate::entities::point::decode`].
impl ElementEncoder for Point {
    fn encode(&self, w: &mut BitWriter, _version: Version) -> Result<()> {
        w.write_bd(self.position.x);
        w.write_bd(self.position.y);
        w.write_bd(self.position.z);
        write_bt(w, self.thickness);
        write_be(w, self.extrusion);
        w.write_bd(self.x_axis_angle);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitcursor::BitCursor;
    use crate::entities::{Point3D, arc, circle, line, point};

    fn default_extrusion() -> Vec3D {
        Vec3D {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        }
    }

    // -------- LINE round-trips --------

    #[test]
    fn line_2d_roundtrips_via_encoder_and_decoder() {
        let original = Line {
            start: Point3D {
                x: 1.0,
                y: 2.0,
                z: 0.0,
            },
            end: Point3D {
                x: 6.0,
                y: 5.0,
                z: 0.0,
            },
            thickness: 0.0,
            extrusion: default_extrusion(),
            is_2d: true,
        };
        let mut w = BitWriter::new();
        original.encode(&mut w, Version::R2018).unwrap();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let round = line::decode(&mut c).unwrap();
        assert_eq!(round, original);
    }

    #[test]
    fn line_3d_with_explicit_thickness_and_extrusion_roundtrips() {
        let original = Line {
            start: Point3D {
                x: 1.0,
                y: 3.0,
                z: 5.0,
            },
            end: Point3D {
                x: 3.0,
                y: 7.0,
                z: 11.0,
            },
            thickness: 2.5,
            extrusion: Vec3D {
                x: 0.5,
                y: 0.25,
                z: 0.75,
            },
            is_2d: false,
        };
        let mut w = BitWriter::new();
        original.encode(&mut w, Version::R2018).unwrap();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let round = line::decode(&mut c).unwrap();
        assert_eq!(round, original);
    }

    // -------- CIRCLE round-trip --------

    #[test]
    fn circle_default_thickness_and_extrusion_roundtrips() {
        let original = Circle {
            center: Point3D {
                x: 10.0,
                y: 20.0,
                z: 0.0,
            },
            radius: 5.0,
            thickness: 0.0,
            extrusion: default_extrusion(),
        };
        let mut w = BitWriter::new();
        original.encode(&mut w, Version::R2018).unwrap();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let round = circle::decode(&mut c).unwrap();
        assert_eq!(round, original);
    }

    // -------- ARC round-trip --------

    #[test]
    fn arc_quarter_circle_roundtrips() {
        let original = Arc {
            center: Point3D {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            radius: 10.0,
            thickness: 0.0,
            extrusion: default_extrusion(),
            start_angle: 0.0,
            end_angle: std::f64::consts::FRAC_PI_2,
        };
        let mut w = BitWriter::new();
        original.encode(&mut w, Version::R2018).unwrap();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let round = arc::decode(&mut c).unwrap();
        assert_eq!(round.center, original.center);
        assert_eq!(round.radius, original.radius);
        assert_eq!(round.start_angle, original.start_angle);
        assert!((round.end_angle - original.end_angle).abs() < 1e-12);
    }

    // -------- POINT round-trip --------

    #[test]
    fn point_default_fields_roundtrip() {
        let original = Point {
            position: Point3D {
                x: 1.25,
                y: 2.5,
                z: 3.75,
            },
            thickness: 0.0,
            extrusion: default_extrusion(),
            x_axis_angle: 0.0,
        };
        let mut w = BitWriter::new();
        original.encode(&mut w, Version::R2018).unwrap();
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let round = point::decode(&mut c).unwrap();
        assert_eq!(round, original);
    }

    // -------- BE/BT encoder/decoder symmetry --------

    #[test]
    fn be_default_encodes_as_one_bit() {
        let mut w = BitWriter::new();
        write_be(&mut w, default_extrusion());
        // 1 bit + 7 padding = 1 byte.
        assert_eq!(w.as_slice().len(), 1);
        // The one bit is true (0x80 after MSB-first packing).
        assert_eq!(w.as_slice()[0] & 0x80, 0x80);
    }

    #[test]
    fn bt_nonzero_encodes_flag_plus_bd() {
        let mut w = BitWriter::new();
        write_bt(&mut w, 2.5);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        // Flag bit should be false, then a BD follows.
        let flag = c.read_b().unwrap();
        assert!(!flag);
        let read = c.read_bd().unwrap();
        assert_eq!(read, 2.5);
    }
}
