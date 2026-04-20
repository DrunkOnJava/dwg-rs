//! SVG writer — string-based emission from the rendering pipeline.
//!
//! Consumes [`crate::curve::Curve`] / [`crate::curve::Path`] and writes
//! SVG 1.1 path/circle/ellipse/line elements. Pure text emission — no
//! external SVG crate dependency (the standard-lib `format!` is
//! sufficient + deterministic).
//!
//! # Output
//!
//! Each call to [`SvgDoc::push_curve`] / [`SvgDoc::push_path`] appends
//! one SVG element, optionally inside a named layer group. Finalize
//! with [`SvgDoc::finish`] to get the complete document as a `String`.
//!
//! ```
//! use dwg::svg::{SvgDoc, Style};
//! use dwg::curve::Curve;
//! use dwg::entities::Point3D;
//!
//! let mut doc = SvgDoc::new(800.0, 600.0);
//! let style = Style { stroke: "#FF0000".to_string(), stroke_width: 1.0, fill: None };
//! doc.push_curve(
//!     &Curve::Line {
//!         a: Point3D::new(0.0, 0.0, 0.0),
//!         b: Point3D::new(100.0, 100.0, 0.0),
//!     },
//!     &style,
//!     None,
//! );
//! let svg = doc.finish();
//! assert!(svg.contains("<svg"));
//! assert!(svg.contains("<line"));
//! ```
//!
//! # Coordinate system
//!
//! SVG's Y axis points DOWN, CAD's Y axis points UP. This writer
//! preserves CAD coordinates verbatim and applies the flip at the
//! root `<svg>` transform (`transform="scale(1,-1) translate(0,-H)"`).
//! Downstream renderers can override by passing a custom viewBox.

use crate::curve::{Curve, Path, PolylineVertex};
use crate::entities::Point3D;

/// A render-time style for an SVG element.
#[derive(Debug, Clone)]
pub struct Style {
    /// Stroke color as SVG/CSS color string (`#RRGGBB`, `red`, …).
    pub stroke: String,
    /// Stroke width in CAD units (pre-transform).
    pub stroke_width: f64,
    /// Optional fill color. `None` → `fill="none"`.
    pub fill: Option<String>,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            stroke: "#000000".to_string(),
            stroke_width: 1.0,
            fill: None,
        }
    }
}

/// SVG document in progress. Elements are appended with the `push_*`
/// methods and the complete document is produced by [`finish`].
#[derive(Debug, Clone)]
pub struct SvgDoc {
    width: f64,
    height: f64,
    body: String,
    current_layer: Option<String>,
}

impl SvgDoc {
    /// Start a new document with the given canvas size in CAD units.
    pub fn new(width: f64, height: f64) -> Self {
        SvgDoc {
            width,
            height,
            body: String::new(),
            current_layer: None,
        }
    }

    /// Begin a named layer group. All subsequent elements go into this
    /// group until [`end_layer`] is called or a new layer begins.
    pub fn begin_layer(&mut self, name: &str) {
        if self.current_layer.is_some() {
            self.end_layer();
        }
        // Escape the name minimally for SVG attribute safety.
        let safe = svg_escape_attr(name);
        self.body
            .push_str(&format!("  <g inkscape:label=\"{safe}\" data-layer=\"{safe}\">\n"));
        self.current_layer = Some(name.to_string());
    }

    /// Close the current layer group.
    pub fn end_layer(&mut self) {
        if self.current_layer.is_some() {
            self.body.push_str("  </g>\n");
            self.current_layer = None;
        }
    }

