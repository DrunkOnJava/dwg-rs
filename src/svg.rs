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
//! let style = Style { stroke: "#FF0000".to_string(), stroke_width: 1.0, fill: None, dashes: None };
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
//!
//! # Paper space vs model space (L9-10)
//!
//! A DWG document has one model space plus zero or more paper-space
//! layouts (title-block sheets). [`SvgSpace`] selects which one the
//! document renders; [`SvgDoc::with_space`] sets it at construction time
//! and the root `<g>` gets a `data-layout="LAYOUT_NAME"` attribute in
//! paper mode so downstream tooling can distinguish model-space output
//! from paper-space sheets (title block, drawing frame, viewports).
//!
//! # PDF export (L9-13)
//!
//! [`SvgDoc::to_pdf_via_paged_svg`] emits an SVG shaped for browser-based
//! "Save as PDF" (headless Chromium). First-class PDF generation without
//! an external browser is deferred; see the function's docstring for the
//! headless-chromium workflow and the `L9-13a` follow-up TODO.

use crate::curve::{Curve, Path, PolylineVertex};
use crate::entities::Point3D;
use crate::error::Result;

/// Which DWG space a [`SvgDoc`] is rendering.
///
/// A DWG document always has one model space (`Model`) plus zero or more
/// paper-space layouts; each layout is identified by its name (e.g.
/// `"Layout1"`, `"ISO A4"`). When the document is a paper-space layout,
/// the root `<g>` gets a `data-layout="LAYOUT_NAME"` attribute so
/// downstream tooling can distinguish the two — title blocks, drawing
/// frames, and viewport definitions all live in paper space and must be
/// rendered differently from the underlying model-space geometry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SvgSpace {
    /// Model space — the CAD drawing itself. This is the default.
    #[default]
    Model,
    /// Paper-space layout with the given name (e.g. `"Layout1"`).
    Paper(String),
}

/// A render-time style for an SVG element.
#[derive(Debug, Clone)]
pub struct Style {
    /// Stroke color as SVG/CSS color string (`#RRGGBB`, `red`, …).
    pub stroke: String,
    /// Stroke width in CAD units (pre-transform).
    pub stroke_width: f64,
    /// Optional fill color. `None` → `fill="none"`.
    pub fill: Option<String>,
    /// Optional dash-gap pattern in CAD units. `None` → solid stroke;
    /// `Some(vec![4.0, 2.0])` → 4 units on, 2 units off, repeating.
    /// Maps directly to SVG `stroke-dasharray`.
    pub dashes: Option<Vec<f64>>,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            stroke: "#000000".to_string(),
            stroke_width: 1.0,
            fill: None,
            dashes: None,
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
    /// Which DWG space this document is rendering (model vs paper).
    /// Defaults to [`SvgSpace::Model`]; set with [`Self::with_space`].
    space: SvgSpace,
    /// Hatch pattern `<defs>` blocks, deduplicated by pattern id.
    /// Each entry is the inner SVG string for one `<pattern>` element.
    /// Emitted at the document root by [`SvgDoc::finish`] so patterns
    /// can be referenced by `fill="url(#hatch-NAME)"` from any layer.
    pattern_defs: Vec<String>,
    /// `<clipPath>` `<defs>` blocks, deduplicated by clip-path id.
    /// Each entry is the inner SVG string for one `<clipPath>` element.
    /// Emitted at the document root by [`SvgDoc::finish`] so viewports
    /// can be referenced by `clip-path="url(#clip-N)"` from any layer.
    /// Mirrors the dedupe strategy used for `pattern_defs`.
    clip_paths: Vec<String>,
}

impl SvgDoc {
    /// Start a new document with the given canvas size in CAD units.
    /// The document defaults to [`SvgSpace::Model`]; call [`Self::with_space`]
    /// to render a paper-space layout instead.
    pub fn new(width: f64, height: f64) -> Self {
        SvgDoc {
            width,
            height,
            body: String::new(),
            current_layer: None,
            space: SvgSpace::Model,
            pattern_defs: Vec::new(),
            clip_paths: Vec::new(),
        }
    }

    /// Set which DWG space this document is rendering. When `space` is
    /// [`SvgSpace::Paper`], the root `<g>` that wraps the flipped-Y
    /// content emits an extra `data-layout="LAYOUT_NAME"` attribute so
    /// downstream tooling can tell paper-space sheets from model-space
    /// drawings (they have different coordinate conventions, title
    /// blocks, viewport definitions, etc.).
    ///
    /// Consuming builder — call this immediately after [`Self::new`]:
    ///
    /// ```
    /// use dwg::svg::{SvgDoc, SvgSpace};
    /// let doc = SvgDoc::new(800.0, 600.0).with_space(SvgSpace::Paper("Layout1".into()));
    /// let s = doc.finish();
    /// assert!(s.contains("data-layout=\"Layout1\""));
    /// ```
    pub fn with_space(mut self, space: SvgSpace) -> Self {
        self.space = space;
        self
    }

    /// Begin a named layer group. All subsequent elements go into this
    /// group until [`end_layer`] is called or a new layer begins.
    pub fn begin_layer(&mut self, name: &str) {
        if self.current_layer.is_some() {
            self.end_layer();
        }
        // Escape the name minimally for SVG attribute safety.
        let safe = svg_escape_attr(name);
        self.body.push_str(&format!(
            "  <g inkscape:label=\"{safe}\" data-layer=\"{safe}\">\n"
        ));
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
        let indent = if self.current_layer.is_some() {
            "    "
        } else {
            "  "
        };
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
            Curve::Circle { center, radius, .. } => {
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
            Curve::TextBaseline {
                insertion,
                height,
                rotation,
                content,
                ..
            } => {
                // Glyph rendering is the SVG layer's job; emit a
                // placement-only `<text>` so consumers see content at
                // the right anchor and rotation.
                let safe = svg_escape_attr(content);
                let deg = rotation.to_degrees();
                self.body.push_str(&format!(
                    "{indent}<text x=\"{}\" y=\"{}\" font-size=\"{}\" \
                     transform=\"rotate({deg} {} {})\"{style_attr}{handle_attr}>{safe}</text>\n",
                    insertion.x, insertion.y, height, insertion.x, insertion.y
                ));
            }
        }
    }

