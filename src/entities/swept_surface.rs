//! SWEPTSURFACE entity — ODA Open Design Specification v5.4.1
//! §19.4.80 (L4-39 in the entity inventory).
//!
//! A swept surface is a profile translated along an arbitrary path
//! (another entity — line, arc, spline, polyline, …). Unlike
//! [`extruded_surface`](super::extruded_surface) the sweep path is
//! not a straight vector; the path entity's handle is stored
//! alongside the cached ACIS body.
//!
//! # Stream shape
//!
//! ```text
//! <SAT blob>                  -- crate::entities::modeler::decode_sat_blob
//! <handle> path_entity        -- reference to the sweep-path entity
//! BB       align_option       -- 0..3 alignment enum; see [`AlignOption`]
//! ```
//!
//! # Handle storage
//!
//! Per spec §19.4.1 and §2.13, the path entity reference is a
//! handle (4-bit code + 4-bit counter + up to 15 bytes). The handle
//! is consumed as raw bits at this point in the stream; resolution
//! into a concrete `RawObject` is deferred to a later pass once the
//! full object stream has been walked and a handle-to-offset map
//! assembled. Per-entity decoders intentionally do not resolve
//! handles — they record the reference for the caller.

use crate::bitcursor::{BitCursor, Handle};
use crate::entities::modeler::{SatBlob, decode_sat_blob};
use crate::error::{Error, Result};

/// Alignment option for the sweep path (BB enum, §19.4.80).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignOption {
    /// 0 — do not align the profile to the path; profile keeps its
    /// original orientation throughout the sweep.
    NoAlignment,
    /// 1 — align the profile's normal to the path tangent.
    Normal,
    /// 2 — translate the profile to the path start without rotating.
    Translate,
    /// 3 — align the profile's Y axis to the path bank (roll the
    /// profile around the path tangent).
    ForcedAlignment,
}

impl AlignOption {
    pub fn from_bb(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Self::NoAlignment),
            1 => Ok(Self::Normal),
            2 => Ok(Self::Translate),
            3 => Ok(Self::ForcedAlignment),
            other => Err(Error::SectionMap(format!(
                "SWEPTSURFACE alignment option {other} not in 0..=3"
            ))),
        }
    }

    pub fn to_bb(self) -> u8 {
        match self {
            Self::NoAlignment => 0,
            Self::Normal => 1,
            Self::Translate => 2,
            Self::ForcedAlignment => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SweptSurface {
    /// Opaque ACIS SAT body — may be empty.
    pub sat: SatBlob,
    /// Raw handle reference to the path entity. Resolution deferred
    /// to the caller after the full handle map is built.
    pub path_handle: Handle,
    /// Profile-to-path alignment mode.
    pub align_option: AlignOption,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<SweptSurface> {
    let sat = decode_sat_blob(c)?;
    let path_handle = c.read_handle()?;
    let align_option = AlignOption::from_bb(c.read_bb()?)?;
    Ok(SweptSurface {
        sat,
        path_handle,
        align_option,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::entities::modeler::tests::write_sat_blob;

    #[test]
    fn roundtrip_swept_surface_with_handle() {
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: false,
                version: 2,
                bytes: b"SW".to_vec(),
            },
        );
        // Soft-owner handle (code 2), value 0x42.
        w.write_handle(2, 0x42);
        w.write_bb(1); // Normal alignment
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert_eq!(s.sat.version, 2);
        assert_eq!(s.path_handle.code, 2);
        assert_eq!(s.path_handle.value, 0x42);
        assert_eq!(s.align_option, AlignOption::Normal);
    }

    #[test]
    fn rejects_bad_alignment_enum() {
        // The BB read at the end of decode() will only ever yield
        // 0..=3, so the enum converter cannot be driven to an
        // out-of-range value from decode(). Still, exercise the
        // converter directly so the error path is covered.
        assert!(AlignOption::from_bb(4).is_err());
        assert!(matches!(
            AlignOption::from_bb(4).unwrap_err(),
            Error::SectionMap(_)
        ));
    }

    #[test]
    fn align_option_roundtrip() {
        for opt in [
            AlignOption::NoAlignment,
            AlignOption::Normal,
            AlignOption::Translate,
            AlignOption::ForcedAlignment,
        ] {
            assert_eq!(AlignOption::from_bb(opt.to_bb()).unwrap(), opt);
        }
    }
}
