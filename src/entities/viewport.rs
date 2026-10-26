//! VIEWPORT entity (§20.4.38, type 34) — a floating viewport window in
//! paper space showing a clipped view of model space.
//!
//! # Stream shape (§20.4.38)
//!
//! ```text
//! 3BD  center                          10
//! BD   width                           40
//! BD   height                          41
//! R2000+:
//!   3BD  view target                   17
//!   3BD  view direction                16
//!   BD   view twist angle              51
//!   BD   view height                   45
//!   BD   lens length                   42
//!   BD   front clip z                  43
//!   BD   back clip z                   44
//!   BD   snap angle                    50
//!   2RD  view center                   12
//!   2RD  snap base                     13
//!   2RD  snap spacing                  14
//!   2RD  grid spacing                  15
//!   BS   circle zoom                   72
//! R2007+:
//!   BS   grid major                    61
//! R2000+:
//!   BL   frozen layer count
//!   BL   status flags                  90
//!   TV   style sheet                    1
//!   RC   render mode                  281
//!   B    UCS at origin                 74
//!   B    UCS per viewport              71
//!   3BD  UCS origin                   110
//!   3BD  UCS X axis                   111
//!   3BD  UCS Y axis                   112
//!   BD   UCS elevation                146
//!   BS   UCS ortho view type           79
//! R2004+:
//!   BS   shade plot mode              170
//! R2007+:
//!   B    use default lights           292
//!   RC   default lighting type        282
//!   BD   brightness                   141
//!   BD   contrast                     142
//!   CMC  ambient light colour          63
//! ```
//!
//! R13/R14 write only the first three fields. The handles that follow
//! (frozen layers, clip boundary, named and base UCS, and from R2007
//! the background, visual style, shade plot and sun) live in the handle
//! stream and are not read here. On R2007+ the style sheet `TV` and the
//! CMC's optional names come from the record's string stream.
//!
//! # Measured (2026-10-26, #71)
//!
//! Until this change the decoder read only the first three fields, and
//! every R2000+ VIEWPORT the exact-boundary check of #63 saw reported
//! an error (`delta -819` on the six records of `sample_AC1032.dwg`).
//! The whole §20.4.38 list, version branches as printed, closes at
//! `delta 0` on all 51 VIEWPORT records of the public corpus: the six
//! of each of ACadSharp's R14, R2000, R2004, R2007, R2010, R2013 and
//! R2018 saves of one drawing, and the nine of `export_sample.dwg`
//! (R2018, a different drawing). For the 36 records of the R2000 to
//! R2018 saves, every value read also equals the `AcDbViewport` group
//! of AutoCAD's DXF export of the same drawing, matched by handle
//! (1,686 values). The R14 save and `export_sample.dwg` have no DXF
//! export, so their 15 records are measured on the boundary only, and
//! no R13 file exists to measure. ARCHITECTURE.md §7k has the table;
//! `examples/probe_viewport.rs` reproduces it.

use crate::bitcursor::BitCursor;
use crate::entities::{Point2D, Point3D, Vec3D, read_bd3, read_rd2};
use crate::error::Result;
use crate::string_stream::StringReader;
use crate::version::Version;

/// A decoded VIEWPORT entity (§20.4.38).
#[derive(Debug, Clone, PartialEq)]
pub struct Viewport {
    /// `3BD` centre of the viewport in paper space (DXF 10).
    pub center: Point3D,
    /// `BD` width in paper-space units (DXF 40).
    pub width: f64,
    /// `BD` height in paper-space units (DXF 41).
    pub height: f64,
    /// The R2000+ view block; `None` on R13/R14, which stop after the
    /// height.
    pub view: Option<ViewportView>,
}

/// The R2000+ part of a VIEWPORT record (§20.4.38).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportView {
    /// `3BD` view target (DXF 17).
    pub view_target: Point3D,
    /// `3BD` view direction from the target (DXF 16).
    pub view_direction: Vec3D,
    /// `BD` view twist angle in radians (DXF 51).
    pub view_twist_angle: f64,
    /// `BD` view height in model-space units (DXF 45).
    pub view_height: f64,
    /// `BD` perspective lens length (DXF 42).
    pub lens_length: f64,
    /// `BD` front clip plane offset (DXF 43).
    pub front_clip_z: f64,
    /// `BD` back clip plane offset (DXF 44).
    pub back_clip_z: f64,
    /// `BD` snap angle in radians (DXF 50).
    pub snap_angle: f64,
    /// `2RD` view centre in display coordinates (DXF 12).
    pub view_center: Point2D,
    /// `2RD` snap base point (DXF 13).
    pub snap_base: Point2D,
    /// `2RD` snap spacing (DXF 14).
    pub snap_spacing: Point2D,
    /// `2RD` grid spacing (DXF 15).
    pub grid_spacing: Point2D,
    /// `BS` circle zoom percent (DXF 72).
    pub circle_zoom: i16,
    /// `BS` grid lines per major line (DXF 61); R2007+ only.
    pub grid_major: Option<i16>,
    /// `BL` number of frozen-layer handles in the handle stream.
    pub frozen_layer_count: u32,
    /// `BL` status flags (DXF 90).
    pub status_flags: u32,
    /// `TV` plot style sheet name (DXF 1).
    pub style_sheet: String,
    /// `RC` render mode (DXF 281).
    pub render_mode: u8,
    /// `B` UCS icon at the UCS origin (DXF 74).
    pub ucs_at_origin: bool,
    /// `B` UCS saved with the viewport (DXF 71).
    pub ucs_per_viewport: bool,
    /// `3BD` UCS origin (DXF 110).
    pub ucs_origin: Point3D,
    /// `3BD` UCS X axis (DXF 111).
    pub ucs_x_axis: Vec3D,
    /// `3BD` UCS Y axis (DXF 112).
    pub ucs_y_axis: Vec3D,
    /// `BD` UCS elevation (DXF 146).
    pub ucs_elevation: f64,
    /// `BS` orthographic UCS type (DXF 79).
    pub ucs_ortho_view_type: i16,
    /// `BS` shade plot mode (DXF 170); R2004+ only.
    pub shade_plot_mode: Option<i16>,
    /// The R2007+ lighting block.
    pub lighting: Option<ViewportLighting>,
}

