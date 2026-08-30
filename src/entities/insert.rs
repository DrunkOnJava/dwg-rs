//! INSERT entity (ODA Open Design Specification for .dwg files v5.4
//! §20.4.9) — block reference.
//!
//! An INSERT places one instance of a BLOCK (external or internal)
//! at a specific insertion point with optional scale and rotation.
//!
//! # Stream shape (R2000+)
//!
//! ```text
//! 3BD  insertion_point       10
//! BB   data_flags                -- how the scale is stored, below
//! ...  scale data
//! BD   rotation              50
//! 3BD  extrusion             210
//! B    has_attribs           66
//! (R2004+, and only when has_attribs)
//!   BL owned_object_count
//! B    (undocumented — see below)
//! ```
//!
//! `data_flags` selects one of four scale encodings (§20.4.9):
//!
//! | flags | meaning |
//! |-------|---------|
//! | `11`  | scale is `(1, 1, 1)`; nothing stored |
//! | `01`  | x is `1.0`; y and z are `DD`s defaulting to `1.0` |
//! | `10`  | x is an `RD`; y and z equal x |
//! | `00`  | x is an `RD`; y and z are `DD`s defaulting to x |
//!
//! The previous reading of this entity used `BD` scale components, a
//! `BE` extrusion and no owned-object count. On `sample_AC1032.dwg`
//! that overran three of the four INSERT records and mis-scaled the
//! fourth.
//!
//! # Measured: the owned-object count is conditional
//!
//! §20.4.9 tags `Owned Object Count` `R2004+` with no further guard.
//! On the four R2018 INSERT records of `sample_AC1032.dwg` the count
//! appears **only** when `has_attribs` is set:
//!
//! | handle | data flags | x scale | rotation | has_attribs | count | ends | boundary |
//! |--------|-----------|---------|----------|-------------|-------|------|----------|
//! | `0x660` | `10` | 2.5 | 3.9269908 | false | — | 405 | 405 |
//! | `0x661` | `10` | 2.5 | 3.9269908 | false | — | 405 | 405 |
//! | `0xC9E` | `10` | 2.5 | 3.9269908 | false | — | 405 | 405 |
//! | `0x79C` | `10` | 4.1911332 | 0 | true | 3 | 309 | 309 |
//!
//! Reading the count unconditionally leaves `0x660` one bit short of a
//! `BL`. (These records carry no string stream, so their boundary is
//! the bit *before* the `strings present` trailer flag — see
//! [`crate::string_stream::data_field_end`].)

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::Result;
use crate::version::Version;

/// Maximum `owned_object_count` accepted — an INSERT owns one ATTRIB
/// per attribute definition of its block.
pub const MAX_OWNED_OBJECTS: u32 = 65_536;

#[derive(Debug, Clone, PartialEq)]
pub struct Insert {
    pub insertion_point: Point3D,
    pub scale: Point3D,
    pub rotation: f64,
    pub extrusion: Vec3D,
    /// `B 66` — ATTRIB sub-entities follow this INSERT.
    pub has_attribs: bool,
    /// `BL` owned-object count (R2004+, only when `has_attribs`).
    pub owned_object_count: u32,
}

