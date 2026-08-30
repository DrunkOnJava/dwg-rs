//! LAYOUT (ODA spec v5.4.1 §20.4.84, type code `0x52`) — a paper-space
//! or model-space tab: its embedded plot settings, limits, extents and
//! UCS.
//!
//! Every AutoCAD drawing carries one `Model` layout and zero or more
//! paper-space layouts (`Layout1`, `Layout2`, …). The block record a
//! layout owns holds the geometry; the LAYOUT object holds the
//! metadata — tab order, limits, extents, UCS, and the whole
//! plot-settings block.
//!
//! # Provenance of the field list
//!
//! §20.4.84 is the one place the spec prescribes both halves: the
//! record opens with the plot-settings block (each row glossed
//! `plotsettings …`; see [`crate::objects::acad_plot_settings`]) and
//! then continues with LAYOUT's own fields. The module previously
//! cited "§19.6.12 (L6-12)" — the spec has no §19.6 chapter — and its
//! field list omitted the six margin/paper `BD`s, the paper-size and
//! plot-view `TV`s and the shade-plot triple, so it could not close on
//! any real record. That list is withdrawn.
//!
//! ```text
//! <plot-settings block — see acad_plot_settings>
//! TV   layout_name            (1)
//! BL   tab_order              (71)
//! BS   flags                  (70)
//! 3BD  ucs_origin             (13)
//! 2RD  limmin                 (10)
//! 2RD  limmax                 (11)
//! 3BD  insertion_point        (12)
//! 3BD  ucs_x_axis             (16)
//! 3BD  ucs_y_axis             (17)
//! BD   elevation              (146)
//! BS   ucs_ortho_view_type    (76)
//! 3BD  extent_min             (14)
//! 3BD  extent_max             (15)
//! BL   viewport_count                -- R2004+; the spec says RL
//! ```
//!
//! The handle references §20.4.84 lists after `viewport_count` — plot
//! view, visual style, paperspace block record, last active viewport,
//! base and named UCS, and one per viewport — live in the object's
//! handle stream, past the data-stream boundary this module checks, so
//! they are not fields of this struct.
//!
//! # `viewport_count` is a `BL`, not the `RL` §20.4.84 prints
//!
//! Measured: on `sample_AC1032.dwg` handle 89 the last field starts 10
//! bits before the string-stream start bit. An `RL` is 32 raw bits and
//! cannot fit; a `BL` in its 8-bit form is exactly 10 bits and decodes
//! `2`. Every other corpus record agrees: a `BL` of `0` in its 2-bit
//! form on the 28 layouts with no viewport, and `2` on the three
//! paper layouts of `sample_AC1032.dwg`. No record closes with an `RL`
//! there — on the 2-bit records it would overrun the boundary by 30
//! bits.
//!
//! # Why the list is right, not merely consistent
//!
//! Each record's data fields must end exactly on the first bit of its
//! string stream (R2010+) or on the `RL` object-data-size (R2004); see
//! [`crate::objects::modern`]. The list above lands **all 31 LAYOUT
//! records of the corpus** on that bit — 9 on the R2004 files, 9 on
//! R2010, 9 on R2013, 4 on `sample_AC1032.dwg` — with delta 0 and no
//! bits to spare.
//!
//! The decoded values corroborate it independently:
//!
//! - `limmin` / `limmax` come out `(0, 0)` … `(12, 9)` on every `Model`
//!   record — AutoCAD's default model limits — and `(420, 297)` on the
//!   A3 `Layout1` of `arc_2004.dwg`;
//! - on `sample_AC1032.dwg` the paper layouts' limits are the sheet
//!   less its margins, in inches: `(-0.25, -0.25) … (10.75, 8.25)` on
//!   the Letter layout whose plot-settings margins are all `6.35` mm,
//!   and `(-0.1667, -0.1667) … (11.526, 8.101)` on the A4 layout whose
//!   margins are `≈4.23` mm;
//! - `ucs_x_axis` decodes `(1, 0, 0)` and `ucs_y_axis` `(0, 1, 0)` on
//!   every one of the 31 records, and `ucs_origin` `(0, 0, 0)`;
//! - `extent_min` / `extent_max` are `+1e20` / `-1e20` on the empty
//!   `Layout1` records of the R2004, R2010 and R2013 files — AutoCAD's
//!   uninitialised-extents sentinel;
//! - `tab_order` is `0` on each `Model` record and `1`, `2`, `3` on the
//!   paper layouts, matching their `layout_name` strings;
//! - the embedded plot-settings block's `paper_width` × `paper_height`
//!   matches its own `paper_size` string on every record that carries
//!   one — `215.9 × 279.4` for `ANSI_A_(8.50_x_11.00_Inches)` and
//!   `Letter_(8.50_x_11.00_Inches)`, `210 × 297` for `A4`.
//!
//! A field list off by a single bit reproduces none of that.
//!
//! # Versions
//!
//! | Release | Status |
//! |---|---|
//! | R2004 | closes on all 9 corpus records (inline `TV`s, `RL` boundary) |
//! | R2010 / R2013 / R2018 | closes on all 22 corpus records (string-stream `TV`s) |
//! | R13 / R14 / R2000 | §20.4.84 inserts `T plot_view_name` and omits shade-plot + `viewport_count`; those files have no walkable object stream in this crate (`STATUS.md` #104), so the branch is undetermined — [`crate::error::Error::Unsupported`] |
//! | R2007 | same field list as R2004 is likely, but the R2007 string stream is not locatable here yet — [`crate::error::Error::Unsupported`] |

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::objects::acad_plot_settings::{self, AcadPlotSettings};
use crate::objects::modern;
use crate::version::Version;

