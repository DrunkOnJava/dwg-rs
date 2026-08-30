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
use crate::tables::{TableEntryHeader, modern, read_table_entry_header};
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

/// Decodes a `ViewEntry` table entry that follows the common object header.
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

/// Decode an R2007+ VIEW whose name lives in the object's string stream
/// (ODA v5.4.1 §19.1 split layout, §20.4.7 VIEW field table).
///
/// Data stream after the common object prefix:
///
/// ```text
/// B     64-flag
/// B     xref dependent
/// BS    xref index + 1
/// BD    view height
/// BD    view width
/// 2RD   view center
/// BD3   target
/// BD3   view direction
/// BD    twist angle
/// BD    lens length
/// BD    front clip
/// BD    back clip
/// 4BITS view mode
/// RC    render mode
/// B     use default lights          (R2007+)
/// RC    default lighting type       (R2007+)
/// BD    brightness                  (R2007+)
/// BD    contrast                    (R2007+)
/// CMC   ambient colour              (R2007+, full BS/BL/RC form)
/// B     paper-space flag
/// B     associated UCS
///   if set: BD3 origin, BD3 x-direction, BD3 y-direction,
///           BD elevation, BS orthographic view type
/// B     camera plottable            (R2007+)
/// ```
///
/// Only the fields already surfaced by [`ViewEntry`] are kept; the
/// R2007+ lighting block and the associated-UCS block are consumed so
/// the body lands on the string stream, which
/// [`modern::SplitStream::finish`] then verifies.
///
/// Reconstructed from the `view_custom` record of `sample_AC1032.dwg`
/// (the only VIEW in the sample corpus): view height 106.9159, width
/// 234.1224, centre (65.6976, 15.9301), target (0,0,0), direction
/// (0,0,1), lens length 50.0, ambient colour RGB 0x333333, identity
/// associated UCS.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<ViewEntry> {
    let mut split = modern::open_table_entry(payload, object_body_start, version)?;
    let (flag64, xref_index_plus_1, is_xref_dependent) = modern::read_entry_flags(&mut split.data)?;
    let c = &mut split.data;
    let view_height = c.read_bd()?;
    let view_width = c.read_bd()?;
    let view_center = Point2D {
        x: c.read_rd()?,
        y: c.read_rd()?,
    };
    let target = read_bd3(c)?;
    let view_direction = read_bd3(c)?;
    let twist_angle = c.read_bd()?;
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
    let is_paperspace = c.read_b()?;
    let is_associated_ucs = c.read_b()?;
    if is_associated_ucs {
        let _origin = read_bd3(c)?;
        let _x_direction = read_bd3(c)?;
        let _y_direction = read_bd3(c)?;
        let _elevation = c.read_bd()?;
        let _ortho_view_type = c.read_bs()?;
    }
    let _camera_plottable = c.read_b()?;
    split.finish("VIEW")?;
    let name = split.strings.read_tv()?;
    Ok(ViewEntry {
        header: TableEntryHeader {
            name,
            is_xref_dependent,
            xref_index_plus_1,
            is_xref_resolved: flag64,
        },
        view_height,
        view_width,
        view_center,
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

    /// Write the R2007+ VIEW body the decoder expects, then read it back.
    #[test]
    fn r2007_split_stream_view_reads_name_from_string_stream() {
        let mut body = BitWriter::new();
        body.write_bs_u(0); // no EED
        body.write_b(true); // no xdictionary
        body.write_b(false); // no binary data
        body.write_b(false); // 64-flag
        body.write_b(false); // xref dependent
        body.write_bs(0); // xref index + 1
        body.write_bd(106.5); // view height
        body.write_bd(234.25); // view width
        body.write_rd(65.5); // centre x
        body.write_rd(15.5); // centre y
        body.write_bd(0.0); // target
        body.write_bd(0.0);
        body.write_bd(0.0);
        body.write_bd(0.0); // direction
        body.write_bd(0.0);
        body.write_bd(1.0);
        body.write_bd(0.0); // twist
        body.write_bd(50.0); // lens length
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
        body.write_b(false); // paper space
        body.write_b(true); // associated UCS
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
        body.write_bs(0); // ortho view type
        body.write_b(false); // camera plottable
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["view_custom"]);
        let v = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(v.header.name, "view_custom");
        assert_eq!(v.view_height, 106.5);
        assert_eq!(v.view_width, 234.25);
        assert_eq!(v.view_center, Point2D { x: 65.5, y: 15.5 });
        assert_eq!(v.lens_length, 50.0);
        assert_eq!(v.view_mode, 1);
        assert_eq!(v.render_mode, 0);
        assert!(!v.is_paperspace);
        assert!(v.is_associated_ucs);
        assert_eq!(
            v.view_direction,
            Point3D {
                x: 0.0,
                y: 0.0,
                z: 1.0
            }
        );
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
