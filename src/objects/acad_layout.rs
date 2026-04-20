//! ACAD_LAYOUT object (spec §19.6.12 — L6-12) — paper-space layout
//! descriptor with plot settings, UCS, and block-record ownership.
//!
//! Every AutoCAD drawing carries one MODEL layout and zero-or-more
//! paper-space layouts (LAYOUT1, LAYOUT2, ...). Each layout is a
//! separate container for viewports, title blocks, and plot
//! configuration. The model-space block and each paper-space block
//! hold the geometry; the LAYOUT object itself holds the metadata
//! (tab order, limits, extents, UCS, plot-settings reference).
//!
//! # Stream shape (R2000+)
//!
//! ```text
//! BS    flags                  -- plot/shade flags
//! TV    layout_name            -- e.g. "Model", "Layout1"
//! BS    tab_order              -- layout-tab index
//! BD2   paper_min_limits
//! BD2   paper_max_limits
//! BD3   insertion_base_point
//! BD3   min_extents
//! BD3   max_extents
//! BD    elevation
//! BS    ucs_ortho_view_type
//! BD3   ucs_origin
//! BD3   ucs_x_axis
//! BD3   ucs_y_axis
//! H     plot_settings_handle
//! H     block_record_handle
//! H     last_active_viewport_handle
//! H     base_ucs_handle
//! H     named_ucs_handle
//! ```
//!
//! The `block_record_handle` points at the BLOCK_HEADER whose body
//! holds the actual viewport + title-block entities for this layout
//! — paper-space rendering walks that block, not the LAYOUT object
//! itself.
//!
//! # Provenance
//!
//! Field order and types are the clean-room composition of:
//! - the public AutoCAD DXF group-code reference for LAYOUT
//!   (group codes 1, 70, 10/20, 11/21, 12/22/32, 13/23/33, 14/24/34,
//!   15/25/35, 16/26/36, 17/27/37, 76, 146, 330, 331, 332, 333, 345,
//!   346)
//! - the ODA Open Design Specification v5.4.1 bit-stream primitive
//!   mapping (§2: BD/BS/TV/H sizing)
//!
//! No Autodesk SDK source, no ODA SDK source, no decompiled AutoCAD
//! binary was consulted. See `CLEANROOM.md`.

use crate::bitcursor::{BitCursor, Handle};
use crate::error::Result;
use crate::tables::read_tv;
use crate::version::Version;

/// 2D point used for paper-space limits (width/height rectangle).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

/// 3D point used for insertion base, extents, and UCS vectors.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

fn read_bd2(c: &mut BitCursor<'_>) -> Result<Point2D> {
    let x = c.read_bd()?;
    let y = c.read_bd()?;
    Ok(Point2D { x, y })
}

fn read_bd3(c: &mut BitCursor<'_>) -> Result<Point3D> {
    let x = c.read_bd()?;
    let y = c.read_bd()?;
    let z = c.read_bd()?;
    Ok(Point3D { x, y, z })
}

/// Decoded ACAD_LAYOUT object.
#[derive(Debug, Clone, PartialEq)]
pub struct AcadLayout {
    pub flags: i16,
    pub layout_name: String,
    pub tab_order: i16,
    pub paper_min_limits: Point2D,
    pub paper_max_limits: Point2D,
    pub insertion_base_point: Point3D,
    pub min_extents: Point3D,
    pub max_extents: Point3D,
    pub elevation: f64,
    pub ucs_ortho_view_type: i16,
    pub ucs_origin: Point3D,
    pub ucs_x_axis: Point3D,
    pub ucs_y_axis: Point3D,
    pub plot_settings_handle: Handle,
    pub block_record_handle: Handle,
    pub last_active_viewport_handle: Handle,
    pub base_ucs_handle: Handle,
    pub named_ucs_handle: Handle,
}

impl AcadLayout {
    /// True iff this is the special MODEL layout (always tab_order 0
    /// and named "Model"). Paper-space layouts are tab_order >= 1.
    pub fn is_model_space(&self) -> bool {
        self.tab_order == 0 && self.layout_name.eq_ignore_ascii_case("Model")
    }

    /// Paper width (max.x - min.x). Returns the unsigned magnitude so
    /// a malformed layout with swapped min/max still reports a
    /// non-negative width.
    pub fn paper_width(&self) -> f64 {
        (self.paper_max_limits.x - self.paper_min_limits.x).abs()
    }

    /// Paper height (max.y - min.y), unsigned.
    pub fn paper_height(&self) -> f64 {
        (self.paper_max_limits.y - self.paper_min_limits.y).abs()
    }

