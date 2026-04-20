//! VPORT table entry (ODA Open Design Specification v5.4.1 §19.5.8,
//! L6-08) — viewport preset for model-space layout.
//!
//! A VPORT stores the viewport's lower-left/upper-right screen
//! rectangle plus all camera/view parameters. The current active
//! viewport is the one named `*Active`.
//!
//! # Scope — rendering-essential subset
//!
//! Like VIEW (§19.5.7), VPORT carries a long tail of visual-style,
//! snap, and grid fields. This decoder covers the subset a renderer
//! needs to reconstruct the viewport camera + screen rectangle +
//! snap/grid state. The structure mirrors VIEW with additional
//! screen-rectangle, snap, and grid fields.
//!
//! | Slot   | Field              | Type | Notes                       |
//! |--------|--------------------|------|-----------------------------|
//! | 1      | view_height        | BD   |                             |
//! | 2      | aspect_ratio       | BD   |                             |
//! | 3,4    | view_center        | 2×BD |                             |
//! | 5..7   | view_target        | BD3  |                             |
//! | 8..10  | view_direction     | BD3  |                             |
//! | 11     | view_twist         | BD   |                             |
//! | 12     | lens_length        | BD   |                             |
//! | 13     | front_clip         | BD   |                             |
//! | 14     | back_clip          | BD   |                             |
//! | 15     | view_mode          | BS   | perspective/clip bits       |
//! | 16     | render_mode        | RC   | 0..=6 visual style          |
//! | 17     | lower_left         | 2×RD | screen-space (0..1)         |
//! | 18     | upper_right        | 2×RD | screen-space (0..1)         |
//! | 19     | ucs_at_origin      | B    | display UCS icon at origin  |
//! | 20     | ucs_per_vport      | B    |                             |
//! | 21,22  | snap_base          | 2×RD |                             |
//! | 23,24  | snap_spacing       | 2×RD |                             |
//! | 25,26  | grid_spacing       | 2×RD |                             |
//! | 27     | snap_rotation      | BD   |                             |

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Point3D, read_bd3};
use crate::error::Result;
use crate::tables::{TableEntryHeader, read_table_entry_header};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct VportEntry {
    pub header: TableEntryHeader,
    pub view_height: f64,
    pub aspect_ratio: f64,
    pub view_center: Point2D,
    pub view_target: Point3D,
    pub view_direction: Point3D,
    pub view_twist: f64,
    pub lens_length: f64,
    pub front_clip: f64,
    pub back_clip: f64,
    pub view_mode: i16,
    pub render_mode: u8,
    pub lower_left: Point2D,
    pub upper_right: Point2D,
    pub ucs_at_origin: bool,
    pub ucs_per_vport: bool,
    pub snap_base: Point2D,
    pub snap_spacing: Point2D,
    pub grid_spacing: Point2D,
    pub snap_rotation: f64,
}

// Legacy alias retained so callers keep compiling while they migrate to
// [`VportEntry`].
pub type VPort = VportEntry;

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<VportEntry> {
    let header = read_table_entry_header(c, version)?;
    let view_height = c.read_bd()?;
    let aspect_ratio = c.read_bd()?;
    let view_center = Point2D {
        x: c.read_bd()?,
        y: c.read_bd()?,
    };
    let view_target = read_bd3(c)?;
    let view_direction = read_bd3(c)?;
    let view_twist = c.read_bd()?;
    let lens_length = c.read_bd()?;
    let front_clip = c.read_bd()?;
    let back_clip = c.read_bd()?;
    let view_mode = c.read_bs()?;
    let render_mode = c.read_rc()?;
    let lower_left = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let upper_right = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let ucs_at_origin = c.read_b()?;
    let ucs_per_vport = c.read_b()?;
    let snap_base = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let snap_spacing = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let grid_spacing = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let snap_rotation = c.read_bd()?;
    Ok(VportEntry {
        header,
        view_height,
        aspect_ratio,
        view_center,
        view_target,
        view_direction,
        view_twist,
        lens_length,
        front_clip,
        back_clip,
        view_mode,
        render_mode,
        lower_left,
        upper_right,
        ucs_at_origin,
        ucs_per_vport,
        snap_base,
        snap_spacing,
        grid_spacing,
        snap_rotation,
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
    fn roundtrip_active_vport() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"*Active");
        w.write_bd(10.0); // view height
        w.write_bd(1.5); // aspect ratio
        w.write_bd(0.0);
        w.write_bd(0.0); // center
        for _ in 0..3 {
            w.write_bd(0.0);
        } // target
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0); // direction
        w.write_bd(0.0); // twist
        w.write_bd(50.0); // lens
        w.write_bd(0.0);
        w.write_bd(0.0); // clips
        w.write_bs(0); // view mode
        w.write_rc(0x02); // render mode
        w.write_rd(0.0);
        w.write_rd(0.0); // lower left
        w.write_rd(1.0);
        w.write_rd(1.0); // upper right
        w.write_b(true); // ucs at origin
        w.write_b(false); // ucs per vport
        w.write_rd(0.0);
        w.write_rd(0.0); // snap base
        w.write_rd(0.5);
        w.write_rd(0.5); // snap spacing
        w.write_rd(1.0);
        w.write_rd(1.0); // grid spacing
        w.write_bd(0.0); // snap rotation
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let v = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(v.header.name, "*Active");
        assert_eq!(v.aspect_ratio, 1.5);
        assert_eq!(v.upper_right, Point2D { x: 1.0, y: 1.0 });
        assert!(v.ucs_at_origin);
        assert_eq!(v.snap_spacing, Point2D { x: 0.5, y: 0.5 });
        assert_eq!(v.grid_spacing, Point2D { x: 1.0, y: 1.0 });
        assert_eq!(v.render_mode, 0x02);
    }
}
