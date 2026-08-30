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
use crate::tables::{TableEntryHeader, modern, read_table_entry_header};
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

/// Decodes a `VportEntry` table entry that follows the common object header.
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

/// Decode an R2007+ VPORT whose name lives in the object's string
/// stream (ODA v5.4.1 §19.1 split layout, §20.4.8 VPORT field table).
///
/// Data stream after the common object prefix:
///
/// ```text
/// B     64-flag
/// B     xref dependent
/// BS    xref index + 1
/// BD    view height
/// BD    view width          (surfaced as `aspect_ratio`, see below)
/// 2RD   view center
/// BD3   view target
/// BD3   view direction
/// BD    view twist
/// BD    lens length
/// BD    front clip
/// BD    back clip
/// 4BITS view mode
/// RC    render mode
/// B     use default lights        (R2007+)
/// RC    default lighting type     (R2007+)
/// BD    brightness                (R2007+)
/// BD    contrast                  (R2007+)
/// CMC   ambient colour            (R2007+, full BS/BL/RC form)
/// 2RD   lower left
/// 2RD   upper right
/// B     UCSFOLLOW
/// BS    circle zoom percent
/// B     fast zoom
/// BB    UCSICON
/// B     grid on
/// 2RD   grid spacing
/// B     snap on
/// B     snap style
/// BS    snap isopair
/// BD    snap rotation
/// 2RD   snap base
/// 2RD   snap spacing
/// B     unknown
/// B     UCS per viewport
/// BD3   ucs origin
/// BD3   ucs x-direction
/// BD3   ucs y-direction
/// BD    ucs elevation
/// BS    ucs orthographic view type
/// BS    grid flags                (R2007+)
/// BS    grid major                (R2007+)
/// ```
///
/// # Reconstructed from bytes
///
/// Verified against the `*Active` VPORT of `sample_AC1032.dwg`
/// (R2018), `line_2013.dwg` (R2013) and `arc_2010.dwg` (R2010). The
/// three files agree bit-for-bit on the flag words: the 23 bits after
/// the screen rectangle read `B 0`, `BS 1000` (the VIEWRES circle-zoom
/// default), `B 1`, `BB 3` (UCSICON on + at origin), `B 1`; the six
/// bits between grid spacing and snap base read `B 0`, `B 0`,
/// `BS 0`, `BD 0.0`; the UCS block reads origin `(0,0,0)`,
/// x-direction `(1,0,0)`, y-direction `(0,1,0)`, elevation `0.0`; and
/// the trailing `BS` values read `0`, `3`, `5` — the last matching the
/// `GRIDMAJOR` default. The
/// second `BD` is documented as an aspect ratio but observes as the
/// view *width*: `line_2013.dwg` reports height 297.0 with that field
/// 603.8576 and centre x 301.9288 = 603.8576 / 2.
///
/// `ucs_at_origin` is taken from bit 1 of `UCSICON`; the UCS block is
/// consumed but not surfaced by [`VportEntry`].
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<VportEntry> {
    let mut split = modern::open_table_entry(payload, object_body_start, version)?;
    let (flag64, xref_index_plus_1, is_xref_dependent) = modern::read_entry_flags(&mut split.data)?;
    let c = &mut split.data;
    let view_height = c.read_bd()?;
    let aspect_ratio = c.read_bd()?;
    let view_center = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let view_target = read_bd3(c)?;
    let view_direction = read_bd3(c)?;
    let view_twist = c.read_bd()?;
    let lens_length = c.read_bd()?;
    let front_clip = c.read_bd()?;
    let back_clip = c.read_bd()?;
    let view_mode = modern::read_4bits(c)? as i16;
    let render_mode = c.read_rc()?;
    let _use_default_lights = c.read_b()?;
    let _default_lighting_type = c.read_rc()?;
    let _brightness = c.read_bd()?;
    let _contrast = c.read_bd()?;
    let _ambient_color = modern::read_cmc_full(c)?;
    let lower_left = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let upper_right = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let _ucs_follow = c.read_b()?;
    let _circle_zoom = c.read_bs()?;
    let _fast_zoom = c.read_b()?;
    let ucs_icon = c.read_bb()?;
    let _grid_on = c.read_b()?;
    let grid_spacing = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let _snap_on = c.read_b()?;
    let _snap_style = c.read_b()?;
    let _snap_isopair = c.read_bs()?;
    let snap_rotation = c.read_bd()?;
    let snap_base = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let snap_spacing = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let _unknown = c.read_b()?;
    let ucs_per_vport = c.read_b()?;
    let _ucs_origin = read_bd3(c)?;
    let _ucs_x_direction = read_bd3(c)?;
    let _ucs_y_direction = read_bd3(c)?;
    let _ucs_elevation = c.read_bd()?;
    let _ucs_ortho_view_type = c.read_bs()?;
    let _grid_flags = c.read_bs()?;
    let _grid_major = c.read_bs()?;
    split.finish("VPORT")?;
    let name = split.strings.read_tv()?;
    Ok(VportEntry {
        header: TableEntryHeader {
            name,
            is_xref_dependent,
            xref_index_plus_1,
            is_xref_resolved: flag64,
        },
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
        ucs_at_origin: ucs_icon & 0x02 != 0,
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

    /// Write the R2007+ VPORT body the decoder expects, then read it back.
    #[test]
    fn r2007_split_stream_vport_reads_name_from_string_stream() {
        let mut body = BitWriter::new();
        body.write_bs_u(0); // no EED
        body.write_b(true); // no xdictionary
        body.write_b(false); // no binary data
        body.write_b(false); // 64-flag
        body.write_b(false); // xref dependent
        body.write_bs(0); // xref index + 1
        body.write_bd(297.0); // view height
        body.write_bd(603.85); // view width
        body.write_rd(301.925); // centre
        body.write_rd(148.5);
        body.write_bd(0.0); // target
        body.write_bd(0.0);
        body.write_bd(0.0);
        body.write_bd(0.0); // direction
        body.write_bd(0.0);
        body.write_bd(1.0);
        body.write_bd(0.0); // twist
        body.write_bd(50.0); // lens
        body.write_bd(0.0); // front clip
        body.write_bd(0.0); // back clip
        for bit in [false, false, false, true] {
            body.write_b(bit); // 4BITS view mode = 1
        }
        body.write_rc(0); // render mode
        body.write_b(true); // use default lights
        body.write_rc(1); // default lighting type
        body.write_bd(0.0); // brightness
        body.write_bd(0.0); // contrast
        body.write_bs(0); // ambient colour index
        body.write_bl(0x00333333); // ambient colour rgb
        body.write_rc(0); // ambient colour byte
        body.write_rd(0.0); // lower left
        body.write_rd(0.0);
        body.write_rd(1.0); // upper right
        body.write_rd(1.0);
        body.write_b(false); // UCSFOLLOW
        body.write_bs(1000); // circle zoom percent
        body.write_b(true); // fast zoom
        body.write_bb(0b11); // UCSICON — on, at origin
        body.write_b(true); // grid on
        body.write_rd(10.0); // grid spacing
        body.write_rd(10.0);
        body.write_b(false); // snap on
        body.write_b(false); // snap style
        body.write_bs(0); // snap isopair
        body.write_bd(0.0); // snap rotation
        body.write_rd(0.0); // snap base
        body.write_rd(0.0);
        body.write_rd(10.0); // snap spacing
        body.write_rd(10.0);
        body.write_b(false); // unknown
        body.write_b(true); // UCS per viewport
        body.write_bd(0.0); // ucs origin
        body.write_bd(0.0);
        body.write_bd(0.0);
        body.write_bd(1.0); // ucs x-direction
        body.write_bd(0.0);
        body.write_bd(0.0);
        body.write_bd(0.0); // ucs y-direction
        body.write_bd(1.0);
        body.write_bd(0.0);
        body.write_bd(0.0); // ucs elevation
        body.write_bs(0); // ucs ortho view type
        body.write_bs(3); // grid flags
        body.write_bs(5); // grid major
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["*Active"]);
        let v = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(v.header.name, "*Active");
        assert_eq!(v.view_height, 297.0);
        assert_eq!(
            v.view_center,
            Point2D {
                x: 301.925,
                y: 148.5
            }
        );
        assert_eq!(v.lower_left, Point2D { x: 0.0, y: 0.0 });
        assert_eq!(v.upper_right, Point2D { x: 1.0, y: 1.0 });
        assert_eq!(v.grid_spacing, Point2D { x: 10.0, y: 10.0 });
        assert_eq!(v.snap_base, Point2D { x: 0.0, y: 0.0 });
        assert_eq!(v.snap_spacing, Point2D { x: 10.0, y: 10.0 });
        assert_eq!(v.lens_length, 50.0);
        assert!(v.ucs_at_origin);
        assert!(v.ucs_per_vport);
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