    /// Append a compound path (multiple segments) as a single SVG
    /// `<path>` element with one `d=` attribute.
    pub fn push_path(&mut self, path: &Path, style: &Style, data_handle: Option<&str>) {
        let indent = if self.current_layer.is_some() {
            "    "
        } else {
            "  "
        };
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

    /// Append a TEXT entity (L9-05). Emits a single `<text>` element at
    /// `position`, sized by `height` (CAD units, becomes SVG `font-size`),
    /// rotated by `rotation_radians` about its insertion point.
    ///
    /// # Font fallback
    ///
    /// AutoCAD `.shx` shape files are vector-only formats no browser can
    /// render. If `font_family` ends in `.shx` (case-insensitive), the
    /// emitted font-family is `Arial, sans-serif`. Otherwise the supplied
    /// family is used verbatim with `, sans-serif` appended.
    ///
    /// # Coordinate flip
    ///
    /// Because the document's root group flips Y, text emitted in CAD
    /// coordinates would render upside-down. This method counter-flips
    /// inside the text element (`scale(1,-1)` around the anchor) so the
    /// glyphs read correctly while their position remains in CAD space.
    #[allow(clippy::too_many_arguments)]
    pub fn push_text(
        &mut self,
        text: &str,
        position: Point3D,
        height: f64,
        rotation_radians: f64,
        font_family: &str,
        style: &Style,
        data_handle: Option<&str>,
    ) {
        let indent = self.current_indent();
        let handle_attr = data_handle
            .map(|h| format!(" data-handle=\"{}\"", svg_escape_attr(h)))
            .unwrap_or_default();
        let resolved_font = resolve_font_family(font_family);
        let font_attr = format!(" font-family=\"{}\"", svg_escape_attr(&resolved_font));
        let size_attr = format!(" font-size=\"{height}\"");
        let fill = style.fill.clone().unwrap_or_else(|| style.stroke.clone());
        let fill_attr = format!(" fill=\"{}\"", svg_escape_attr(&fill));
        let rotation_deg = rotation_radians.to_degrees();
        // Counter-flip Y so glyphs read right-side-up under the root flip,
        // then apply rotation about the anchor in CAD-space degrees.
        let transform = format!(
            " transform=\"translate({x},{y}) scale(1,-1) rotate({deg}) translate({neg_x},{neg_y})\"",
            x = position.x,
            y = position.y,
            deg = -rotation_deg,
            neg_x = -position.x,
            neg_y = -position.y,
        );
        let escaped = svg_escape_text(text);
        self.body.push_str(&format!(
            "{indent}<text x=\"{x}\" y=\"{y}\"{font_attr}{size_attr}{fill_attr}{transform}{handle_attr}>{escaped}</text>\n",
            x = position.x,
            y = position.y,
        ));
    }

    /// Append an MTEXT entity (L9-06) with inline formatting → `<tspan>`s.
    ///
    /// Recognized AutoCAD MTEXT codes:
    ///
    /// | Code            | Effect                                       |
    /// |-----------------|----------------------------------------------|
    /// | `\P`            | Paragraph break (new `<tspan>` on next line) |
    /// | `\f<name>;`     | Font family override for following spans     |
    /// | `\C<n>;`        | ACI color index (0–255) override             |
    /// | `\H<n>x;`       | Height multiplier (font-size scale)          |
    /// | `\L` … `\l`     | Underline on / off                           |
    /// | `\O` … `\o`     | Overline on / off                            |
    /// | `{` / `}`       | Push / pop the inline-style stack            |
    ///
    /// Unknown codes are emitted as literal text and accompanied by a
    /// `<!-- mtext code: \X -->` diagnostic comment so the original
    /// formatting intent is recoverable from the SVG source.
    #[allow(clippy::too_many_arguments)]
    pub fn push_mtext(
        &mut self,
        mtext: &str,
        position: Point3D,
        height: f64,
        rotation_radians: f64,
        font_family: &str,
        style: &Style,
        data_handle: Option<&str>,
    ) {
        let indent = self.current_indent();
        let handle_attr = data_handle
            .map(|h| format!(" data-handle=\"{}\"", svg_escape_attr(h)))
            .unwrap_or_default();
        let base_font = resolve_font_family(font_family);
        let base_fill = style.fill.clone().unwrap_or_else(|| style.stroke.clone());
        let rotation_deg = rotation_radians.to_degrees();
        let transform = format!(
            " transform=\"translate({x},{y}) scale(1,-1) rotate({deg}) translate({neg_x},{neg_y})\"",
            x = position.x,
            y = position.y,
            deg = -rotation_deg,
            neg_x = -position.x,
            neg_y = -position.y,
        );
        // Open the <text> element. Per-tspan styling carries the inline
        // overrides; the outer element only carries the base attributes.
        self.body.push_str(&format!(
            "{indent}<text x=\"{x}\" y=\"{y}\" font-family=\"{font_attr}\" font-size=\"{height}\" fill=\"{fill}\"{transform}{handle_attr}>",
            x = position.x,
            y = position.y,
            font_attr = svg_escape_attr(&base_font),
            fill = svg_escape_attr(&base_fill),
        ));
        // Style stack — top of stack is the active style. `{` pushes a
        // copy; `}` pops back to the previous frame.
        let mut stack: Vec<MTextStyle> = vec![MTextStyle {
            font: base_font.clone(),
            fill: base_fill.clone(),
            height_scale: 1.0,
            underline: false,
            overline: false,
        }];
        let mut buf = String::new();
        let mut diag = String::new();
        let mut first_line = true;
        let chars: Vec<char> = mtext.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' && i + 1 < chars.len() {
                let next = chars[i + 1];
                match next {
                    'P' => {
                        Self::flush_mtext_buf(
                            &mut self.body,
                            &mut buf,
                            stack.last().unwrap(),
                            first_line,
                            position.x,
                            height,
                        );
                        first_line = false;
                        i += 2;
                        continue;
                    }
                    'f' | 'F' => {
                        if let Some((value, consumed)) = parse_mtext_arg(&chars, i + 2) {
                            Self::flush_mtext_buf(
                                &mut self.body,
                                &mut buf,
                                stack.last().unwrap(),
                                first_line,
                                position.x,
                                height,
                            );
                            first_line = false;
                            // For \f, only the first sub-token (before the
                            // first '|') is the font name; AutoCAD packs
                            // bold/italic flags after, which we ignore.
                            let font_name = value.split('|').next().unwrap_or("").to_string();
                            if let Some(top) = stack.last_mut() {
                                top.font = resolve_font_family(&font_name);
                            }
                            i += 2 + consumed;
                            continue;
                        }
                    }
                    'C' => {
                        if let Some((value, consumed)) = parse_mtext_arg(&chars, i + 2)
                            && let Ok(idx) = value.parse::<u32>()
                        {
                            Self::flush_mtext_buf(
                                &mut self.body,
                                &mut buf,
                                stack.last().unwrap(),
                                first_line,
                                position.x,
                                height,
                            );
                            first_line = false;
                            if let Some(top) = stack.last_mut() {
                                top.fill = aci_to_hex(idx);
                            }
                            i += 2 + consumed;
                            continue;
                        }
                    }
                    'H' => {
                        if let Some((value, consumed)) = parse_mtext_arg(&chars, i + 2) {
                            // Strip trailing 'x' (multiplier marker).
                            let trimmed = value.trim_end_matches('x').trim_end_matches('X');
                            if let Ok(scale) = trimmed.parse::<f64>() {
                                Self::flush_mtext_buf(
                                    &mut self.body,
                                    &mut buf,
                                    stack.last().unwrap(),
                                    first_line,
                                    position.x,
                                    height,
                                );
                                first_line = false;
                                if let Some(top) = stack.last_mut() {
                                    top.height_scale = scale;
                                }
                                i += 2 + consumed;
                                continue;
                            }
                        }
                    }
                    'L' => {
                        Self::flush_mtext_buf(
                            &mut self.body,
                            &mut buf,
                            stack.last().unwrap(),
                            first_line,
                            position.x,
                            height,
                        );
                        first_line = false;
                        if let Some(top) = stack.last_mut() {
                            top.underline = true;
                        }
                        i += 2;
                        continue;
                    }
                    'l' => {
                        Self::flush_mtext_buf(
                            &mut self.body,
                            &mut buf,
                            stack.last().unwrap(),
                            first_line,
                            position.x,
                            height,
                        );
                        first_line = false;
                        if let Some(top) = stack.last_mut() {
                            top.underline = false;
                        }
                        i += 2;
                        continue;
                    }
                    'O' => {
                        Self::flush_mtext_buf(
                            &mut self.body,
                            &mut buf,
                            stack.last().unwrap(),
                            first_line,
                            position.x,
                            height,
                        );
                        first_line = false;
                        if let Some(top) = stack.last_mut() {
                            top.overline = true;
                        }
                        i += 2;
                        continue;
                    }
                    'o' => {
                        Self::flush_mtext_buf(
                            &mut self.body,
                            &mut buf,
                            stack.last().unwrap(),
                            first_line,
                            position.x,
                            height,
                        );
                        first_line = false;
                        if let Some(top) = stack.last_mut() {
                            top.overline = false;
                        }
                        i += 2;
                        continue;
                    }
                    '\\' => {
                        // Escaped backslash → literal `\`.
                        buf.push('\\');
                        i += 2;
                        continue;
                    }
                    other => {
                        // Unknown code: record diag, skip the marker, and
                        // pass through the rest as literal text.
                        diag.push_str(&format!("\\{other} "));
                        i += 2;
                        continue;
                    }
                }
            }
            if c == '{' {
                Self::flush_mtext_buf(
                    &mut self.body,
                    &mut buf,
                    stack.last().unwrap(),
                    first_line,
                    position.x,
                    height,
                );
                first_line = false;
                let top = stack.last().cloned().unwrap();
                stack.push(top);
                i += 1;
                continue;
            }
            if c == '}' {
                Self::flush_mtext_buf(
                    &mut self.body,
                    &mut buf,
                    stack.last().unwrap(),
                    first_line,
                    position.x,
                    height,
                );
                first_line = false;
                if stack.len() > 1 {
                    stack.pop();
                }
                i += 1;
                continue;
            }
            buf.push(c);
            i += 1;
        }
        // Flush trailing buffer.
        Self::flush_mtext_buf(
            &mut self.body,
            &mut buf,
            stack.last().unwrap(),
            first_line,
            position.x,
            height,
        );
        self.body.push_str("</text>\n");
        if !diag.is_empty() {
            self.body.push_str(&format!(
                "{indent}<!-- mtext code: {} -->\n",
                diag.trim_end()
            ));
        }
    }

    /// Append a SOLID hatch boundary (L9-07) — a closed `<path>` filled
    /// with `fill_color` and no stroke. The boundary's existing shape
    /// is rendered as one path with `Z` closure on each loop.
    pub fn push_hatch_solid(
        &mut self,
        boundary: &Path,
        fill_color: &str,
        data_handle: Option<&str>,
    ) {
        let indent = self.current_indent();
        let handle_attr = data_handle
            .map(|h| format!(" data-handle=\"{}\"", svg_escape_attr(h)))
            .unwrap_or_default();
        let d = boundary_path_d(boundary);
        if d.is_empty() {
            self.body
                .push_str(&format!("{indent}<!-- hatch-solid: empty boundary -->\n"));
            return;
        }
        self.body.push_str(&format!(
            "{indent}<path d=\"{d}\" fill=\"{fill}\" fill-rule=\"evenodd\" stroke=\"none\"{handle_attr}/>\n",
            fill = svg_escape_attr(fill_color),
        ));
    }

    /// Append a PATTERN hatch (L9-07). For `SOLID`, delegates to
    /// [`Self::push_hatch_solid`]. For `ANSI31` and `ANGLE`, registers
    /// a one-time `<pattern>` definition (via [`Self::register_pattern`])
    /// and emits a `<path>` referencing it through `fill="url(#…)"`.
    /// All other names emit a placeholder comment plus a solid fill so
    /// the boundary is at least visible to a downstream renderer.
    pub fn push_hatch_pattern(
        &mut self,
        boundary: &Path,
        pattern_name: &str,
        _scale: f64,
        _angle_radians: f64,
        fill_color: &str,
        data_handle: Option<&str>,
    ) {
        let upper = pattern_name.to_ascii_uppercase();
        if upper == "SOLID" {
            self.push_hatch_solid(boundary, fill_color, data_handle);
            return;
        }
        let indent = self.current_indent();
        let handle_attr = data_handle
            .map(|h| format!(" data-handle=\"{}\"", svg_escape_attr(h)))
            .unwrap_or_default();
        let d = boundary_path_d(boundary);
        if d.is_empty() {
            self.body.push_str(&format!(
                "{indent}<!-- hatch-pattern {pattern_name}: empty boundary -->\n"
            ));
            return;
        }
        let pattern_id = match upper.as_str() {
            "ANSI31" => {
                self.register_pattern(
                    "hatch-ANSI31",
                    &format!(
                        "<pattern id=\"hatch-ANSI31\" patternUnits=\"userSpaceOnUse\" \
                         width=\"8\" height=\"8\" patternTransform=\"rotate(45)\">\
                         <line x1=\"0\" y1=\"0\" x2=\"0\" y2=\"8\" \
                         stroke=\"{stroke}\" stroke-width=\"0.5\"/></pattern>",
                        stroke = svg_escape_attr(fill_color),
                    ),
                );
                "hatch-ANSI31".to_string()
            }
            "ANGLE" => {
                let angle_deg = _angle_radians.to_degrees();
                let id = "hatch-ANGLE".to_string();
                self.register_pattern(
                    &id,
                    &format!(
                        "<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" \
                         width=\"8\" height=\"8\" patternTransform=\"rotate({angle_deg})\">\
                         <line x1=\"0\" y1=\"0\" x2=\"0\" y2=\"8\" \
                         stroke=\"{stroke}\" stroke-width=\"0.5\"/></pattern>",
                        stroke = svg_escape_attr(fill_color),
                    ),
                );
                id
            }
            _ => {
                // Unknown pattern: record the original name in a comment
                // and fall through to solid fill so the boundary is still
                // visible. A future revision may add more patterns.
                self.body.push_str(&format!(
                    "{indent}<!-- hatch-pattern unsupported: {} -->\n",
                    svg_escape_attr(pattern_name)
                ));
                self.body.push_str(&format!(
                    "{indent}<path d=\"{d}\" fill=\"{fill}\" fill-rule=\"evenodd\" stroke=\"none\"{handle_attr}/>\n",
                    fill = svg_escape_attr(fill_color),
                ));
                return;
            }
        };
        self.body.push_str(&format!(
            "{indent}<path d=\"{d}\" fill=\"url(#{pattern_id})\" fill-rule=\"evenodd\" stroke=\"none\"{handle_attr}/>\n",
        ));
    }

    /// Append a linear DIMENSION (L9-08). Emits two extension lines
    /// (perpendicular projection of `p1` / `p2` onto `dimension_line`),
    /// the dimension line itself, two filled-triangle arrowheads, and
    /// the dimension text centered on the dimension line.
    ///
    /// Geometry: the dimension line passes through `dimension_line`,
    /// parallel to `p2 - p1`. The visible dim line spans the two feet
    /// of perpendicular from the measured points to that line. Compute
    /// the perpendicular offset as `(d - p1) - ((d - p1) · u) * u` and
    /// project both endpoints by adding it. Arrowhead size scales with
    /// `text_height * 0.5`.
    #[allow(clippy::too_many_arguments)]
    pub fn push_dimension_linear(
        &mut self,
        p1: Point3D,
        p2: Point3D,
        dimension_line: Point3D,
        text: &str,
        font_family: &str,
        text_height: f64,
        style: &Style,
    ) {
        let indent = self.current_indent();
        let style_attr = style.to_attrs();
        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < f64::EPSILON {
            self.body.push_str(&format!(
                "{indent}<!-- dimension-linear: zero-length baseline -->\n"
            ));
            return;
        }
        let ux = dx / len;
        let uy = dy / len;
        // Perpendicular offset from the baseline to the dim line.
        let rx = dimension_line.x - p1.x;
        let ry = dimension_line.y - p1.y;
        let along = rx * ux + ry * uy;
        let perp_x = rx - along * ux;
        let perp_y = ry - along * uy;
        let f1 = Point3D::new(p1.x + perp_x, p1.y + perp_y, 0.0);
        let f2 = Point3D::new(p2.x + perp_x, p2.y + perp_y, 0.0);
        // Extension lines.
        self.body.push_str(&format!(
            "{indent}<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{style_attr}/>\n",
            p1.x, p1.y, f1.x, f1.y
        ));
        self.body.push_str(&format!(
            "{indent}<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{style_attr}/>\n",
            p2.x, p2.y, f2.x, f2.y
        ));
        // Dimension line connecting the two feet.
        self.body.push_str(&format!(
            "{indent}<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{style_attr}/>\n",
            f1.x, f1.y, f2.x, f2.y
        ));
        // Arrowheads — small filled triangles at each foot.
        let head = text_height * 0.5;
        let half = head * 0.3;
        let nx = -uy;
        let ny = ux;
        let tip1 = f1;
        let base1a = Point3D::new(
            f1.x + head * ux + half * nx,
            f1.y + head * uy + half * ny,
            0.0,
        );
        let base1b = Point3D::new(
            f1.x + head * ux - half * nx,
            f1.y + head * uy - half * ny,
            0.0,
        );
        self.body.push_str(&format!(
            "{indent}<path d=\"M {} {} L {} {} L {} {} Z\" fill=\"{stroke}\" stroke=\"none\"/>\n",
            tip1.x,
            tip1.y,
            base1a.x,
            base1a.y,
            base1b.x,
            base1b.y,
            stroke = svg_escape_attr(&style.stroke),
        ));
        let tip2 = f2;
        let base2a = Point3D::new(
            f2.x - head * ux + half * nx,
            f2.y - head * uy + half * ny,
            0.0,
        );
        let base2b = Point3D::new(
            f2.x - head * ux - half * nx,
            f2.y - head * uy - half * ny,
            0.0,
        );
        self.body.push_str(&format!(
            "{indent}<path d=\"M {} {} L {} {} L {} {} Z\" fill=\"{stroke}\" stroke=\"none\"/>\n",
            tip2.x,
            tip2.y,
            base2a.x,
            base2a.y,
            base2b.x,
            base2b.y,
            stroke = svg_escape_attr(&style.stroke),
        ));
        // Dimension text — centered at the midpoint of the dim line.
        let mx = (f1.x + f2.x) * 0.5;
        let my = (f1.y + f2.y) * 0.5;
        let rotation_deg = uy.atan2(ux).to_degrees();
        let resolved_font = resolve_font_family(font_family);
        let escaped = svg_escape_text(text);
        self.body.push_str(&format!(
            "{indent}<text x=\"{mx}\" y=\"{my}\" font-family=\"{font}\" font-size=\"{text_height}\" \
             text-anchor=\"middle\" fill=\"{fill}\" \
             transform=\"translate({mx},{my}) scale(1,-1) rotate({neg_rot}) translate({neg_mx},{neg_my})\">{escaped}</text>\n",
            font = svg_escape_attr(&resolved_font),
            fill = svg_escape_attr(&style.stroke),
            neg_rot = -rotation_deg,
            neg_mx = -mx,
            neg_my = -my,
        ));
    }

    /// Append a title-block frame (L9-11) at `position` with the given
    /// `width` × `height` and a generic label→value field list.
    ///
    /// Emits:
    ///   * one closed `<path>` forming the outer rectangle (black stroke,
    ///     no fill, stroke-width `0.5`) — the drawing frame;
    ///   * one `<text>` element per `(label, value)` pair, laid out
    ///     top-down along the right edge of the frame in Arial at a font
    ///     size derived from the frame height (≈ 3% of `height`, clamped
    ///     to a sensible minimum).
    ///
    /// `fields` is intentionally generic — callers supply whatever schema
    /// their title block uses (Drawing Number, Revision, Date, Scale,
    /// Sheet Size, Project, Designer, Checked By, …). No assumptions are
    /// baked into the writer about which labels are present or their
    /// order; the caller owns the layout contract.
    ///
    /// Text uses [`resolve_font_family`] so `.shx` fallbacks and the
    /// existing font machinery behave the same way as [`Self::push_text`].
    pub fn push_title_block(
        &mut self,
        position: Point3D,
        width: f64,
        height: f64,
        fields: &[(String, String)],
    ) {
        let indent = self.current_indent();
        let x = position.x;
        let y = position.y;
        // Closed rectangular frame — path rather than <rect> so the
        // callers that post-process SVG geometry see the same tag family
        // as other boundary emitters (hatches, viewports).
        self.body.push_str(&format!(
            "{indent}<path d=\"M {x} {y} L {x2} {y} L {x2} {y2} L {x} {y2} Z\" \
             stroke=\"#000000\" fill=\"none\" stroke-width=\"0.5\" \
             data-role=\"title-block-frame\"/>\n",
            x2 = x + width,
            y2 = y + height,
        ));
        if fields.is_empty() {
            return;
        }
        // Field text: laid out top-down along the inside-right edge of
        // the frame, one row per field. Font size scales with the frame
        // height so small frames don't blow up and large frames don't
        // render tiny; clamp to at least 1.0 CAD unit for legibility.
        let row_count = fields.len() as f64;
        let font_size = (height * 0.03).max(height / (row_count * 1.5 + 1.0).max(2.0));
        let row_height = font_size * 1.6;
        let label_x = x + width * 0.55;
        let value_x = x + width * 0.95;
        let resolved_font = resolve_font_family("Arial");
        let font_escaped = svg_escape_attr(&resolved_font);
        let top_y = y + height - row_height * 0.8;
        for (i, (label, value)) in fields.iter().enumerate() {
            let row_y = top_y - (i as f64) * row_height;
            // Counter-flip Y so glyphs read right-side-up under the root
            // flip — identical transform shape used by push_text.
            let label_transform = format!(
                " transform=\"translate({lx},{ry}) scale(1,-1) translate({nlx},{nry})\"",
                lx = label_x,
                ry = row_y,
                nlx = -label_x,
                nry = -row_y,
            );
            let value_transform = format!(
                " transform=\"translate({vx},{ry}) scale(1,-1) translate({nvx},{nry})\"",
                vx = value_x,
                ry = row_y,
                nvx = -value_x,
                nry = -row_y,
            );
            self.body.push_str(&format!(
                "{indent}<text x=\"{label_x}\" y=\"{row_y}\" \
                 font-family=\"{font_escaped}\" font-size=\"{font_size}\" \
                 fill=\"#000000\"{label_transform} \
                 data-role=\"title-block-label\">{label_text}</text>\n",
                label_text = svg_escape_text(label),
            ));
            self.body.push_str(&format!(
                "{indent}<text x=\"{value_x}\" y=\"{row_y}\" \
                 font-family=\"{font_escaped}\" font-size=\"{font_size}\" \
                 text-anchor=\"end\" fill=\"#000000\"{value_transform} \
                 data-role=\"title-block-value\">{value_text}</text>\n",
                value_text = svg_escape_text(value),
            ));
        }
    }

    /// Append a paper-space viewport (L9-12) at `position` with the given
    /// `width` × `height`. The viewport defines a clipping region for
    /// model-space content projected onto a paper-space sheet.
    ///
    /// Emits:
    ///   * one `<clipPath id="clip-NNN">` in the document `<defs>` block
    ///     (deduplicated via [`Self::clip_paths`] — repeated registrations
    ///     of the same id are no-ops); the clip path is a rectangular
    ///     region `width` × `height` in CAD units;
    ///   * one opening `<g clip-path="url(#clip-NNN)">` in the body.
    ///
    /// Subsequent `push_*` calls write elements that are clipped by the
    /// viewport rectangle. Call [`Self::pop_clip`] to close the group.
    ///
    /// `clip_path_id` is the raw id the caller wants to use (e.g.
    /// `"viewport-42"`); the `clip-` prefix is added automatically so the
    /// final element id is `clip-viewport-42`. Non-ASCII characters are
    /// escaped per [`svg_escape_attr`] but callers are encouraged to use
    /// ids matching `[A-Za-z0-9_-]+`.
    pub fn push_viewport(
        &mut self,
        position: Point3D,
        width: f64,
        height: f64,
        clip_path_id: &str,
    ) {
        let indent = self.current_indent();
        let full_id = format!("clip-{clip_path_id}");
        let safe_id = svg_escape_attr(&full_id);
        let x = position.x;
        let y = position.y;
        // Register the clipPath once — dedupe by id against any previous
        // registration so a caller can emit the same viewport id twice
        // without producing a malformed defs block.
        let needle = format!("id=\"{safe_id}\"");
        if !self.clip_paths.iter().any(|d| d.contains(&needle)) {
            self.clip_paths.push(format!(
                "<clipPath id=\"{safe_id}\">\
                 <path d=\"M {x} {y} L {x2} {y} L {x2} {y2} L {x} {y2} Z\"/>\
                 </clipPath>",
                x2 = x + width,
                y2 = y + height,
            ));
        }
        self.body.push_str(&format!(
            "{indent}<g clip-path=\"url(#{safe_id})\" data-role=\"viewport\">\n"
        ));
    }

    /// Close the most recent [`Self::push_viewport`] (or any caller-managed
    /// clip group). Emits the matching `</g>`. Balanced pairing is the
    /// caller's responsibility — this method does not validate that an
    /// open clip group exists and will happily emit an unbalanced tag if
    /// called incorrectly.
    pub fn pop_clip(&mut self) {
        let indent = self.current_indent();
        self.body.push_str(&format!("{indent}</g>\n"));
    }

    /// Render this document as an SVG shaped for browser-based PDF
    /// export (L9-13) via headless Chromium. Returns the SVG bytes.
    ///
    /// `page_size` is the target page size in millimetres (e.g.
    /// `(210.0, 297.0)` for ISO A4 portrait). `dpi` is the resolution
    /// the browser should use when rasterizing the page (typically
    /// `96.0` for web or `300.0` for print-quality).
    ///
    /// The returned SVG embeds a `<style>` block containing a CSS
    /// `@page { size: Wmm Hmm; margin: 0; }` rule and sets the root
    /// `<svg>` element's width/height to the physical page size. Paired
    /// with headless Chromium's "print to PDF" mode, this produces a
    /// pixel-accurate PDF without pulling a heavyweight PDF crate into
    /// the dependency tree:
    ///
    /// ```text
    /// chromium --headless --print-to-pdf=out.pdf file:///path/to/input.svg
    /// ```
    ///
    /// First-class PDF generation (an embedded PDF writer that doesn't
    /// depend on an external browser) is deferred to a follow-up task;
    /// the open TODO is tracked as `L9-13a` — see the module docs.
    pub fn to_pdf_via_paged_svg(&self, page_size: (f64, f64), dpi: f64) -> Result<Vec<u8>> {
        let (w_mm, h_mm) = page_size;
        // Compute pixel size from mm × dpi. The `@page` rule uses raw
        // millimetres so the PDF rasterizer sees a physical page; the
        // `<svg width="…px">` attribute is for browsers that don't
        // honor `@page` (e.g. older headless Chromium) so the SVG still
        // lays out at the correct pixel size in a viewport.
        let mm_per_inch = 25.4;
        let w_px = w_mm / mm_per_inch * dpi;
        let h_px = h_mm / mm_per_inch * dpi;
        // Rebuild the SVG body with paged-SVG framing. This mirrors the
        // shape of `finish()` but takes a borrowed self (no consume) so
        // the caller can keep using the document if desired.
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" \
             xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" \
             width=\"{w_mm}mm\" height=\"{h_mm}mm\" \
             viewBox=\"0 0 {inner_w} {inner_h}\" \
             data-print-dpi=\"{dpi}\" data-print-page-mm=\"{w_mm}x{h_mm}\" \
             data-print-page-px=\"{w_px}x{h_px}\">\n",
            inner_w = self.width,
            inner_h = self.height,
        ));
        // CSS @page rule — honored by headless Chromium in print mode.
        // The `size` declaration uses millimetres so the PDF is
        // dimensioned in physical units regardless of the SVG's
        // internal user-unit coordinate system.
        out.push_str(&format!(
            "  <style>\n\
             @page {{ size: {w_mm}mm {h_mm}mm; margin: 0; }}\n\
             svg {{ width: {w_mm}mm; height: {h_mm}mm; }}\n\
             </style>\n"
        ));
        if !self.pattern_defs.is_empty() || !self.clip_paths.is_empty() {
            out.push_str("  <defs>\n");
            for def in &self.pattern_defs {
                out.push_str("    ");
                out.push_str(def);
                out.push('\n');
            }
            for def in &self.clip_paths {
                out.push_str("    ");
                out.push_str(def);
                out.push('\n');
            }
            out.push_str("  </defs>\n");
        }
        let layout_attr = match &self.space {
            SvgSpace::Model => String::new(),
            SvgSpace::Paper(name) => format!(" data-layout=\"{}\"", svg_escape_attr(name)),
        };
        out.push_str(&format!(
            "  <g transform=\"translate(0,{h}) scale(1,-1)\"{layout_attr}>\n",
            h = self.height,
        ));
        // Close any still-open layer group so the body is well-formed.
        out.push_str(&self.body);
        if self.current_layer.is_some() {
            out.push_str("  </g>\n");
        }
        out.push_str("  </g>\n");
        out.push_str("</svg>\n");
        Ok(out.into_bytes())
    }

    /// Resolve the indent prefix for the current nesting depth (root or
    /// inside a layer group). Centralized so the four new emitters stay
    /// in sync with the original `push_curve` / `push_path` indent rule.
    fn current_indent(&self) -> &'static str {
        if self.current_layer.is_some() {
            "    "
        } else {
            "  "
        }
    }

    /// Register a `<pattern>` definition for inclusion in the document
    /// `<defs>`. Idempotent — the second registration of a given id is a
    /// no-op so callers don't need to track which patterns they've used.
    fn register_pattern(&mut self, id: &str, body: &str) {
        let needle = format!("id=\"{id}\"");
        if self.pattern_defs.iter().any(|d| d.contains(&needle)) {
            return;
        }
        self.pattern_defs.push(body.to_string());
    }

    /// Emit one MTEXT span — internal helper for [`Self::push_mtext`].
    fn flush_mtext_buf(
        body: &mut String,
        buf: &mut String,
        style: &MTextStyle,
        first_line: bool,
        x: f64,
        height: f64,
    ) {
        if buf.is_empty() {
            return;
        }
        let mut attrs = String::new();
        attrs.push_str(&format!(
            " font-family=\"{}\"",
            svg_escape_attr(&style.font)
        ));
        attrs.push_str(&format!(" fill=\"{}\"", svg_escape_attr(&style.fill)));
        if (style.height_scale - 1.0).abs() > f64::EPSILON {
            attrs.push_str(&format!(" font-size=\"{}\"", height * style.height_scale));
        }
        let mut deco = Vec::new();
        if style.underline {
            deco.push("underline");
        }
        if style.overline {
            deco.push("overline");
        }
        if !deco.is_empty() {
            attrs.push_str(&format!(" text-decoration=\"{}\"", deco.join(" ")));
        }
        if !first_line {
            attrs.push_str(&format!(" x=\"{x}\" dy=\"{height}\""));
        }
        body.push_str(&format!("<tspan{attrs}>{}</tspan>", svg_escape_text(buf)));
        buf.clear();
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
        // Pattern + clip-path defs sit OUTSIDE the Y-flip transform so
        // the pattern tile orientation matches user expectation (lines
        // that look 45-degree in the source render 45-degree on screen)
        // and clipPath coordinates are interpreted in the same
        // post-transform space as the elements that reference them.
        if !self.pattern_defs.is_empty() || !self.clip_paths.is_empty() {
            out.push_str("  <defs>\n");
            for def in &self.pattern_defs {
                out.push_str("    ");
                out.push_str(def);
                out.push('\n');
            }
            for def in &self.clip_paths {
                out.push_str("    ");
                out.push_str(def);
                out.push('\n');
            }
            out.push_str("  </defs>\n");
        }
        // CAD Y-up → SVG Y-down flip. In paper space we also tag the
        // root group with a `data-layout` attribute so downstream
        // tooling can distinguish paper-space sheets from model space.
        let layout_attr = match &self.space {
            SvgSpace::Model => String::new(),
            SvgSpace::Paper(name) => format!(" data-layout=\"{}\"", svg_escape_attr(name)),
        };
        out.push_str(&format!(
            "  <g transform=\"translate(0,{h}) scale(1,-1)\"{layout_attr}>\n"
        ));
        out.push_str(&self.body);
        out.push_str("  </g>\n");
        out.push_str("</svg>\n");
        out
    }
}