    /// Append one curve with the given style. Optional `data_handle`
    /// is emitted as a `data-handle` attribute for downstream tooling.
    pub fn push_curve(&mut self, curve: &Curve, style: &Style, data_handle: Option<&str>) {
        let indent = if self.current_layer.is_some() { "    " } else { "  " };
        let handle_attr = data_handle
            .map(|h| format!(" data-handle=\"{}\"", svg_escape_attr(h)))
            .unwrap_or_default();
        let style_attr = style.to_attrs();
        match curve {
            Curve::Line { a, b } => {
                self.body.push_str(&format!(
                    "{indent}<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{style_attr}{handle_attr}/>\n",
                    a.x, a.y, b.x, b.y
                ));
            }
            Curve::Circle {
                center, radius, ..
            } => {
                self.body.push_str(&format!(
                    "{indent}<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{style_attr}{handle_attr}/>\n",
                    center.x, center.y, radius
                ));
            }
            Curve::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ..
            } => {
                let p0 = polar_point(*center, *radius, *start_angle);
                let p1 = polar_point(*center, *radius, *end_angle);
                let large_arc = if (end_angle - start_angle).abs() > std::f64::consts::PI {
                    1
                } else {
                    0
                };
                let sweep = if end_angle > start_angle { 1 } else { 0 };
                self.body.push_str(&format!(
                    "{indent}<path d=\"M {} {} A {} {} 0 {} {} {} {}\"{style_attr}{handle_attr}/>\n",
                    p0.x, p0.y, radius, radius, large_arc, sweep, p1.x, p1.y
                ));
            }
            Curve::Ellipse {
                center,
                major_axis,
                ratio,
                ..
            } => {
                let major_len =
                    (major_axis.x.powi(2) + major_axis.y.powi(2) + major_axis.z.powi(2)).sqrt();
                let minor_len = major_len * ratio;
                let angle_deg = major_axis.y.atan2(major_axis.x).to_degrees();
                self.body.push_str(&format!(
                    "{indent}<ellipse cx=\"{}\" cy=\"{}\" rx=\"{major_len}\" ry=\"{minor_len}\" transform=\"rotate({angle_deg} {} {})\"{style_attr}{handle_attr}/>\n",
                    center.x, center.y, center.x, center.y
                ));
            }
            Curve::Polyline { vertices, closed } => {
                let d = polyline_path_d(vertices, *closed);
                self.body.push_str(&format!(
                    "{indent}<path d=\"{d}\"{style_attr}{handle_attr}/>\n"
                ));
            }
            Curve::Spline(_) | Curve::Helix { .. } => {
                // Tessellation is the renderer's responsibility —
                // emit a placeholder comment for now. Production
                // renderer should sample the curve + emit polyline.
                self.body.push_str(&format!(
                    "{indent}<!-- spline/helix: tessellate before emit -->\n"
                ));
            }
        }
    }

    /// Append a compound path (multiple segments) as a single SVG
    /// `<path>` element with one `d=` attribute.
    pub fn push_path(&mut self, path: &Path, style: &Style, data_handle: Option<&str>) {
        let indent = if self.current_layer.is_some() { "    " } else { "  " };
        let handle_attr = data_handle
            .map(|h| format!(" data-handle=\"{}\"", svg_escape_attr(h)))
            .unwrap_or_default();
        let mut d = String::new();
        let mut moved = false;
        for seg in &path.segments {
            match seg {
                Curve::Line { a, b } => {
                    if !moved {
                        d.push_str(&format!("M {} {} ", a.x, a.y));
                        moved = true;
                    }
                    d.push_str(&format!("L {} {} ", b.x, b.y));
                }
                _ => {
                    // For non-line segments, break and emit single-segment
                    // paths. (A future rev could emit arc / bezier commands
                    // inline.)
                    self.push_curve(seg, style, data_handle);
                }
            }
        }
        if path.closed && !d.is_empty() {
            d.push('Z');
        }
        if !d.is_empty() {
            let style_attr = style.to_attrs();
            self.body.push_str(&format!(
                "{indent}<path d=\"{d}\"{style_attr}{handle_attr}/>\n"
            ));
        }
    }

    /// Finalize the document and return the complete SVG string.
    /// Closes any open layer group.
    pub fn finish(mut self) -> String {
        if self.current_layer.is_some() {
            self.end_layer();
        }
        let w = self.width;
        let h = self.height;
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" \
             xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" \
             width=\"{w}\" height=\"{h}\" \
             viewBox=\"0 0 {w} {h}\">\n"
        ));
        // CAD Y-up → SVG Y-down flip.
        out.push_str(&format!(
            "  <g transform=\"translate(0,{h}) scale(1,-1)\">\n"
        ));
        out.push_str(&self.body);
        out.push_str("  </g>\n");
        out.push_str("</svg>\n");
        out
    }
}

impl Style {
    fn to_attrs(&self) -> String {
        let fill = self
            .fill
            .as_ref()
            .map(|f| format!(" fill=\"{f}\""))
            .unwrap_or_else(|| " fill=\"none\"".to_string());
        format!(
            " stroke=\"{}\" stroke-width=\"{}\"{fill}",
            self.stroke, self.stroke_width
        )
    }
}

/// Compute a point on a circle for SVG arc emission.
fn polar_point(center: Point3D, radius: f64, angle: f64) -> Point3D {
    Point3D {
        x: center.x + radius * angle.cos(),
        y: center.y + radius * angle.sin(),
        z: center.z,
    }
}

/// Build an SVG `d=` attribute for a polyline (optionally closed).
/// Bulges are NOT honored — straight segments only; bulged segments
/// are expected to be pre-expanded to arcs by the caller.
fn polyline_path_d(vertices: &[PolylineVertex], closed: bool) -> String {
    let mut d = String::new();
    for (i, v) in vertices.iter().enumerate() {
        if i == 0 {
            d.push_str(&format!("M {} {} ", v.point.x, v.point.y));
        } else {
            d.push_str(&format!("L {} {} ", v.point.x, v.point.y));
        }
    }
    if closed {
        d.push('Z');
    }
    d
}

