//! ACAD_SCALE object (spec §19.6.8 — L6-15) — single entry in the
//! per-drawing scale list.
//!
//! The drawing's named-object dictionary carries an `ACAD_SCALELIST`
//! sub-dictionary; each value in that sub-dictionary points at one
//! of these `ACAD_SCALE` records. A scale is a named ratio of paper
//! units to drawing units — `1:1`, `1/4" = 1'-0"`, `1:50`, etc.
//!
//! `SCALE` is a custom class (`AcDbScale`), so its object type code is
//! assigned per file through `AcDb:Classes`; the dispatcher resolves it
//! by DXF class name.
//!
//! # Stream shape — measured
//!
//! ```text
//! BS    version
//! TV    scale_name
//! BD    paper_units
//! BD    drawing_units
//! B     is_unit_scale
//! ```
//!
//! An earlier cut of this decoder read `TV, BD, BD, BS` — no leading
//! `BS`, and a `BS flag` where the wire has a single `B`. The bytes say
//! otherwise, in both layouts:
//!
//! - `arc_2004.dwg` (AC1018), the `1:1` record (handle 67). Its `RL`
//!   object-data-size leaves 49 bits after the common object data, and
//!   they are spent exactly as `BS = 0` (2 bits), `TV` of length 4
//!   whose bytes are `31 3A 31 00` — `"1:1"` and its NUL — (46 bits),
//!   `BD = 1.0` (2 bits), `BD = 1.0` (2 bits), `B = 1` (1 bit). The
//!   `B` reading is corroborated semantically: `1:1` is precisely the
//!   record AutoCAD marks as the unit scale.
//! - `arc_2013.dwg` (AC1027), the `1:2` record (handle 68). With the
//!   name in the string stream the budget is 71 bits: `BS = 0`,
//!   `BD = 1.0`, `BD` as a full 66-bit double whose eight little-endian
//!   bytes are `00 00 00 00 00 00 00 40` — 2.0 — and `B = 0`.
//!   Its `1:1` sibling (handle 67) spends 7 bits on the same fields
//!   with both `BD`s taking the 1.0 short form and `B = 1`.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct AcadScale {
    /// Leading `BS`; 0 in every record observed.
    pub version: i16,
    pub scale_name: String,
    pub paper_units: f64,
    pub drawing_units: f64,
    /// True for the record AutoCAD treats as the 1:1 unit scale.
    pub is_unit_scale: bool,
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
}

/// Read the ACAD_SCALE field list from whichever streams hold it.
fn read_body(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<AcadScale> {
    let scale_version = c.read_bs()?;
    let scale_name = modern::read_tv(c, strings, version)?;
    let paper_units = c.read_bd()?;
    let drawing_units = c.read_bd()?;
    let is_unit_scale = c.read_b()?;
    Ok(AcadScale {
        version: scale_version,
        scale_name,
        paper_units,
        drawing_units,
        is_unit_scale,
    })
}

/// Decodes the `AcadScale` payload that follows the common object header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AcadScale> {
    read_body(c, &mut None, version)
}

/// Decode an ACAD_SCALE straight from its raw object payload, taking the
/// scale name from the R2007+ string stream when the file has one and
/// checking the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadScale> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings, version)?;
    split.finish("SCALE")?;
    Ok(out)
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
        w.write_bs(0);
        encode_tv_r2000(&mut w, b"1:1");
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_b(true);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(s.scale_name, "1:1");
        assert_eq!(s.ratio(), Some(1.0));
        assert!(s.is_unit_scale);
    }

    #[test]
    fn roundtrip_quarter_inch_scale() {
        let mut w = BitWriter::new();
        w.write_bs(0);
        encode_tv_r2000(&mut w, b"1/4\" = 1'-0\"");
        w.write_bd(0.25);
        w.write_bd(12.0);
        w.write_b(false);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(s.scale_name, "1/4\" = 1'-0\"");
        assert!((s.ratio().unwrap() - (0.25 / 12.0)).abs() < 1e-12);
        assert!(!s.is_unit_scale);
    }

    #[test]
    fn ratio_none_on_zero_denominator() {
        let mut w = BitWriter::new();
        w.write_bs(0);
        encode_tv_r2000(&mut w, b"Degenerate");
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_b(false);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert!(s.ratio().is_none());
    }

    #[test]
    fn r2018_split_stream_scale_reads_its_name_from_the_string_stream() {
        let mut body = modern::tests::r2018_object_prefix(1);
        body.write_bs(0); // version
        body.write_bd(1.0); // paper units
        body.write_bd(2.0); // drawing units
        body.write_b(false); // not the unit scale
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["1:2"]);
        let s = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(s.scale_name, "1:2");
        assert_eq!(s.paper_units, 1.0);
        assert_eq!(s.drawing_units, 2.0);
        assert!(!s.is_unit_scale);
        assert_eq!(s.ratio(), Some(0.5));
    }

    /// The pre-fix field list (`TV, BD, BD, BS`, no leading `BS`) must
    /// no longer satisfy the boundary check.
    #[test]
    fn r2018_split_stream_scale_rejects_the_old_field_list() {
        let mut body = modern::tests::r2018_object_prefix(1);
        body.write_bd(1.0);
        body.write_bd(1.0);
        body.write_bs(1);
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["1:1"]);
        assert!(decode_object(&payload, 8, None, Version::R2018).is_err());
    }
}