/// One frame of the MTEXT inline-style stack.
#[derive(Debug, Clone)]
struct MTextStyle {
    font: String,
    fill: String,
    height_scale: f64,
    underline: bool,
    overline: bool,
}

impl Style {
    fn to_attrs(&self) -> String {
        let fill = self
            .fill
            .as_ref()
            .map(|f| format!(" fill=\"{f}\""))
            .unwrap_or_else(|| " fill=\"none\"".to_string());
        let dashes = self
            .dashes
            .as_ref()
            .map(|v| {
                format!(
                    " stroke-dasharray=\"{}\"",
                    v.iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            })
            .unwrap_or_default();
        format!(
            " stroke=\"{}\" stroke-width=\"{}\"{fill}{dashes}",
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

/// Escape characters that break XML element text content. Quotes and
/// apostrophes are valid in text nodes — only the three structural
/// characters need encoding here. Kept distinct from [`svg_escape_attr`]
/// so callers don't double-encode quotes inside `<text>...</text>`.
fn svg_escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Resolve a CAD-supplied font family to an SVG-renderable family.
/// AutoCAD `.shx` shape files are vector formats no browser can render;
/// they fall back to `Arial, sans-serif`. Other families pass through
/// with `, sans-serif` appended for graceful degradation when the
/// requested family is not installed.
fn resolve_font_family(font_family: &str) -> String {
    let trimmed = font_family.trim();
    if trimmed.is_empty() {
        return "Arial, sans-serif".to_string();
    }
    if trimmed.to_ascii_lowercase().ends_with(".shx") {
        return "Arial, sans-serif".to_string();
    }
    format!("{trimmed}, sans-serif")
}

/// Parse one MTEXT inline-code argument starting at `start`. The
/// argument runs up to the next `;` (which is consumed) or the next
/// whitespace character (which is NOT consumed). Returns the argument
/// text and the number of characters advanced past `start`.
fn parse_mtext_arg(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut value = String::new();
    let mut consumed = 0;
    let mut found_terminator = false;
    while start + consumed < chars.len() {
        let ch = chars[start + consumed];
        if ch == ';' {
            // Semicolon is the canonical terminator and is consumed.
            consumed += 1;
            found_terminator = true;
            break;
        }
        if ch.is_whitespace() {
            // Whitespace also ends an arg, but stays in the input
            // stream so it renders as the leading space of the text.
            found_terminator = true;
            break;
        }
        value.push(ch);
        consumed += 1;
    }
    if !found_terminator && value.is_empty() {
        return None;
    }
    Some((value, consumed))
}

/// Map the AutoCAD Color Index (ACI, 0–255) to a hex `#RRGGBB`. Only
/// the canonical first 8 indices are exact; everything beyond falls
/// back to a deterministic synthesized color so the SVG remains valid.
fn aci_to_hex(idx: u32) -> String {
    match idx {
        0 => "#000000".to_string(), // BYBLOCK — render as black
        1 => "#FF0000".to_string(),
        2 => "#FFFF00".to_string(),
        3 => "#00FF00".to_string(),
        4 => "#00FFFF".to_string(),
        5 => "#0000FF".to_string(),
        6 => "#FF00FF".to_string(),
        7 => "#FFFFFF".to_string(),
        8 => "#414141".to_string(),
        9 => "#808080".to_string(),
        256 => "#000000".to_string(), // BYLAYER
        _ => {
            // Spread the remaining indices across HSL space deterministically.
            let h = (f64::from(idx) * 137.508) % 360.0;
            let (r, g, b) = hsl_to_rgb(h, 0.7, 0.5);
            format!("#{r:02X}{g:02X}{b:02X}")
        }
    }
}

/// HSL → RGB helper for [`aci_to_hex`]. `h` in degrees, `s`/`l` in 0..1.
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (rp, gp, bp) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((rp + m) * 255.0).round() as u8,
        ((gp + m) * 255.0).round() as u8,
        ((bp + m) * 255.0).round() as u8,
    )
}

/// Build an SVG `d=` attribute string for a hatch boundary loop. Lines
/// emit `M x y L x y …`; arcs use `A` commands; polylines flatten to
/// `L` segments. Splines / circles inside hatch boundaries are uncommon
/// enough that they emit nothing here (a future revision may
/// pre-tessellate). Closes each loop with `Z`.
fn boundary_path_d(boundary: &Path) -> String {
    let mut d = String::new();
    let mut moved = false;
    let mut last: Option<Point3D> = None;
    for seg in &boundary.segments {
        match seg {
            Curve::Line { a, b } => {
                if !moved {
                    d.push_str(&format!("M {} {} ", a.x, a.y));
                    moved = true;
                } else if let Some(p) = last
                    && (p.x - a.x).abs() + (p.y - a.y).abs() > f64::EPSILON
                {
                    // Discontinuous — start a new sub-loop.
                    d.push_str(&format!("M {} {} ", a.x, a.y));
                }
                d.push_str(&format!("L {} {} ", b.x, b.y));
                last = Some(*b);
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
                if !moved {
                    d.push_str(&format!("M {} {} ", p0.x, p0.y));
                    moved = true;
                }
                let large_arc = if (end_angle - start_angle).abs() > std::f64::consts::PI {
                    1
                } else {
                    0
                };
                let sweep = if end_angle > start_angle { 1 } else { 0 };
                d.push_str(&format!(
                    "A {radius} {radius} 0 {large_arc} {sweep} {} {} ",
                    p1.x, p1.y
                ));
                last = Some(p1);
            }
            Curve::Polyline { vertices, .. } => {
                for (i, v) in vertices.iter().enumerate() {
                    if i == 0 && !moved {
                        d.push_str(&format!("M {} {} ", v.point.x, v.point.y));
                        moved = true;
                    } else {
                        d.push_str(&format!("L {} {} ", v.point.x, v.point.y));
                    }
                    last = Some(v.point);
                }
            }
            _ => {
                // Splines / circles / ellipses inside hatch boundaries
                // are uncommon enough that we emit a no-op marker; a
                // future revision may pre-tessellate these.
            }
        }
    }
    if boundary.closed && !d.is_empty() {
        d.push('Z');
    }
    d.trim_end().to_string()
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
            dashes: None,
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
    fn style_with_dashes_emits_stroke_dasharray() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style {
            dashes: Some(vec![4.0, 2.0, 1.0, 2.0]),
            ..Default::default()
        };
        doc.push_curve(
            &Curve::Line {
                a: Point3D::new(0.0, 0.0, 0.0),
                b: Point3D::new(10.0, 0.0, 0.0),
            },
            &style,
            None,
        );
        let s = doc.finish();
        assert!(s.contains("stroke-dasharray=\"4,2,1,2\""));
    }

    #[test]
    fn style_without_dashes_omits_dasharray_attribute() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        doc.push_curve(
            &Curve::Line {
                a: Point3D::new(0.0, 0.0, 0.0),
                b: Point3D::new(1.0, 1.0, 0.0),
            },
            &style,
            None,
        );
        let s = doc.finish();
        assert!(!s.contains("stroke-dasharray"));
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

    // --- L9-05: TEXT ----------------------------------------------------

    #[test]
    fn push_text_emits_text_element_with_font_and_size() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        doc.push_text(
            "Hello",
            Point3D::new(10.0, 20.0, 0.0),
            5.0,
            0.0,
            "Helvetica",
            &style,
            Some("0x42"),
        );
        let s = doc.finish();
        assert!(s.contains("<text"));
        assert!(s.contains(">Hello</text>"));
        assert!(s.contains("font-family=\"Helvetica, sans-serif\""));
        assert!(s.contains("font-size=\"5\""));
        assert!(s.contains("data-handle=\"0x42\""));
        assert!(s.contains("x=\"10\""));
        assert!(s.contains("y=\"20\""));
        // Counter-flip transform must be present.
        assert!(s.contains("scale(1,-1)"));
    }

