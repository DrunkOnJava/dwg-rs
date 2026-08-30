//! PLOTSETTINGS — the "Page Setup" block: printer, paper size,
//! margins, plot origin/window, print scale and shade-plot mode.
//!
//! # Where the spec puts this field list
//!
//! The ODA *Open Design Specification for .dwg files* v5.4.1 lists
//! `PLOTSETTINGS` in §20.3's table of non-fixed object types, but §20.4
//! carries **no separate prescription** for it — the object chapter
//! runs `20.4.1 Common Entity Data` … `20.4.104 XRECORD` and has no
//! PLOTSETTINGS entry. (An earlier revision of this module cited
//! "§19.6.6 (L6-14)"; the spec has no §19.6 chapter at all.)
//!
//! The field list *is* prescribed, once: **§20.4.84 LAYOUT** opens with
//! the whole plot-settings block — every field below carries the
//! literal `plotsettings …` gloss in that table — before LAYOUT's own
//! fields begin. That is the list this module implements, and
//! [`crate::objects::acad_layout`] reads it through
//! [`read_fields`] so the two can never drift apart.
//!
//! # The wire shape — §20.4.84, measured on 31 records
//!
//! ```text
//! TV   page_setup_name             (1)
//! TV   printer_config_name         (2)
//! BS   plot_layout_flags           (70)
//! BD   margin_left                 (40)   millimetres
//! BD   margin_bottom               (41)   millimetres
//! BD   margin_right                (42)   millimetres
//! BD   margin_top                  (43)   millimetres
//! BD   paper_width                 (44)   millimetres
//! BD   paper_height                (45)   millimetres
//! TV   paper_size                  (4)
//! 2BD  plot_origin                 (46,47)
//! BS   paper_units                 (72)
//! BS   plot_rotation               (73)
//! BS   plot_type                   (74)
//! 2BD  window_min                  (48,49)
//! 2BD  window_max                  (140,141)
//! BD   real_world_units            (142)  custom-scale numerator
//! BD   drawing_units               (143)  custom-scale denominator
//! TV   current_style_sheet         (7)
//! BS   standard_scale_type         (75)
//! BD   scale_factor                (147)
//! 2BD  paper_image_origin          (148,149)
//! BS   shade_plot_mode             (76)   -- R2004+
//! BS   shade_plot_resolution_level (77)   -- R2004+
//! BS   shade_plot_custom_dpi       (78)   -- R2004+
//! ```
//!
//! On R2007+ the five `TV` slots consume no data-stream bits — their
//! characters live in the object's string stream
//! ([`crate::string_stream`]) — which is why every corpus record shows
//! exactly five string-stream entries, three of them from this block:
//! `["", "PDFill PDF&Image Writer", "", "", "Layout1"]` on
//! `arc_2013.dwg`, `["", "none_device", "ANSI_A_(8.50_x_11.00_Inches)",
//! "", "Model"]` on `sample_AC1032.dwg`.
//!
//! # Corroboration from the decoded values
//!
//! The evidence that this ordering is the right one is in
//! [`crate::objects::acad_layout`], which measures every record end-to-
//! end against its data-stream boundary. The values this block
//! contributes:
//!
//! - `paper_width` / `paper_height` decode to `215.9` × `279.4` on the
//!   record whose `paper_size` string reads
//!   `ANSI_A_(8.50_x_11.00_Inches)` — 8.5 × 11 inches in millimetres —
//!   and to `210` × `297` on the one that reads `A4`;
//! - all four margins decode to `6.35` (0.25 in) on the `none_device`
//!   records and to `≈4.23` on the HP LaserJet record;
//! - `real_world_units / drawing_units` and `scale_factor` agree:
//!   `1 / 2.5848954647` and `0.38686283977` on the `Model` record of
//!   `sample_AC1032.dwg`, `1 / 1` and `1` on its paper layouts.
//!
//! # Versions
//!
//! | Release | Status |
//! |---|---|
//! | R2004, R2010, R2013, R2018 | closes on all 31 corpus records |
//! | R13 / R14 / R2000 | §20.4.84 inserts a `T plot_view_name` (6) here and omits the three shade-plot fields; no walkable corpus file, so [`Error::Unsupported`] |
//! | R2007 | field list is presumably the R2004 one, but this crate cannot locate an R2007 object's string stream yet (`STATUS.md` #104), so [`Error::Unsupported`] |

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern::{self, ObjectStream};
use crate::version::Version;

/// Simple 2D point used for plot origin, window extents and image origin.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point2D {
    /// X ordinate.
    pub x: f64,
    /// Y ordinate.
    pub y: f64,
}

pub(crate) fn read_bd2(c: &mut BitCursor<'_>) -> Result<Point2D> {
    let x = c.read_bd()?;
    let y = c.read_bd()?;
    Ok(Point2D { x, y })
}