/// Minimal SVG attribute escaping. Handles the 5 characters that
/// break XML attributes; does NOT HTML-encode Unicode.
fn svg_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::Vec3D;

    #[test]
    fn empty_doc_has_svg_root() {
        let doc = SvgDoc::new(100.0, 200.0);
        let s = doc.finish();
        assert!(s.contains("<?xml"));
        assert!(s.contains("<svg"));
        assert!(s.contains("width=\"100\""));
        assert!(s.contains("height=\"200\""));
        assert!(s.contains("viewBox=\"0 0 100 200\""));
        assert!(s.contains("</svg>"));
    }

    #[test]
    fn line_emits_svg_line_element() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        let curve = Curve::Line {
            a: Point3D::new(1.0, 2.0, 0.0),
            b: Point3D::new(3.0, 4.0, 0.0),
        };
        doc.push_curve(&curve, &style, None);
        let s = doc.finish();
        assert!(s.contains("<line"));
        assert!(s.contains("x1=\"1\""));
        assert!(s.contains("y2=\"4\""));
        assert!(s.contains("stroke=\"#000000\""));
        assert!(s.contains("fill=\"none\""));
    }

    #[test]
    fn circle_emits_svg_circle_element() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style {
            stroke: "#FF0000".to_string(),
            stroke_width: 2.0,
            fill: Some("#FFFFFF".to_string()),
        };
        let curve = Curve::Circle {
            center: Point3D::new(50.0, 50.0, 0.0),
            radius: 10.0,
            normal: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        };
        doc.push_curve(&curve, &style, None);
        let s = doc.finish();
        assert!(s.contains("<circle"));
        assert!(s.contains("cx=\"50\""));
        assert!(s.contains("cy=\"50\""));
        assert!(s.contains("r=\"10\""));
        assert!(s.contains("stroke=\"#FF0000\""));
        assert!(s.contains("fill=\"#FFFFFF\""));
    }

    #[test]
    fn layer_wraps_contents_in_g() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        doc.begin_layer("Layer 1");
        let style = Style::default();
        doc.push_curve(
            &Curve::Line {
                a: Point3D::new(0.0, 0.0, 0.0),
                b: Point3D::new(1.0, 1.0, 0.0),
            },
            &style,
            None,
        );
        doc.end_layer();
        let s = doc.finish();
        assert!(s.contains("<g inkscape:label=\"Layer 1\""));
        assert!(s.contains("data-layer=\"Layer 1\""));
        assert!(s.matches("</g>").count() >= 2); // layer g + root flip g
    }

    #[test]
    fn data_handle_emitted_as_attribute() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        doc.push_curve(
            &Curve::Line {
                a: Point3D::new(0.0, 0.0, 0.0),
                b: Point3D::new(1.0, 1.0, 0.0),
            },
            &style,
            Some("0x83"),
        );
        let s = doc.finish();
        assert!(s.contains("data-handle=\"0x83\""));
    }

    #[test]
    fn attribute_escape_handles_special_chars() {
        assert_eq!(svg_escape_attr("foo & <bar>"), "foo &amp; &lt;bar&gt;");
        assert_eq!(svg_escape_attr("x\"y"), "x&quot;y");
        assert_eq!(svg_escape_attr("it's"), "it&apos;s");
    }

    #[test]
    fn path_emits_m_l_z_commands() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        let pts = [
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(10.0, 0.0, 0.0),
            Point3D::new(10.0, 10.0, 0.0),
        ];
        let p = Path::from_polyline(&pts, true);
        doc.push_path(&p, &style, None);
        let s = doc.finish();
        assert!(s.contains("<path"));
        assert!(s.contains("M 0 0"));
        assert!(s.contains("L 10 0"));
        assert!(s.contains("Z"));
    }

    #[test]
    fn flip_transform_in_root_group() {
        let doc = SvgDoc::new(100.0, 200.0);
        let s = doc.finish();
        // CAD Y-up → SVG Y-down
        assert!(s.contains("translate(0,200)"));
        assert!(s.contains("scale(1,-1)"));
    }

    #[test]
    fn multiple_elements_append_in_order() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let s1 = Style {
            stroke: "#FF0000".to_string(),
            ..Default::default()
        };
        let s2 = Style {
            stroke: "#00FF00".to_string(),
            ..Default::default()
        };
        doc.push_curve(
            &Curve::Line {
                a: Point3D::new(0.0, 0.0, 0.0),
                b: Point3D::new(1.0, 0.0, 0.0),
            },
            &s1,
            None,
        );
        doc.push_curve(
            &Curve::Line {
                a: Point3D::new(0.0, 0.0, 0.0),
                b: Point3D::new(0.0, 1.0, 0.0),
            },
            &s2,
            None,
        );
        let out = doc.finish();
        let first = out.find("#FF0000").unwrap();
        let second = out.find("#00FF00").unwrap();
        assert!(first < second, "elements must appear in push order");
    }
}