/// The R2007+ lighting fields of a VIEWPORT record (§20.4.38).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportLighting {
    /// `B` default lighting on (DXF 292).
    pub use_default_lights: bool,
    /// `RC` default lighting type (DXF 282).
    pub default_lighting_type: u8,
    /// `BD` brightness (DXF 141).
    pub brightness: f64,
    /// `BD` contrast (DXF 142).
    pub contrast: f64,
    /// `CMC` ambient light colour (DXF 63, and 421 for the true colour).
    pub ambient_light_color: crate::entities::mleader::Cmc,
}

/// Decodes the §20.4.38 VIEWPORT payload that follows the common entity
/// data. `strings` is the record's R2007+ string stream, `None` before
/// R2007, where `TV` is inline.
pub fn decode(
    c: &mut BitCursor<'_>,
    version: Version,
    strings: &mut Option<StringReader<'_>>,
) -> Result<Viewport> {
    let center = read_bd3(c)?;
    let width = c.read_bd()?;
    let height = c.read_bd()?;
    let view = if version == Version::R14 {
        None
    } else {
        Some(decode_view(c, version, strings)?)
    };
    Ok(Viewport {
        center,
        width,
        height,
        view,
    })
}

fn read_tv(
    c: &mut BitCursor<'_>,
    version: Version,
    strings: &mut Option<StringReader<'_>>,
) -> Result<String> {
    crate::tables::modern::read_tv_field(c, version, strings.as_mut())
}

fn decode_view(
    c: &mut BitCursor<'_>,
    version: Version,
    strings: &mut Option<StringReader<'_>>,
) -> Result<ViewportView> {
    let view_target = read_bd3(c)?;
    let view_direction = read_bd3(c)?;
    let view_twist_angle = c.read_bd()?;
    let view_height = c.read_bd()?;
    let lens_length = c.read_bd()?;
    let front_clip_z = c.read_bd()?;
    let back_clip_z = c.read_bd()?;
    let snap_angle = c.read_bd()?;
    let view_center = read_rd2(c)?;
    let snap_base = read_rd2(c)?;
    let snap_spacing = read_rd2(c)?;
    let grid_spacing = read_rd2(c)?;
    let circle_zoom = c.read_bs()?;
    let grid_major = if version.is_r2007_plus() {
        Some(c.read_bs()?)
    } else {
        None
    };
    let frozen_layer_count = c.read_bl_u()?;
    let status_flags = c.read_bl_u()?;
    let style_sheet = read_tv(c, version, strings)?;
    let render_mode = c.read_rc()?;
    let ucs_at_origin = c.read_b()?;
    let ucs_per_viewport = c.read_b()?;
    let ucs_origin = read_bd3(c)?;
    let ucs_x_axis = read_bd3(c)?;
    let ucs_y_axis = read_bd3(c)?;
    let ucs_elevation = c.read_bd()?;
    let ucs_ortho_view_type = c.read_bs()?;
    let shade_plot_mode = if version.is_r2004_plus() {
        Some(c.read_bs()?)
    } else {
        None
    };
    let lighting = if version.is_r2007_plus() {
        let use_default_lights = c.read_b()?;
        let default_lighting_type = c.read_rc()?;
        let brightness = c.read_bd()?;
        let contrast = c.read_bd()?;
        let index = c.read_bs()?;
        let rgb = c.read_bl_u()?;
        let color_byte = c.read_rc()?;
        let name = if color_byte & 1 != 0 {
            read_tv(c, version, strings)?
        } else {
            String::new()
        };
        let book_name = if color_byte & 2 != 0 {
            read_tv(c, version, strings)?
        } else {
            String::new()
        };
        Some(ViewportLighting {
            use_default_lights,
            default_lighting_type,
            brightness,
            contrast,
            ambient_light_color: crate::entities::mleader::Cmc {
                index,
                rgb,
                color_byte,
                name,
                book_name,
            },
        })
    } else {
        None
    };
    Ok(ViewportView {
        view_target,
        view_direction,
        view_twist_angle,
        view_height,
        lens_length,
        front_clip_z,
        back_clip_z,
        snap_angle,
        view_center,
        snap_base,
        snap_spacing,
        grid_spacing,
        circle_zoom,
        grid_major,
        frozen_layer_count,
        status_flags,
        style_sheet,
        render_mode,
        ucs_at_origin,
        ucs_per_viewport,
        ucs_origin,
        ucs_x_axis,
        ucs_y_axis,
        ucs_elevation,
        ucs_ortho_view_type,
        shade_plot_mode,
        lighting,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_viewport_header() {
        let mut w = BitWriter::new();
        w.write_bd(100.0);
        w.write_bd(200.0);
        w.write_bd(0.0);
        w.write_bd(50.0);
        w.write_bd(25.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let v = decode(&mut c, Version::R14, &mut None).unwrap();
        assert_eq!(
            v.center,
            Point3D {
                x: 100.0,
                y: 200.0,
                z: 0.0
            }
        );
        assert_eq!(v.width, 50.0);
        assert_eq!(v.height, 25.0);
    }
}