    #[test]
    fn push_text_falls_back_arial_for_shx_fonts() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        doc.push_text(
            "ROMANS",
            Point3D::new(0.0, 0.0, 0.0),
            2.5,
            0.0,
            "romans.shx",
            &style,
            None,
        );
        let s = doc.finish();
        // SHX should be silently swapped for Arial — never expose the
        // unrenderable family name to a downstream SVG consumer.
        assert!(s.contains("font-family=\"Arial, sans-serif\""));
        assert!(!s.contains("romans.shx"));
    }

    #[test]
    fn push_text_escapes_xml_special_characters_in_content() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        doc.push_text(
            "A & B < C > D",
            Point3D::new(0.0, 0.0, 0.0),
            2.0,
            0.0,
            "Arial",
            &style,
            None,
        );
        let s = doc.finish();
        assert!(s.contains("A &amp; B &lt; C &gt; D"));
    }

    // --- L9-06: MTEXT ---------------------------------------------------

    #[test]
    fn push_mtext_paragraph_break_creates_new_tspan_with_dy() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        doc.push_mtext(
            "first line\\Psecond line",
            Point3D::new(5.0, 10.0, 0.0),
            3.0,
            0.0,
            "Arial",
            &style,
            None,
        );
        let s = doc.finish();
        // Two tspans expected — one per paragraph line.
        assert!(
            s.matches("<tspan").count() >= 2,
            "expected ≥2 <tspan> for two-paragraph mtext, svg was: {s}"
        );
        assert!(s.contains("first line"));
        assert!(s.contains("second line"));
        assert!(s.contains("dy=\"3\""));
        assert!(s.contains("x=\"5\""));
    }

    #[test]
    fn push_mtext_supports_underline_overline_color_height_codes() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        doc.push_mtext(
            "\\Lunder\\l \\Oover\\o \\C1;red \\H1.5x;big",
            Point3D::new(0.0, 0.0, 0.0),
            4.0,
            0.0,
            "Arial",
            &style,
            None,
        );
        let s = doc.finish();
        assert!(s.contains("text-decoration=\"underline\""));
        assert!(s.contains("text-decoration=\"overline\""));
        // ACI 1 = red.
        assert!(s.contains("fill=\"#FF0000\""));
        // 1.5x height multiplier on a base of 4.0 → 6.
        assert!(s.contains("font-size=\"6\""));
        // All four literal payload words must reach the output.
        assert!(s.contains("under"));
        assert!(s.contains("over"));
        assert!(s.contains("red"));
        assert!(s.contains("big"));
    }

    #[test]
    fn push_mtext_unknown_code_emits_diagnostic_comment() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        // \Q is not a recognized MTEXT code.
        doc.push_mtext(
            "before\\Qafter",
            Point3D::new(0.0, 0.0, 0.0),
            3.0,
            0.0,
            "Arial",
            &style,
            None,
        );
        let s = doc.finish();
        assert!(s.contains("<!-- mtext code: \\Q -->"));
        // Both flanking texts still emitted.
        assert!(s.contains("before"));
        assert!(s.contains("after"));
    }

    #[test]
    fn push_mtext_brace_groups_push_and_pop_style_stack() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        doc.push_mtext(
            "outer{\\C1;red}back",
            Point3D::new(0.0, 0.0, 0.0),
            3.0,
            0.0,
            "Arial",
            &style,
            None,
        );
        let s = doc.finish();
        assert!(s.contains("outer"));
        assert!(s.contains("red"));
        assert!(s.contains("back"));
        // The "back" tspan must NOT inherit the red color from inside
        // the closed brace group — i.e., the pop restored the original
        // black/stroke fill.
        assert!(s.contains("fill=\"#000000\""));
    }

    // --- L9-07: HATCH ---------------------------------------------------

    #[test]
    fn push_hatch_solid_emits_filled_path() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let pts = [
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(10.0, 0.0, 0.0),
            Point3D::new(10.0, 10.0, 0.0),
            Point3D::new(0.0, 10.0, 0.0),
        ];
        let boundary = Path::from_polyline(&pts, true);
        doc.push_hatch_solid(&boundary, "#888888", Some("0xC0"));
        let s = doc.finish();
        assert!(s.contains("<path"));
        assert!(s.contains("fill=\"#888888\""));
        assert!(s.contains("fill-rule=\"evenodd\""));
        assert!(s.contains("stroke=\"none\""));
        assert!(s.contains("data-handle=\"0xC0\""));
        assert!(s.contains("Z"));
    }

    #[test]
    fn push_hatch_pattern_ansi31_registers_defs_block_once() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let pts = [
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(10.0, 0.0, 0.0),
            Point3D::new(10.0, 10.0, 0.0),
        ];
        let boundary = Path::from_polyline(&pts, true);
        // Push the same pattern twice — the def must appear once.
        doc.push_hatch_pattern(&boundary, "ANSI31", 1.0, 0.0, "#000000", None);
        doc.push_hatch_pattern(&boundary, "ANSI31", 1.0, 0.0, "#000000", None);
        let s = doc.finish();
        assert!(s.contains("<defs>"));
        assert_eq!(
            s.matches("id=\"hatch-ANSI31\"").count(),
            1,
            "pattern def must dedupe to a single registration"
        );
        // Two boundary paths reference the same pattern.
        assert_eq!(
            s.matches("fill=\"url(#hatch-ANSI31)\"").count(),
            2,
            "each push_hatch_pattern call should still emit its own <path>"
        );
    }

    #[test]
    fn push_hatch_pattern_solid_delegates_to_solid_method() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let pts = [
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(5.0, 0.0, 0.0),
            Point3D::new(5.0, 5.0, 0.0),
        ];
        let boundary = Path::from_polyline(&pts, true);
        doc.push_hatch_pattern(&boundary, "SOLID", 1.0, 0.0, "#123456", None);
        let s = doc.finish();
        // SOLID delegate must NOT register a pattern def.
        assert!(!s.contains("<defs>"));
        assert!(s.contains("fill=\"#123456\""));
    }

    #[test]
    fn push_hatch_pattern_unknown_falls_back_to_solid_with_comment() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let pts = [
            Point3D::new(0.0, 0.0, 0.0),
            Point3D::new(5.0, 0.0, 0.0),
            Point3D::new(5.0, 5.0, 0.0),
        ];
        let boundary = Path::from_polyline(&pts, true);
        doc.push_hatch_pattern(&boundary, "EARTH", 1.0, 0.0, "#998877", None);
        let s = doc.finish();
        assert!(s.contains("<!-- hatch-pattern unsupported: EARTH -->"));
        // Visible solid fill still emitted as the safety fallback.
        assert!(s.contains("fill=\"#998877\""));
        assert!(!s.contains("<defs>"));
    }

    // --- L9-08: DIMENSION -----------------------------------------------

    #[test]
    fn push_dimension_linear_emits_extension_dim_arrows_and_text() {
        let mut doc = SvgDoc::new(200.0, 200.0);
        let style = Style {
            stroke: "#0000FF".to_string(),
            ..Default::default()
        };
        // Horizontal baseline from (10,10) to (60,10), dim line offset
        // 20 above (so dim line passes through y=30).
        doc.push_dimension_linear(
            Point3D::new(10.0, 10.0, 0.0),
            Point3D::new(60.0, 10.0, 0.0),
            Point3D::new(35.0, 30.0, 0.0),
            "50.00",
            "Arial",
            3.0,
            &style,
        );
        let s = doc.finish();
        // Three <line> elements: two extension + one dim line.
        assert!(
            s.matches("<line").count() >= 3,
            "expected ≥3 <line> elements (2 extension + 1 dim), got: {s}"
        );
        // Two arrowhead triangle paths (Z-closed M..L..L..Z fragments).
        assert!(s.matches("Z\" fill=\"#0000FF\"").count() >= 2);
        // Dimension text rendered at midpoint x = 35.
        assert!(s.contains(">50.00</text>"));
        assert!(s.contains("text-anchor=\"middle\""));
        // The dim line foot is at y = 30 (perpendicular projection of
        // dim line onto a horizontal baseline parallel to itself).
        assert!(s.contains("y1=\"30\""));
        assert!(s.contains("y2=\"30\""));
    }

    #[test]
    fn push_dimension_linear_zero_length_baseline_emits_diagnostic_comment_only() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        doc.push_dimension_linear(
            Point3D::new(5.0, 5.0, 0.0),
            Point3D::new(5.0, 5.0, 0.0),
            Point3D::new(5.0, 10.0, 0.0),
            "0.00",
            "Arial",
            2.0,
            &style,
        );
        let s = doc.finish();
        assert!(s.contains("<!-- dimension-linear: zero-length baseline -->"));
        assert!(!s.contains("<line"));
        // Outer <text> count from the dimension method must be zero.
        assert!(!s.contains("<text"));
    }

    // --- L9-07/08 helper utilities --------------------------------------

    #[test]
    fn aci_to_hex_canonical_indices_are_exact_known_values() {
        assert_eq!(aci_to_hex(1), "#FF0000");
        assert_eq!(aci_to_hex(2), "#FFFF00");
        assert_eq!(aci_to_hex(3), "#00FF00");
        assert_eq!(aci_to_hex(7), "#FFFFFF");
        // ACI 256 is BYLAYER → render as black so the SVG is valid.
        assert_eq!(aci_to_hex(256), "#000000");
    }

    #[test]
    fn resolve_font_family_handles_shx_empty_and_normal() {
        assert_eq!(resolve_font_family("Helvetica"), "Helvetica, sans-serif");
        assert_eq!(resolve_font_family("ROMANS.SHX"), "Arial, sans-serif");
        assert_eq!(resolve_font_family("simplex.shx"), "Arial, sans-serif");
        assert_eq!(resolve_font_family(""), "Arial, sans-serif");
        assert_eq!(resolve_font_family("   "), "Arial, sans-serif");
    }

    // --- L9-10..13: paper space / title block / viewport / PDF --------

    #[test]
    fn svg_space_default_is_model() {
        // Default-constructed SvgDoc renders model space and omits the
        // `data-layout` attribute on the root flip group. Negative check
        // against paper-space output establishes the contract that
        // downstream tooling relies on to distinguish the two.
        let doc = SvgDoc::new(100.0, 100.0);
        let s = doc.finish();
        assert!(!s.contains("data-layout"));
    }

    #[test]
    fn with_space_paper_emits_data_layout_on_root_group() {
        let doc = SvgDoc::new(100.0, 100.0).with_space(SvgSpace::Paper("Layout1".to_string()));
        let s = doc.finish();
        assert!(s.contains("data-layout=\"Layout1\""));
        // The attribute must live on the flip-group wrapper, not the
        // outer <svg> (so per-layout styling works under a common root).
        assert!(s.contains("scale(1,-1)\" data-layout=\"Layout1\""));
    }

    #[test]
    fn model_space_and_paper_space_root_g_attributes_differ() {
        // Regression guard: the two spaces must produce meaningfully
        // different root-group markup so downstream tooling can route
        // content correctly. Model space: bare `<g transform=…>`; paper
        // space: `<g transform=… data-layout="…">`.
        let model = SvgDoc::new(100.0, 100.0).finish();
        let paper = SvgDoc::new(100.0, 100.0)
            .with_space(SvgSpace::Paper("Sheet-A".to_string()))
            .finish();
        assert_ne!(model, paper);
        assert!(!model.contains("data-layout"));
        assert!(paper.contains("data-layout=\"Sheet-A\""));
    }

    #[test]
    fn paper_space_layout_name_is_attribute_escaped() {
        // Layout names may contain `&`, quotes, or angle brackets — SVG
        // attributes must stay well-formed regardless.
        let doc =
            SvgDoc::new(10.0, 10.0).with_space(SvgSpace::Paper("A & B <Layout>\"1\"".to_string()));
        let s = doc.finish();
        assert!(s.contains("data-layout=\"A &amp; B &lt;Layout&gt;&quot;1&quot;\""));
    }

    #[test]
    fn push_title_block_emits_frame_and_field_text() {
        let mut doc = SvgDoc::new(300.0, 200.0);
        let fields = vec![
            ("Drawing Number".to_string(), "DWG-001".to_string()),
            ("Revision".to_string(), "A".to_string()),
            ("Date".to_string(), "2026-04-19".to_string()),
            ("Scale".to_string(), "1:100".to_string()),
        ];
        doc.push_title_block(Point3D::new(10.0, 10.0, 0.0), 280.0, 60.0, &fields);
        let s = doc.finish();
        // Frame path — closed rectangle with the required style.
        assert!(s.contains("data-role=\"title-block-frame\""));
        assert!(s.contains("stroke=\"#000000\""));
        assert!(s.contains("fill=\"none\""));
        assert!(s.contains("stroke-width=\"0.5\""));
        // Each field label + value is a separate <text> element.
        assert!(s.contains(">Drawing Number</text>"));
        assert!(s.contains(">DWG-001</text>"));
        assert!(s.contains(">Revision</text>"));
        assert!(s.contains(">2026-04-19</text>"));
        assert!(s.contains("data-role=\"title-block-label\""));
        assert!(s.contains("data-role=\"title-block-value\""));
        // Font machinery matches the rest of the writer — Arial plus
        // the , sans-serif graceful-degradation suffix.
        assert!(s.contains("font-family=\"Arial, sans-serif\""));
    }

    #[test]
    fn push_title_block_with_empty_fields_emits_frame_only() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        doc.push_title_block(Point3D::new(0.0, 0.0, 0.0), 100.0, 50.0, &[]);
        let s = doc.finish();
        assert!(s.contains("data-role=\"title-block-frame\""));
        assert!(!s.contains("title-block-label"));
        assert!(!s.contains("title-block-value"));
    }

    #[test]
    fn push_title_block_escapes_xml_specials_in_field_values() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let fields = vec![("Client".to_string(), "Smith & <Co>".to_string())];
        doc.push_title_block(Point3D::new(0.0, 0.0, 0.0), 100.0, 50.0, &fields);
        let s = doc.finish();
        assert!(s.contains(">Smith &amp; &lt;Co&gt;</text>"));
    }

    #[test]
    fn push_viewport_registers_clip_path_and_opens_group() {
        let mut doc = SvgDoc::new(400.0, 300.0);
        doc.push_viewport(Point3D::new(20.0, 30.0, 0.0), 300.0, 200.0, "vp1");
        doc.pop_clip();
        let s = doc.finish();
        // clipPath id is the concatenation of the `clip-` prefix and the
        // caller-supplied id.
        assert!(s.contains("<clipPath id=\"clip-vp1\""));
        // The clip shape is a rectangle sized to (20+300, 30+200).
        assert!(s.contains("M 20 30"));
        assert!(s.contains("L 320 230"));
        // The body references the clip path and balances with </g>.
        assert!(s.contains("clip-path=\"url(#clip-vp1)\""));
        assert!(s.contains("data-role=\"viewport\""));
    }

    #[test]
    fn push_viewport_dedupes_repeat_registrations_of_same_id() {
        // Two calls with the same clip id should still only emit one
        // <clipPath> definition — mirrors the pattern_defs dedupe rule.
        let mut doc = SvgDoc::new(400.0, 300.0);
        doc.push_viewport(Point3D::new(0.0, 0.0, 0.0), 100.0, 100.0, "dup");
        doc.pop_clip();
        doc.push_viewport(Point3D::new(0.0, 0.0, 0.0), 100.0, 100.0, "dup");
        doc.pop_clip();
        let s = doc.finish();
        assert_eq!(s.matches("<clipPath id=\"clip-dup\"").count(), 1);
        // But each push_viewport opens its own <g>, so two clip-path
        // references are expected in the body.
        assert_eq!(s.matches("clip-path=\"url(#clip-dup)\"").count(), 2);
    }

    #[test]
    fn pop_clip_emits_closing_g_tag() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        doc.push_viewport(Point3D::new(0.0, 0.0, 0.0), 50.0, 50.0, "v");
        // Body should contain an opening <g clip-path=…> at this point.
        doc.pop_clip();
        let s = doc.finish();
        // One opening clip group, one closing </g> for it plus the
        // root flip group.
        assert_eq!(s.matches("data-role=\"viewport\"").count(), 1);
        assert!(s.matches("</g>").count() >= 2);
    }

    #[test]
    fn to_pdf_via_paged_svg_emits_page_size_in_mm_and_dpi_metadata() {
        let doc = SvgDoc::new(100.0, 100.0);
        let bytes = doc
            .to_pdf_via_paged_svg((210.0, 297.0), 96.0)
            .expect("paged SVG should emit");
        let s = std::str::from_utf8(&bytes).expect("UTF-8 output");
        // Root svg dimensions are in millimetres.
        assert!(s.contains("width=\"210mm\""));
        assert!(s.contains("height=\"297mm\""));
        // @page CSS rule — size in millimetres, zero margin so the
        // drawing fills the entire page.
        assert!(s.contains("@page { size: 210mm 297mm; margin: 0; }"));
        // DPI metadata round-trips through data-print-dpi.
        assert!(s.contains("data-print-dpi=\"96\""));
        assert!(s.contains("data-print-page-mm=\"210x297\""));
    }

    #[test]
    fn to_pdf_via_paged_svg_preserves_body_elements() {
        let mut doc = SvgDoc::new(100.0, 100.0);
        let style = Style::default();
        doc.push_curve(
            &Curve::Line {
                a: Point3D::new(0.0, 0.0, 0.0),
                b: Point3D::new(50.0, 50.0, 0.0),
            },
            &style,
            None,
        );
        let bytes = doc
            .to_pdf_via_paged_svg((210.0, 297.0), 300.0)
            .expect("paged SVG should emit");
        let s = std::str::from_utf8(&bytes).expect("UTF-8 output");
        assert!(s.contains("<line"));
        assert!(s.contains("x2=\"50\""));
        // Paper-space layouts must also make it through to_pdf output.
    }

    #[test]
    fn to_pdf_via_paged_svg_propagates_paper_space_data_layout() {
        let doc = SvgDoc::new(100.0, 100.0).with_space(SvgSpace::Paper("ISO-A4".into()));
        let bytes = doc
            .to_pdf_via_paged_svg((210.0, 297.0), 96.0)
            .expect("paged SVG should emit");
        let s = std::str::from_utf8(&bytes).expect("UTF-8 output");
        assert!(s.contains("data-layout=\"ISO-A4\""));
    }
}
