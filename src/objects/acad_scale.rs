//! ACAD_SCALE object (spec §19.6.8 — L6-15) — single entry in the
//! per-drawing scale list.
//!
//! The drawing's named-object dictionary carries an `ACAD_SCALELIST`
//! sub-dictionary; each value in that sub-dictionary points at one
//! of these `ACAD_SCALE` records. A scale is a named ratio of paper
//! units to drawing units — `1:1`, `1/4" = 1'-0"`, `1:50`, etc.
//!
//! # Stream shape
//!
//! ```text
//! TV    scale_name
//! BD    paper_units
//! BD    drawing_units
//! BS    flag                   -- bit 0 = is-unit-scale (1.0 = 1.0)
//! ```
//!
//! The decoded ratio `paper_units / drawing_units` is the numeric
//! scale; `scale_name` is the user-facing label, which may or may
//! not correspond exactly to that ratio (AutoCAD allows hand-edited
//! names).

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::tables::read_tv;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct AcadScale {
    pub scale_name: String,
    pub paper_units: f64,
    pub drawing_units: f64,
    pub flag: i16,
}

impl AcadScale {
    /// Numeric scale ratio = `paper_units / drawing_units`.
    /// Returns `None` if `drawing_units == 0.0` (malformed record).
    pub fn ratio(&self) -> Option<f64> {
        if self.drawing_units == 0.0 {
            None
        } else {
            Some(self.paper_units / self.drawing_units)
        }
    }

    /// True when bit 0 of `flag` is set — the record is the special
    /// "unit scale" (1:1) entry.
    pub fn is_unit_scale(&self) -> bool {
        (self.flag & 0x01) != 0
    }
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AcadScale> {
    let scale_name = read_tv(c, version)?;
    let paper_units = c.read_bd()?;
    let drawing_units = c.read_bd()?;
    let flag = c.read_bs()?;
    Ok(AcadScale {
        scale_name,
        paper_units,
        drawing_units,
        flag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn encode_tv_r2000(w: &mut BitWriter, s: &[u8]) {
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
    }

    #[test]
    fn roundtrip_one_to_one() {
        let mut w = BitWriter::new();
        encode_tv_r2000(&mut w, b"1:1");
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_bs(0x01); // unit scale flag
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(s.scale_name, "1:1");
        assert_eq!(s.ratio(), Some(1.0));
        assert!(s.is_unit_scale());
    }

    #[test]
    fn roundtrip_quarter_inch_scale() {
        let mut w = BitWriter::new();
        encode_tv_r2000(&mut w, b"1/4\" = 1'-0\"");
        w.write_bd(0.25);
        w.write_bd(12.0);
        w.write_bs(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(s.scale_name, "1/4\" = 1'-0\"");
        assert!((s.ratio().unwrap() - (0.25 / 12.0)).abs() < 1e-12);
        assert!(!s.is_unit_scale());
    }

    #[test]
    fn ratio_none_on_zero_denominator() {
        let mut w = BitWriter::new();
        encode_tv_r2000(&mut w, b"Degenerate");
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bs(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert!(s.ratio().is_none());
    }
}