/// 2D point used for the layout's paper-space limits.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point2D {
    /// X ordinate.
    pub x: f64,
    /// Y ordinate.
    pub y: f64,
}

/// 3D point used for the insertion base, extents and UCS vectors.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point3D {
    /// X ordinate.
    pub x: f64,
    /// Y ordinate.
    pub y: f64,
    /// Z ordinate.
    pub z: f64,
}

fn read_rd2(c: &mut BitCursor<'_>) -> Result<Point2D> {
    let x = c.read_rd()?;
    let y = c.read_rd()?;
    Ok(Point2D { x, y })
}

fn read_bd3(c: &mut BitCursor<'_>) -> Result<Point3D> {
    let x = c.read_bd()?;
    let y = c.read_bd()?;
    let z = c.read_bd()?;
    Ok(Point3D { x, y, z })
}

/// A decoded LAYOUT record.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AcadLayout {
    /// The plot-settings block §20.4.84 opens with.
    pub plot_settings: AcadPlotSettings,
    /// Layout name (group 1), e.g. `Model`, `Layout1`.
    pub layout_name: String,
    /// Layout-tab order (group 71); `0` on the `Model` record.
    pub tab_order: i32,
    /// Layout flag word (group 70).
    pub flags: i16,
    /// UCS origin (group 13).
    pub ucs_origin: Point3D,
    /// Minimum paper-space limits (group 10).
    pub limmin: Point2D,
    /// Maximum paper-space limits (group 11).
    pub limmax: Point2D,
    /// Insertion base point (group 12).
    pub insertion_point: Point3D,
    /// UCS X-axis direction (group 16).
    pub ucs_x_axis: Point3D,
    /// UCS Y-axis direction (group 17).
    pub ucs_y_axis: Point3D,
    /// Elevation (group 146).
    pub elevation: f64,
    /// Orthographic view type of the UCS (group 76).
    pub ucs_ortho_view_type: i16,
    /// Minimum drawing extents (group 14).
    pub extent_min: Point3D,
    /// Maximum drawing extents (group 15).
    pub extent_max: Point3D,
    /// Number of viewports this layout owns. R2004+; see the module
    /// docs on why this is a `BL` rather than the `RL` §20.4.84 prints.
    pub viewport_count: i32,
    /// String-stream entries the field list does not place. Empty on
    /// every corpus record; a `TV` costs no data-stream bits, so an
    /// unplaced one leaves no measurable trace and is surfaced rather
    /// than dropped.
    pub trailing_strings: Vec<String>,
}

impl AcadLayout {
    /// True for the special `Model` layout — tab order `0` and the
    /// name `Model`.
    pub fn is_model_space(&self) -> bool {
        self.tab_order == 0 && self.layout_name.eq_ignore_ascii_case("Model")
    }

    /// Limits width, `|limmax.x - limmin.x|`.
    pub fn limits_width(&self) -> f64 {
        (self.limmax.x - self.limmin.x).abs()
    }

    /// Limits height, `|limmax.y - limmin.y|`.
    pub fn limits_height(&self) -> f64 {
        (self.limmax.y - self.limmin.y).abs()
    }

