//! TOLERANCE entity (§19.4.44) — geometric dimensioning and tolerance
//! feature control frame.
//!
//! TOLERANCE places a GD&T feature control frame (e.g.
//! `⌖ |⌀0.1|A|B|C|`) at a location in a drawing. Under the hood it's
//! essentially a styled text block anchored to a point — the real
//! content is in the `text_string` field, which carries `%%`-escaped
//! tolerance symbols that the renderer unpacks into glyphs.
//!
//! Fixed object type code `0x2E` per ODA spec §5 Table 4.
//!
//! # Stream shape
//!
//! ```text
//! (R2007+) BS  unknown_short
//! BD   height
//! BD   dimgap
//! BD3  insertion_point
//! BD3  x_axis_direction
//! BD3  extrusion
//! TV   text_string
//! H    dimstyle_handle    -- parsed but not dereferenced
//! ```
//!
//! The `dimstyle_handle` points at a DIMSTYLE table entry whose
//! properties (font, color, arrowhead) drive the feature frame's
//! appearance. That lookup is deferred to a later pass over the
//! resolved handle map.

use crate::bitcursor::{BitCursor, Handle};
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::{Error, Result};
use crate::version::Version;

/// Decoded TOLERANCE payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Tolerance {
    /// Spec-reserved short, R2007+ only. Observed to be zero in real
    /// drawings; preserved verbatim to round-trip.
    pub unknown_short: i16,
    pub height: f64,
    pub dimgap: f64,
    pub insertion_point: Point3D,
    pub x_axis_direction: Vec3D,
    pub extrusion: Vec3D,
    pub text_string: String,
    /// Handle to the governing DIMSTYLE table entry. Not dereferenced
    /// at decode time.
    pub dimstyle_handle: Handle,
}

/// Decode a TOLERANCE payload. The cursor must already be positioned
/// past the common entity preamble.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Tolerance> {
    let unknown_short = if version.is_r2007_plus() {
        c.read_bs()?
    } else {
        0
    };
    let height = c.read_bd()?;
    let dimgap = c.read_bd()?;
    let insertion_point = read_bd3(c)?;
    let x_axis_direction = read_bd3(c)?;
    let extrusion = read_bd3(c)?;
    let text_string = read_tv(c, version)?;
    let dimstyle_handle = c.read_handle()?;
    Ok(Tolerance {
        unknown_short,
        height,
        dimgap,
        insertion_point,
        x_axis_direction,
        extrusion,
        text_string,
        dimstyle_handle,
    })
}

/// Read a variable-text (TV) field. R2007+ uses UTF-16LE with length
/// counted in codepoint shorts (excluding NUL). Prior versions use
/// 8-bit MBCS-or-ASCII. Mirrors the helpers in [`crate::entities::text`]
/// and [`crate::tables`] — kept private because the caller rarely
/// needs to reach for a raw TV from outside the decoder.
fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if version.is_r2007_plus() {
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            let lo = c.read_rc()? as u16;
            let hi = c.read_rc()? as u16;
            units.push((hi << 8) | lo);
        }
        if units.last() == Some(&0) {
            units.pop();
        }
        String::from_utf16(&units)
            .map_err(|_| Error::SectionMap("TOLERANCE text is not valid UTF-16".into()))
    } else {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(c.read_rc()?);
        }
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_tolerance_r2000() {
        let mut w = BitWriter::new();
        // No unknown_short on pre-R2007.
        w.write_bd(2.5); // height
        w.write_bd(0.09); // dimgap
        // insertion_point
        w.write_bd(10.0);
        w.write_bd(20.0);
        w.write_bd(0.0);
        // x_axis_direction
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // extrusion
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        // text_string — "{\\Fgdt;j}" style tolerance glyph command, 5 chars
        let text = b"TOL_1";
        w.write_bs_u(text.len() as u16);
        for b in text {
            w.write_rc(*b);
        }
        // dimstyle handle — soft-pointer (code 5), value 0x1A
        w.write_handle(5, 0x1A);

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let t = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(t.unknown_short, 0);
        assert_eq!(t.height, 2.5);
        assert!((t.dimgap - 0.09).abs() < 1e-12);
        assert_eq!(
            t.insertion_point,
            Point3D {
                x: 10.0,
                y: 20.0,
                z: 0.0
            }
        );
        assert_eq!(t.text_string, "TOL_1");
        assert_eq!(t.dimstyle_handle.code, 5);
        assert_eq!(t.dimstyle_handle.value, 0x1A);
    }

    #[test]
    fn roundtrip_tolerance_r2010_includes_unknown_short() {
        let mut w = BitWriter::new();
        w.write_bs(0); // unknown short on R2007+
        w.write_bd(5.0);
        w.write_bd(0.125);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        // Empty text — TV length 0, no body bytes.
        w.write_bs_u(0);
        w.write_handle(5, 0);

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let t = decode(&mut c, Version::R2010).unwrap();
        assert_eq!(t.height, 5.0);
        assert_eq!(t.dimgap, 0.125);
        assert_eq!(t.text_string, "");
        assert_eq!(t.dimstyle_handle.counter, 0);
    }
}
