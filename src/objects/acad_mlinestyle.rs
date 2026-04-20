//! ACAD_MLINESTYLE object (spec §19.6.4 — L6-13) — multiline style
//! definition (up to 16 parallel line elements).
//!
//! MLINESTYLE is the style record backing the MLINE entity: it
//! describes how many parallel lines are drawn, each line's offset
//! from the reference axis, colour, and linetype.
//!
//! # Stream shape
//!
//! ```text
//! TV       name
//! TV       description
//! BS       flags
//! BS       fill_color           -- ACI index (simplified CMC)
//! BD       start_angle
//! BD       end_angle
//! RC       num_lines            -- capped at 16 (format-limit)
//! // Per line (num_lines times):
//! BD       offset
//! BS       color                -- ACI index
//! H        linetype_handle
//! ```
//!
//! Note: the spec permits up to 16 lines per MLINESTYLE. A file
//! claiming more than 16 is either adversarial or a format
//! corruption; the decoder rejects it.

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};
use crate::tables::read_tv;
use crate::version::Version;

/// Format-limit cap on the number of lines a single MLINESTYLE may
/// carry (spec §19.6.4).
const MAX_MLINESTYLE_LINES: usize = 16;

/// One of the parallel line elements in an MLINESTYLE.
#[derive(Debug, Clone, PartialEq)]
pub struct MlineStyleLine {
    pub offset: f64,
    pub color: i16,
    pub linetype_handle: Handle,
}

/// Decoded ACAD_MLINESTYLE body.
#[derive(Debug, Clone, PartialEq)]
pub struct AcadMlinestyle {
    pub name: String,
    pub description: String,
    pub flags: i16,
    pub fill_color: i16,
    pub start_angle: f64,
    pub end_angle: f64,
    pub lines: Vec<MlineStyleLine>,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AcadMlinestyle> {
    let name = read_tv(c, version)?;
    let description = read_tv(c, version)?;
    let flags = c.read_bs()?;
    let fill_color = c.read_bs()?;
    let start_angle = c.read_bd()?;
    let end_angle = c.read_bd()?;
    let num_lines = c.read_rc()? as usize;
    if num_lines > MAX_MLINESTYLE_LINES {
        return Err(Error::SectionMap(format!(
            "ACAD_MLINESTYLE claims {num_lines} lines (max {MAX_MLINESTYLE_LINES} per spec §19.6.4)"
        )));
    }
    let mut lines = Vec::with_capacity(num_lines);
    for _ in 0..num_lines {
        let offset = c.read_bd()?;
        let color = c.read_bs()?;
        let linetype_handle = c.read_handle()?;
        lines.push(MlineStyleLine {
            offset,
            color,
            linetype_handle,
        });
    }
    Ok(AcadMlinestyle {
        name,
        description,
        flags,
        fill_color,
        start_angle,
        end_angle,
        lines,
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
    fn roundtrip_standard_style() {
        let mut w = BitWriter::new();
        encode_tv_r2000(&mut w, b"STANDARD");
        encode_tv_r2000(&mut w, b"");
        w.write_bs(0); // flags
        w.write_bs(256); // fill_color = BYLAYER-ish sentinel
        w.write_bd(90.0); // start angle
        w.write_bd(90.0); // end angle
        w.write_rc(2); // 2 lines
        // line 1: offset 0.5, color 1, linetype handle 5.1.0x10
        w.write_bd(0.5);
        w.write_bs(1);
        w.write_handle(5, 0x10);
        // line 2: offset -0.5, color 2, linetype handle 5.1.0x11
        w.write_bd(-0.5);
        w.write_bs(2);
        w.write_handle(5, 0x11);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(s.name, "STANDARD");
        assert_eq!(s.lines.len(), 2);
        assert_eq!(s.lines[0].offset, 0.5);
        assert_eq!(s.lines[0].color, 1);
        assert_eq!(s.lines[0].linetype_handle.value, 0x10);
        assert_eq!(s.lines[1].offset, -0.5);
        assert_eq!(s.fill_color, 256);
    }

    #[test]
    fn rejects_too_many_lines() {
        let mut w = BitWriter::new();
        encode_tv_r2000(&mut w, b"BIG");
        encode_tv_r2000(&mut w, b"");
        w.write_bs(0);
        w.write_bs(0);
        w.write_bd(90.0);
        w.write_bd(90.0);
        w.write_rc(17); // > 16
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("ACAD_MLINESTYLE")));
    }
}