    /// Diagonal size of drawing extents in model units.
    pub fn extents_diagonal(&self) -> f64 {
        let dx = self.max_extents.x - self.min_extents.x;
        let dy = self.max_extents.y - self.min_extents.y;
        let dz = self.max_extents.z - self.min_extents.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AcadLayout> {
    let flags = c.read_bs()?;
    let layout_name = read_tv(c, version)?;
    let tab_order = c.read_bs()?;
    let paper_min_limits = read_bd2(c)?;
    let paper_max_limits = read_bd2(c)?;
    let insertion_base_point = read_bd3(c)?;
    let min_extents = read_bd3(c)?;
    let max_extents = read_bd3(c)?;
    let elevation = c.read_bd()?;
    let ucs_ortho_view_type = c.read_bs()?;
    let ucs_origin = read_bd3(c)?;
    let ucs_x_axis = read_bd3(c)?;
    let ucs_y_axis = read_bd3(c)?;
    let plot_settings_handle = c.read_handle()?;
    let block_record_handle = c.read_handle()?;
    let last_active_viewport_handle = c.read_handle()?;
    let base_ucs_handle = c.read_handle()?;
    let named_ucs_handle = c.read_handle()?;
    Ok(AcadLayout {
        flags,
        layout_name,
        tab_order,
        paper_min_limits,
        paper_max_limits,
        insertion_base_point,
        min_extents,
        max_extents,
        elevation,
        ucs_ortho_view_type,
        ucs_origin,
        ucs_x_axis,
        ucs_y_axis,
        plot_settings_handle,
        block_record_handle,
        last_active_viewport_handle,
        base_ucs_handle,
        named_ucs_handle,
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

    fn write_bd2(w: &mut BitWriter, p: Point2D) {
        w.write_bd(p.x);
        w.write_bd(p.y);
    }

    fn write_bd3(w: &mut BitWriter, p: Point3D) {
        w.write_bd(p.x);
        w.write_bd(p.y);
        w.write_bd(p.z);
    }

    fn write_layout(
        w: &mut BitWriter,
        name: &[u8],
        tab_order: i16,
        paper: (Point2D, Point2D),
        extents: (Point3D, Point3D),
    ) {
        w.write_bs(0); // flags
        encode_tv_r2000(w, name);
        w.write_bs(tab_order);
        write_bd2(w, paper.0);
        write_bd2(w, paper.1);
        write_bd3(w, Point3D::default()); // insertion_base_point
        write_bd3(w, extents.0);
        write_bd3(w, extents.1);
        w.write_bd(0.0); // elevation
        w.write_bs(0); // ucs_ortho_view_type
        write_bd3(w, Point3D::default()); // ucs_origin
        write_bd3(
            w,
            Point3D {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        );
        write_bd3(
            w,
            Point3D {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
        );
        w.write_handle(5, 0x11);
        w.write_handle(5, 0x12);
        w.write_handle(5, 0x13);
        w.write_handle(5, 0x14);
        w.write_handle(5, 0x15);
    }

    #[test]
    fn roundtrip_model_layout() {
        let mut w = BitWriter::new();
        write_layout(
            &mut w,
            b"Model",
            0,
            (Point2D::default(), Point2D { x: 420.0, y: 297.0 }),
            (
                Point3D {
                    x: -10.0,
                    y: -10.0,
                    z: 0.0,
                },
                Point3D {
                    x: 100.0,
                    y: 50.0,
                    z: 0.0,
                },
            ),
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(l.layout_name, "Model");
        assert_eq!(l.tab_order, 0);
        assert!(l.is_model_space());
        assert!((l.paper_width() - 420.0).abs() < 1e-9);
        assert!((l.paper_height() - 297.0).abs() < 1e-9);
        assert_eq!(l.plot_settings_handle.value, 0x11);
        assert_eq!(l.block_record_handle.value, 0x12);
    }

    #[test]
    fn roundtrip_paper_layout1() {
        let mut w = BitWriter::new();
        write_layout(
            &mut w,
            b"Layout1",
            1,
            (Point2D::default(), Point2D { x: 841.0, y: 594.0 }),
            (
                Point3D::default(),
                Point3D {
                    x: 1.0,
                    y: 1.0,
                    z: 0.0,
                },
            ),
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(l.layout_name, "Layout1");
        assert_eq!(l.tab_order, 1);
        assert!(!l.is_model_space());
    }

    #[test]
    fn extents_diagonal_is_euclidean() {
        let mut w = BitWriter::new();
        write_layout(
            &mut w,
            b"Model",
            0,
            (Point2D::default(), Point2D { x: 1.0, y: 1.0 }),
            (
                Point3D::default(),
                Point3D {
                    x: 3.0,
                    y: 4.0,
                    z: 0.0,
                },
            ),
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert!((l.extents_diagonal() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn paper_size_unsigned_even_when_min_exceeds_max() {
        let mut w = BitWriter::new();
        write_layout(
            &mut w,
            b"Broken",
            2,
            (Point2D { x: 100.0, y: 100.0 }, Point2D::default()),
            (Point3D::default(), Point3D::default()),
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert!(l.paper_width() >= 0.0);
        assert!(l.paper_height() >= 0.0);
    }

    #[test]
    fn model_space_detection_is_case_insensitive() {
        let mut w = BitWriter::new();
        write_layout(
            &mut w,
            b"MODEL",
            0,
            (Point2D::default(), Point2D::default()),
            (Point3D::default(), Point3D::default()),
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert!(l.is_model_space());
    }
}