/// The plot-settings block of §20.4.84 — "Page Setup" state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AcadPlotSettings {
    /// Page-setup name (group 1). Empty on every corpus record.
    pub page_setup_name: String,
    /// Printer or plot-configuration file name (group 2), e.g.
    /// `none_device`, `PDFill PDF&Image Writer`.
    pub printer_config_name: String,
    /// Plot layout flag word (group 70).
    pub plot_layout_flags: i16,
    /// Left printable margin in millimetres (group 40).
    pub margin_left: f64,
    /// Bottom printable margin in millimetres (group 41).
    pub margin_bottom: f64,
    /// Right printable margin in millimetres (group 42).
    pub margin_right: f64,
    /// Top printable margin in millimetres (group 43).
    pub margin_top: f64,
    /// Paper width in millimetres (group 44).
    pub paper_width: f64,
    /// Paper height in millimetres (group 45).
    pub paper_height: f64,
    /// Paper-size name (group 4), e.g. `A4`,
    /// `ANSI_A_(8.50_x_11.00_Inches)`.
    pub paper_size: String,
    /// Plot origin offset in millimetres (groups 46, 47).
    pub plot_origin: Point2D,
    /// Plot paper units (group 72).
    pub paper_units: i16,
    /// Plot rotation (group 73).
    pub plot_rotation: i16,
    /// Plot type (group 74).
    pub plot_type: i16,
    /// Plot-window lower-left corner (groups 48, 49).
    pub window_min: Point2D,
    /// Plot-window upper-right corner (groups 140, 141).
    pub window_max: Point2D,
    /// Numerator of the custom print scale (group 142).
    pub real_world_units: f64,
    /// Denominator of the custom print scale (group 143).
    pub drawing_units: f64,
    /// Current plot-style-table name (group 7).
    pub current_style_sheet: String,
    /// Standard scale type (group 75).
    pub standard_scale_type: i16,
    /// Scale factor (group 147); equals
    /// `real_world_units / drawing_units` on every corpus record.
    pub scale_factor: f64,
    /// Paper image origin (groups 148, 149).
    pub paper_image_origin: Point2D,
    /// Shade-plot mode (group 76). R2004+.
    pub shade_plot_mode: i16,
    /// Shade-plot resolution level (group 77). R2004+.
    pub shade_plot_resolution_level: i16,
    /// Shade-plot custom DPI (group 78). R2004+; `300` on every corpus
    /// record.
    pub shade_plot_custom_dpi: i16,
}

impl AcadPlotSettings {
    /// Printable width — paper width less the left and right margins,
    /// in millimetres.
    pub fn printable_width(&self) -> f64 {
        self.paper_width - self.margin_left - self.margin_right
    }

    /// Printable height — paper height less the top and bottom
    /// margins, in millimetres.
    pub fn printable_height(&self) -> f64 {
        self.paper_height - self.margin_top - self.margin_bottom
    }

    /// The custom print scale as a ratio, or `None` when
    /// `drawing_units` is zero.
    pub fn custom_scale(&self) -> Option<f64> {
        if self.drawing_units == 0.0 {
            None
        } else {
            Some(self.real_world_units / self.drawing_units)
        }
    }
}

/// Reject the release bands whose plot-settings layout this crate has
/// not matched against real bytes.
pub(crate) fn check_version(version: Version, what: &str) -> Result<()> {
    match version {
        Version::R2004 | Version::R2010 | Version::R2013 | Version::R2018 => Ok(()),
        Version::R2007 => Err(Error::Unsupported {
            feature: format!(
                "{what} on R2007: this crate cannot locate an R2007 object's string stream yet"
            ),
        }),
        other => Err(Error::Unsupported {
            feature: format!(
                "{what} is only determined for R2004 and R2010+; got {}",
                other.release()
            ),
        }),
    }
}

/// Read the §20.4.84 plot-settings block from an already-opened object
/// stream, leaving the cursor on the first field that follows it.
///
/// This is the shared half of PLOTSETTINGS and LAYOUT: a LAYOUT record
/// embeds exactly this block ahead of its own fields.
pub(crate) fn read_fields(
    split: &mut ObjectStream<'_>,
    version: Version,
) -> Result<AcadPlotSettings> {
    let page_setup_name = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let printer_config_name = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let c = &mut split.data;
    let plot_layout_flags = c.read_bs()?;
    let margin_left = c.read_bd()?;
    let margin_bottom = c.read_bd()?;
    let margin_right = c.read_bd()?;
    let margin_top = c.read_bd()?;
    let paper_width = c.read_bd()?;
    let paper_height = c.read_bd()?;
    let paper_size = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let c = &mut split.data;
    let plot_origin = read_bd2(c)?;
    let paper_units = c.read_bs()?;
    let plot_rotation = c.read_bs()?;
    let plot_type = c.read_bs()?;
    let window_min = read_bd2(c)?;
    let window_max = read_bd2(c)?;
    let real_world_units = c.read_bd()?;
    let drawing_units = c.read_bd()?;
    let current_style_sheet = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let c = &mut split.data;
    let standard_scale_type = c.read_bs()?;
    let scale_factor = c.read_bd()?;
    let paper_image_origin = read_bd2(c)?;
    let shade_plot_mode = c.read_bs()?;
    let shade_plot_resolution_level = c.read_bs()?;
    let shade_plot_custom_dpi = c.read_bs()?;
    Ok(AcadPlotSettings {
        page_setup_name,
        printer_config_name,
        plot_layout_flags,
        margin_left,
        margin_bottom,
        margin_right,
        margin_top,
        paper_width,
        paper_height,
        paper_size,
        plot_origin,
        paper_units,
        plot_rotation,
        plot_type,
        window_min,
        window_max,
        real_world_units,
        drawing_units,
        current_style_sheet,
        standard_scale_type,
        scale_factor,
        paper_image_origin,
        shade_plot_mode,
        shade_plot_resolution_level,
        shade_plot_custom_dpi,
    })
}