    /// True when the extents carry AutoCAD's uninitialised sentinel —
    /// `extent_min` at `+1e20` and `extent_max` at `-1e20`, the state
    /// of a layout that has never held geometry.
    pub fn has_uninitialised_extents(&self) -> bool {
        self.extent_min.x >= 1e20 && self.extent_max.x <= -1e20
    }

    /// Diagonal of the drawing extents, or `0.0` when they carry the
    /// uninitialised sentinel.
    pub fn extents_diagonal(&self) -> f64 {
        if self.has_uninitialised_extents() {
            return 0.0;
        }
        let dx = self.extent_max.x - self.extent_min.x;
        let dy = self.extent_max.y - self.extent_min.y;
        let dz = self.extent_max.z - self.extent_min.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

/// Decode a LAYOUT record straight from its raw object payload, taking
/// its five `TV` fields from the R2007+ string stream and checking
/// that the data fields end exactly on the data-stream boundary.
///
/// Returns [`crate::error::Error::Unsupported`] for the release bands listed in the
/// module docs.
pub fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadLayout> {
    acad_plot_settings::check_version(version, "LAYOUT")?;
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let plot_settings = acad_plot_settings::read_fields(&mut split, version)?;
    let layout_name = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let c = &mut split.data;
    let tab_order = c.read_bl()?;
    let flags = c.read_bs()?;
    let ucs_origin = read_bd3(c)?;
    let limmin = read_rd2(c)?;
    let limmax = read_rd2(c)?;
    let insertion_point = read_bd3(c)?;
    let ucs_x_axis = read_bd3(c)?;
    let ucs_y_axis = read_bd3(c)?;
    let elevation = c.read_bd()?;
    let ucs_ortho_view_type = c.read_bs()?;
    let extent_min = read_bd3(c)?;
    let extent_max = read_bd3(c)?;
    let viewport_count = c.read_bl()?;

    split.finish("LAYOUT")?;

    let mut trailing_strings = Vec::new();
    if let Some(strings) = split.strings.as_mut() {
        while !strings.is_exhausted() {
            match strings.read_tv() {
                Ok(text) => trailing_strings.push(text),
                Err(_) => break,
            }
        }
    }

    Ok(AcadLayout {
        plot_settings,
        layout_name,
        tab_order,
        flags,
        ucs_origin,
        limmin,
        limmax,
        insertion_point,
        ucs_x_axis,
        ucs_y_axis,
        elevation,
        ucs_ortho_view_type,
        extent_min,
        extent_max,
        viewport_count,
        trailing_strings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::error::Error;
    use crate::objects::acad_plot_settings::tests::write_split_stream_block;

    fn write_bd3(w: &mut BitWriter, p: Point3D) {
        w.write_bd(p.x);
        w.write_bd(p.y);
        w.write_bd(p.z);
    }

    fn write_rd2(w: &mut BitWriter, p: Point2D) {
        w.write_rd(p.x);
        w.write_rd(p.y);
    }

    const X_AXIS: Point3D = Point3D {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    const Y_AXIS: Point3D = Point3D {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };

    /// Build the LAYOUT half of the body, with the `TV` layout name
    /// left to the string stream.
    fn write_layout_block(w: &mut BitWriter, tab_order: i32, limmax: Point2D, viewports: i32) {
        w.write_bl(tab_order);
        w.write_bs(1); // flags
        write_bd3(w, Point3D::default()); // ucs_origin
        write_rd2(w, Point2D::default()); // limmin
        write_rd2(w, limmax);
        write_bd3(w, Point3D::default()); // insertion_point
        write_bd3(w, X_AXIS);
        write_bd3(w, Y_AXIS);
        w.write_bd(0.0); // elevation
        w.write_bs(0); // ucs_ortho_view_type
        write_bd3(w, Point3D::default()); // extent_min
        write_bd3(
            w,
            Point3D {
                x: 3.0,
                y: 4.0,
                z: 0.0,
            },
        ); // extent_max
        w.write_bl(viewports);
    }

    fn build_r2018(name: &str, tab_order: i32, limmax: Point2D, viewports: i32) -> Vec<u8> {
        let mut body = modern::tests::r2018_object_prefix(1);
        write_split_stream_block(&mut body);
        write_layout_block(&mut body, tab_order, limmax, viewports);
        let bits = crate::string_stream::tests::bits_of(&body);
        crate::string_stream::tests::build_payload(
            &bits,
            &[
                "",
                "none_device",
                "ANSI_A_(8.50_x_11.00_Inches)",
                "acad.ctb",
                name,
            ],
        )
    }

    #[test]
    fn r2018_split_stream_layout_closes_on_its_string_stream() {
        let payload = build_r2018("Layout1", 1, Point2D { x: 10.75, y: 8.25 }, 2);
        let l = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(l.layout_name, "Layout1");
        assert_eq!(l.tab_order, 1);
        assert_eq!(l.flags, 1);
        assert_eq!(l.viewport_count, 2);
        assert!(!l.is_model_space());
        assert_eq!(l.ucs_x_axis, X_AXIS);
        assert_eq!(l.ucs_y_axis, Y_AXIS);
        assert!((l.limits_width() - 10.75).abs() < 1e-12);
        assert!((l.limits_height() - 8.25).abs() < 1e-12);
        assert!((l.extents_diagonal() - 5.0).abs() < 1e-12);
        assert!(l.trailing_strings.is_empty());
        // The embedded plot-settings block round-trips too.
        assert_eq!(l.plot_settings.printer_config_name, "none_device");
        assert_eq!(
            l.plot_settings.paper_size,
            "ANSI_A_(8.50_x_11.00_Inches)"
        );
        assert!((l.plot_settings.paper_width - 215.9).abs() < 1e-12);
        assert!((l.plot_settings.paper_height - 279.4).abs() < 1e-12);
        assert!((l.plot_settings.margin_left - 6.35).abs() < 1e-12);
        assert_eq!(l.plot_settings.shade_plot_custom_dpi, 300);
    }

    #[test]
    fn model_layout_is_recognised() {
        let payload = build_r2018("Model", 0, Point2D { x: 12.0, y: 9.0 }, 0);
        let l = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert!(l.is_model_space());
        assert_eq!(l.viewport_count, 0);
        assert!((l.limits_width() - 12.0).abs() < 1e-12);
    }

    /// `viewport_count` is a `BL`: reading the same body with a 32-bit
    /// `RL` there would overrun the string-stream start, so the
    /// boundary check must reject a body that carries an `RL`.
    #[test]
    fn an_rl_sized_viewport_count_does_not_close() {
        let mut body = modern::tests::r2018_object_prefix(1);
        write_split_stream_block(&mut body);
        body.write_bl(1); // tab_order
        body.write_bs(1); // flags
        write_bd3(&mut body, Point3D::default());
        write_rd2(&mut body, Point2D::default());
        write_rd2(&mut body, Point2D { x: 1.0, y: 1.0 });
        write_bd3(&mut body, Point3D::default());
        write_bd3(&mut body, X_AXIS);
        write_bd3(&mut body, Y_AXIS);
        body.write_bd(0.0);
        body.write_bs(0);
        write_bd3(&mut body, Point3D::default());
        write_bd3(&mut body, Point3D::default());
        body.write_rl(2); // an RL viewport count instead of a BL
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(
            &bits,
            &["", "none_device", "A4", "", "Layout1"],
        );
        assert!(decode_object(&payload, 8, None, Version::R2018).is_err());
    }

    /// The plot-settings block is not optional: a body that omits it
    /// leaves the field list short and the boundary check has to
    /// reject it.
    #[test]
    fn a_body_without_the_plot_settings_block_is_rejected() {
        let mut body = modern::tests::r2018_object_prefix(1);
        write_layout_block(&mut body, 0, Point2D::default(), 0);
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload =
            crate::string_stream::tests::build_payload(&bits, &["", "", "", "", "Model"]);
        assert!(decode_object(&payload, 8, None, Version::R2018).is_err());
    }

    #[test]
    fn rejects_the_undetermined_release_bands() {
        let payload = build_r2018("Model", 0, Point2D::default(), 0);
        for version in [Version::R14, Version::R2000, Version::R2007] {
            let err = decode_object(&payload, 8, None, version).unwrap_err();
            assert!(matches!(&err, Error::Unsupported { feature } if feature.contains("LAYOUT")));
        }
    }

    #[test]
    fn uninitialised_extents_report_a_zero_diagonal() {
        let l = AcadLayout {
            extent_min: Point3D {
                x: 1e20,
                y: 1e20,
                z: 1e20,
            },
            extent_max: Point3D {
                x: -1e20,
                y: -1e20,
                z: -1e20,
            },
            ..AcadLayout::default()
        };
        assert!(l.has_uninitialised_extents());
        assert_eq!(l.extents_diagonal(), 0.0);
    }
}
