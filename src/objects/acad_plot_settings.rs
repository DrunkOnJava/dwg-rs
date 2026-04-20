//! ACAD_PLOTSETTINGS object (spec §19.6.6 — L6-14) — per-layout
//! plot/page configuration.
//!
//! PLOTSETTINGS records carry all of the "Page Setup" dialog state:
//! printer config, paper size, origin, rotation, scale, shade-plot
//! mode, stylesheet reference, etc. Each LAYOUT (and the MODEL
//! record) owns a nested PLOTSETTINGS object.
//!
//! # Stream shape
//!
//! ```text
//! TV    page_setup_name
//! TV    printer_config_name
//! BS    flags
//! BD2   paper_lower_left
//! BD2   paper_upper_right
//! BD2   plot_origin
//! BS    paper_units
//! BS    plot_rotation
//! BS    plot_type
//! BD2   window_lower_left
//! BD2   window_upper_right
//! BD    numerator
//! BD    denominator
//! BS    shade_plot_mode
//! BS    shade_plot_resolution_level
//! BS    shade_plot_dpi
//! TV    stylesheet_name
//! B     scale_to_fit
//! H     shade_plot_object_handle
//! ```

use crate::bitcursor::{BitCursor, Handle};
use crate::error::Result;
use crate::tables::read_tv;
use crate::version::Version;

/// Simple 2D point used for paper/window extents and plot origin.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

fn read_bd2(c: &mut BitCursor<'_>) -> Result<Point2D> {
    let x = c.read_bd()?;
    let y = c.read_bd()?;
    Ok(Point2D { x, y })
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcadPlotSettings {
    pub page_setup_name: String,
    pub printer_config_name: String,
    pub flags: i16,
    pub paper_lower_left: Point2D,
    pub paper_upper_right: Point2D,
    pub plot_origin: Point2D,
    pub paper_units: i16,
    pub plot_rotation: i16,
    pub plot_type: i16,
    pub window_lower_left: Point2D,
    pub window_upper_right: Point2D,
    pub numerator: f64,
    pub denominator: f64,
    pub shade_plot_mode: i16,
    pub shade_plot_resolution_level: i16,
    pub shade_plot_dpi: i16,
    pub stylesheet_name: String,
    pub scale_to_fit: bool,
    pub shade_plot_object_handle: Handle,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AcadPlotSettings> {
    let page_setup_name = read_tv(c, version)?;
    let printer_config_name = read_tv(c, version)?;
    let flags = c.read_bs()?;
    let paper_lower_left = read_bd2(c)?;
    let paper_upper_right = read_bd2(c)?;
    let plot_origin = read_bd2(c)?;
    let paper_units = c.read_bs()?;
    let plot_rotation = c.read_bs()?;
    let plot_type = c.read_bs()?;
    let window_lower_left = read_bd2(c)?;
    let window_upper_right = read_bd2(c)?;
    let numerator = c.read_bd()?;
    let denominator = c.read_bd()?;
    let shade_plot_mode = c.read_bs()?;
    let shade_plot_resolution_level = c.read_bs()?;
    let shade_plot_dpi = c.read_bs()?;
    let stylesheet_name = read_tv(c, version)?;
    let scale_to_fit = c.read_b()?;
    let shade_plot_object_handle = c.read_handle()?;
    Ok(AcadPlotSettings {
        page_setup_name,
        printer_config_name,
        flags,
        paper_lower_left,
        paper_upper_right,
        plot_origin,
        paper_units,
        plot_rotation,
        plot_type,
        window_lower_left,
        window_upper_right,
        numerator,
        denominator,
        shade_plot_mode,
        shade_plot_resolution_level,
        shade_plot_dpi,
        stylesheet_name,
        scale_to_fit,
        shade_plot_object_handle,
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

    #[test]
    fn roundtrip_letter_landscape_plot_settings() {
        let mut w = BitWriter::new();
        encode_tv_r2000(&mut w, b"Letter Landscape");
        encode_tv_r2000(&mut w, b"None_Device");
        w.write_bs(0x10); // flags (scale to fit set upstream — flag bit TBD)
        write_bd2(&mut w, Point2D { x: 6.35, y: 6.35 });
        write_bd2(
            &mut w,
            Point2D {
                x: 273.05,
                y: 203.2,
            },
        );
        write_bd2(&mut w, Point2D { x: 0.0, y: 0.0 });
        w.write_bs(0); // paper_units (0 = inches, 1 = mm, 2 = pixels)
        w.write_bs(1); // plot_rotation (1 = 90°)
        w.write_bs(0); // plot_type (0 = display, 1 = extents, ...)
        write_bd2(&mut w, Point2D { x: 0.0, y: 0.0 });
        write_bd2(&mut w, Point2D { x: 10.0, y: 10.0 });
        w.write_bd(1.0); // numerator
        w.write_bd(1.0); // denominator
        w.write_bs(0); // shade_plot_mode
        w.write_bs(2); // shade_plot_resolution_level
        w.write_bs(300); // shade_plot_dpi
        encode_tv_r2000(&mut w, b"acad.ctb");
        w.write_b(true); // scale_to_fit
        w.write_handle(5, 0x99);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(s.page_setup_name, "Letter Landscape");
        assert_eq!(s.printer_config_name, "None_Device");
        assert_eq!(s.paper_lower_left.x, 6.35);
        assert_eq!(s.paper_upper_right.y, 203.2);
        assert_eq!(s.plot_rotation, 1);
        assert!((s.numerator / s.denominator - 1.0).abs() < 1e-12);
        assert_eq!(s.shade_plot_dpi, 300);
        assert_eq!(s.stylesheet_name, "acad.ctb");
        assert!(s.scale_to_fit);
        assert_eq!(s.shade_plot_object_handle.value, 0x99);
    }
}