/// Decode a standalone PLOTSETTINGS object from its raw payload,
/// taking its `TV` fields from the R2007+ string stream and checking
/// that the data fields end exactly on the data-stream boundary.
///
/// Returns [`Error::Unsupported`] for the release bands listed in the
/// module docs.
pub fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadPlotSettings> {
    check_version(version, "PLOTSETTINGS")?;
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let settings = read_fields(&mut split, version)?;
    split.finish("PLOTSETTINGS")?;
    Ok(settings)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Write the §20.4.84 plot-settings block with `TV` slots left to
    /// the string stream, as an R2007+ record does.
    pub(crate) fn write_split_stream_block(w: &mut BitWriter) {
        w.write_bs(1712); // plot_layout_flags
        w.write_bd(6.35); // margin_left
        w.write_bd(19.05); // margin_bottom
        w.write_bd(6.35); // margin_right
        w.write_bd(19.05); // margin_top
        w.write_bd(215.9); // paper_width
        w.write_bd(279.4); // paper_height
        w.write_bd(0.0); // plot_origin.x
        w.write_bd(0.0); // plot_origin.y
        w.write_bs(0); // paper_units
        w.write_bs(1); // plot_rotation
        w.write_bs(5); // plot_type
        w.write_bd(0.0); // window_min.x
        w.write_bd(0.0); // window_min.y
        w.write_bd(0.0); // window_max.x
        w.write_bd(0.0); // window_max.y
        w.write_bd(1.0); // real_world_units
        w.write_bd(1.0); // drawing_units
        w.write_bs(16); // standard_scale_type
        w.write_bd(1.0); // scale_factor
        w.write_bd(0.0); // paper_image_origin.x
        w.write_bd(0.0); // paper_image_origin.y
        w.write_bs(0); // shade_plot_mode
        w.write_bs(2); // shade_plot_resolution_level
        w.write_bs(300); // shade_plot_custom_dpi
    }

    fn build_r2018() -> Vec<u8> {
        let mut body = modern::tests::r2018_object_prefix(1);
        write_split_stream_block(&mut body);
        let bits = crate::string_stream::tests::bits_of(&body);
        crate::string_stream::tests::build_payload(
            &bits,
            &[
                "",
                "none_device",
                "ANSI_A_(8.50_x_11.00_Inches)",
                "acad.ctb",
            ],
        )
    }

    #[test]
    fn r2018_split_stream_plot_settings_closes_on_its_string_stream() {
        let payload = build_r2018();
        let s = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(s.page_setup_name, "");
        assert_eq!(s.printer_config_name, "none_device");
        assert_eq!(s.paper_size, "ANSI_A_(8.50_x_11.00_Inches)");
        assert_eq!(s.current_style_sheet, "acad.ctb");
        assert_eq!(s.plot_layout_flags, 1712);
        assert!((s.margin_left - 6.35).abs() < 1e-12);
        assert!((s.margin_top - 19.05).abs() < 1e-12);
        assert!((s.paper_width - 215.9).abs() < 1e-12);
        assert!((s.paper_height - 279.4).abs() < 1e-12);
        assert_eq!(s.plot_rotation, 1);
        assert_eq!(s.plot_type, 5);
        assert_eq!(s.standard_scale_type, 16);
        assert_eq!(s.shade_plot_custom_dpi, 300);
        assert!((s.printable_width() - (215.9 - 12.7)).abs() < 1e-12);
        assert!((s.printable_height() - (279.4 - 38.1)).abs() < 1e-12);
        assert_eq!(s.custom_scale(), Some(1.0));
    }

    /// A body one field short of the list must not satisfy the
    /// boundary check — the decoder has to error rather than return a
    /// plausible-looking struct.
    #[test]
    fn a_short_body_is_rejected_by_the_boundary_check() {
        let mut body = modern::tests::r2018_object_prefix(1);
        write_split_stream_block(&mut body);
        body.write_bs(0); // one field too many
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["", "x", "", ""]);
        assert!(decode_object(&payload, 8, None, Version::R2018).is_err());
    }

    #[test]
    fn rejects_the_undetermined_release_bands() {
        let payload = build_r2018();
        for version in [Version::R14, Version::R2000, Version::R2007] {
            let err = decode_object(&payload, 8, None, version).unwrap_err();
            assert!(matches!(&err, Error::Unsupported { feature } if feature.contains("PLOTSETTINGS")));
        }
    }

    #[test]
    fn custom_scale_is_none_when_drawing_units_is_zero() {
        let s = AcadPlotSettings::default();
        assert_eq!(s.custom_scale(), None);
    }
}
