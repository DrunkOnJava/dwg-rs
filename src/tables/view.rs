//! VIEW table entry (ODA Open Design Specification v5.4.1 §19.5.7,
//! L6-07) — named saved 3D camera setup.
//!
//! # Scope — rendering-essential subset
//!
//! VIEW is one of the richest symbol-table entries (§19.5.7 lists ~25
//! fields, with ~10 gated on R2007+ perspective/visual-style work). For
//! the initial read pipeline we implement the fields a 2D/3D viewer or
//! round-trip tool needs to reproduce the camera and its clipping:
//!
//! | Slot  | Field              | Type |
//! |-------|--------------------|------|
//! | 1     | view_height        | BD   |
//! | 2     | view_width         | BD   |
//! | 3,4   | view_center        | BD × 2 (2D screen-space) |
//! | 5..7  | target             | BD3  |
//! | 8..10 | view_direction     | BD3  |
//! | 11    | twist_angle        | BD (radians) |
//! | 12    | lens_length        | BD (mm for 35mm equivalent) |
//! | 13    | front_clip         | BD   |
//! | 14    | back_clip          | BD   |
//! | 15    | view_mode          | BS (bit flags) |
//! | 16    | render_mode        | RC (0..=6 visual style) |
//! | 17    | is_paperspace      | B    |
//! | 18    | is_associated_ucs  | B    |
//!
//! Fields past slot 18 (visual style handle, camera object handle,
//! shade plot handle, etc) are deferred — they are version-gated and
//! carry handles whose resolution needs the object map. A richer
//! decoder can layer on top of this one by continuing from the cursor
//! position where [`decode`] leaves it.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Point3D, read_bd3};
use crate::error::Result;
use crate::tables::{TableEntryHeader, read_table_entry_header};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct ViewEntry {
    pub header: TableEntryHeader,
    pub view_height: f64,
    pub view_width: f64,
    pub view_center: Point2D,
    pub target: Point3D,
    pub view_direction: Point3D,
    pub twist_angle: f64,
    pub lens_length: f64,
    pub front_clip: f64,
    pub back_clip: f64,
    pub view_mode: i16,
    pub render_mode: u8,
    pub is_paperspace: bool,
    pub is_associated_ucs: bool,
}

// Legacy alias retained so callers keep compiling while they migrate to
// [`ViewEntry`].
pub type View = ViewEntry;

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<ViewEntry> {
    let header = read_table_entry_header(c, version)?;
    let view_height = c.read_bd()?;
    let view_width = c.read_bd()?;
    let vcx = c.read_bd()?;
    let vcy = c.read_bd()?;
    let target = read_bd3(c)?;
    let view_direction = read_bd3(c)?;
    let twist_angle = c.read_bd()?;
    let lens_length = c.read_bd()?;
    let front_clip = c.read_bd()?;
    let back_clip = c.read_bd()?;
    let view_mode = c.read_bs()?;
    let render_mode = c.read_rc()?;
    let is_paperspace = c.read_b()?;
    let is_associated_ucs = c.read_b()?;
    Ok(ViewEntry {
        header,
        view_height,
        view_width,
        view_center: Point2D { x: vcx, y: vcy },
        target,
        view_direction,
        twist_angle,
        lens_length,
        front_clip,
        back_clip,
        view_mode,
        render_mode,
        is_paperspace,
        is_associated_ucs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn write_header(w: &mut BitWriter, name: &[u8]) {
        w.write_bs_u(name.len() as u16);
        for b in name {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
    }

    #[test]
    fn roundtrip_isometric_view() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"SW-Iso");
        w.write_bd(10.0); // height
        w.write_bd(20.0); // width
        w.write_bd(5.0); // cx
        w.write_bd(5.0); // cy
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(0.0); // target
        w.write_bd(-1.0);
        w.write_bd(-1.0);
        w.write_bd(1.0); // view_direction
        w.write_bd(0.0); // twist
        w.write_bd(50.0); // lens
        w.write_bd(0.0); // front
        w.write_bd(0.0); // back
        w.write_bs(0x0001); // perspective disabled, back clip off
        w.write_rc(0x04); // render mode — gouraud
        w.write_b(false); // not paperspace
        w.write_b(true); // associated with UCS
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let v = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(v.header.name, "SW-Iso");
        assert_eq!(v.view_width, 20.0);
        assert_eq!(v.lens_length, 50.0);
        assert_eq!(v.view_mode, 1);
        assert_eq!(v.render_mode, 0x04);
        assert!(!v.is_paperspace);
        assert!(v.is_associated_ucs);
        assert_eq!(
            v.view_direction,
            Point3D {
                x: -1.0,
                y: -1.0,
                z: 1.0
            }
        );
    }
}