/// Decodes the `Insert` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Insert> {
    let insertion_point = read_bd3(c)?;
    let data_flags = c.read_bb()?;
    let scale = match data_flags {
        0b11 => Point3D {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
        0b01 => Point3D {
            x: 1.0,
            y: c.read_dd(1.0)?,
            z: c.read_dd(1.0)?,
        },
        0b10 => {
            let x = c.read_rd()?;
            Point3D { x, y: x, z: x }
        }
        _ => {
            let x = c.read_rd()?;
            Point3D {
                x,
                y: c.read_dd(x)?,
                z: c.read_dd(x)?,
            }
        }
    };
    let rotation = c.read_bd()?;
    let extrusion = read_bd3(c)?;
    let has_attribs = c.read_b()?;
    let owned_object_count = if has_attribs && version.is_r2004_plus() {
        let n = c.read_bl_u()?;
        if n > MAX_OWNED_OBJECTS {
            return Err(crate::error::Error::SectionMap(format!(
                "INSERT owned_object_count {n} exceeds cap {MAX_OWNED_OBJECTS}"
            )));
        }
        n
    } else {
        0
    };
    Ok(Insert {
        insertion_point,
        scale,
        rotation,
        extrusion,
        has_attribs,
        owned_object_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::string_stream::tests::{bits_of, build_payload};

    /// Write the R2018 common entity preamble a synthetic record needs
    /// in front of its body: no XDATA, no graphics preview, the values
    /// every entity of `sample_AC1032.dwg` carries.
    fn write_preamble(w: &mut BitWriter) {
        w.write_bs_u(0); // no XDATA
        w.write_b(false); // no graphics preview
        w.write_bb(0b10); // entmode
        w.write_bl(0); // num_reactors
        w.write_b(true); // no xdictionary
        w.write_b(false); // no AcDs binary data
        w.write_bs_u(0x0100); // colour
        w.write_bd(1.0); // linetype scale
        w.write_bb(0b00); // ltype flags
        w.write_bb(0b00); // plotstyle
        w.write_bb(0b00); // material
        w.write_rc(0); // shadow
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
        w.write_bs(0); // invisibility
        w.write_rc(0x1D); // lineweight
    }

    #[test]
    fn roundtrip_insert_uniform_scale_data_flag_10() {
        let mut w = BitWriter::new();
        w.write_bd(5.0);
        w.write_bd(10.0);
        w.write_bd(0.0);
        w.write_bb(0b10); // data flags: x as RD, y and z equal x
        w.write_rd(2.5);
        w.write_bd(0.0); // rotation
        w.write_bd(0.0); // extrusion
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_b(false); // no attribs, so no owned-object count
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let i = decode(&mut c, Version::R2018).unwrap();
        assert_eq!(
            i.insertion_point,
            Point3D {
                x: 5.0,
                y: 10.0,
                z: 0.0
            }
        );
        assert_eq!(
            i.scale,
            Point3D {
                x: 2.5,
                y: 2.5,
                z: 2.5
            }
        );
        assert_eq!(
            i.extrusion,
            Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0
            }
        );
        assert!(!i.has_attribs);
        assert_eq!(i.owned_object_count, 0);
    }

    #[test]
    fn roundtrip_insert_scale_data_flag_00() {
        let mut w = BitWriter::new();
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bb(0b00); // x as RD, y and z as DDs defaulting to x
        w.write_rd(2.0);
        w.write_dd(2.0, 3.0);
        w.write_dd(2.0, 4.0);
        w.write_bd(std::f64::consts::FRAC_PI_4);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_b(false);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let i = decode(&mut c, Version::R2018).unwrap();
        assert_eq!(
            i.scale,
            Point3D {
                x: 2.0,
                y: 3.0,
                z: 4.0
            }
        );
        assert!((i.rotation - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    }

    #[test]
    fn roundtrip_insert_unit_scale_data_flag_11() {
        let mut w = BitWriter::new();
        w.write_bd(1.0);
        w.write_bd(2.0);
        w.write_bd(0.0);
        w.write_bb(0b11); // scale is (1, 1, 1), nothing stored
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_b(false);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let i = decode(&mut c, Version::R2018).unwrap();
        assert_eq!(
            i.scale,
            Point3D {
                x: 1.0,
                y: 1.0,
                z: 1.0
            }
        );
    }

    /// The R2018 shape measured on `sample_AC1032.dwg` handle `0x79C`:
    /// data flags `10`, an ATTRIB-carrying INSERT whose owned-object
    /// count is `3`. The whole record is framed with its string stream
    /// so the decode is checked against the data-stream boundary.
    #[test]
    fn r2018_insert_with_attribs_closes_on_the_boundary() {
        let mut w = BitWriter::new();
        write_preamble(&mut w);
        w.write_bd(-3024.0);
        w.write_bd(1.95);
        w.write_bd(0.0);
        w.write_bb(0b10);
        w.write_rd(4.1911331805470775);
        w.write_bd(0.0); // rotation
        w.write_bd(0.0); // extrusion
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_b(true); // has attribs
        w.write_bl(3); // owned object count
        let body = bits_of(&w);
        let payload = build_payload(&body, &[]);

        // Reproduce the dispatcher's boundary check.
        let end = crate::string_stream::data_field_end(&payload, Version::R2018).unwrap();
        let mut c = BitCursor::new(&payload);
        crate::string_stream::seek(&mut c, 8).unwrap();
        crate::common_entity::read_common_entity_data(&mut c, Version::R2018).unwrap();
        let i = decode(&mut c, Version::R2018).unwrap();
        assert_eq!(c.position_bits(), end);
        assert!(i.has_attribs);
        assert_eq!(i.owned_object_count, 3);
        assert_eq!(i.scale.x, 4.1911331805470775);
    }
}
