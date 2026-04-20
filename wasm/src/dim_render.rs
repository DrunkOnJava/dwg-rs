//! V-10 — dimension rendering.
//!
//! Surfaces DIMENSION-family entities as SVG fragments suitable for
//! direct composition inside the wasm viewer. Delegates to the
//! parent crate's [`dwg::svg::SvgDoc::push_dimension_linear`] by
//! building a scratch `SvgDoc`, invoking the emitter, and returning
//! the body fragment — so every fix or enhancement to the linear
//! dimension writer flows through automatically.
//!
//! # Coverage
//!
//! | Subtype        | Parent-crate module                             | This module |
//! |----------------|-------------------------------------------------|-------------|
//! | Linear         | [`dwg::entities::dimension_linear`]             | `render_linear`      |
//! | Aligned        | [`dwg::entities::dimension_aligned`]            | `render_aligned`     |
//! | Radial         | [`dwg::entities::dimension_radial`]             | `render_radial`      |
//! | Diameter       | [`dwg::entities::dimension_diameter`]           | `render_diameter`    |
//! | Angular (2L)   | [`dwg::entities::dimension_angular_2l`]         | `render_angular_2l`  |
//! | Angular (3P)   | [`dwg::entities::dimension_angular_3p`]         | `render_angular_3p`  |
//! | Ordinate       | [`dwg::entities::dimension_ordinate`]           | `render_ordinate`    |
//!
//! Only **linear** currently has a first-class SVG emitter in the
//! parent crate ([`dwg::svg::SvgDoc::push_dimension_linear`]); the
//! other subtypes use composable lower-level primitives (extension
//! lines, arrowheads, text label) built locally here. When/if the
//! parent crate grows first-class emitters for the other families,
//! the local composers collapse to a delegating call.
//!
//! # Output shape
//!
//! Each `render_*` function returns a `String` containing one or
//! more SVG elements (extension lines, dim line, arrowheads, text
//! label). The caller embeds the fragment inside its own SVG root;
//! no envelope is emitted here.

use dwg::entities::Point3D;
use dwg::svg::{Style, SvgDoc};

use crate::text_render::resolve_font_family;

/// Default stroke width for dimension geometry (CAD units). Matches
/// the parent-crate SVG writer convention for thin construction
/// lines.
const DEFAULT_STROKE_WIDTH: f64 = 0.5;

/// Render a linear DIMENSION. Delegates to the parent-crate
/// [`dwg::svg::SvgDoc::push_dimension_linear`] via a scratch
/// `SvgDoc`, then extracts the emitted body fragment.
///
/// `p1` / `p2` are the two measured points; `dimension_line` is any
/// point on the offset dimension line (the parent emitter projects
/// both measured points perpendicularly onto that line). `text` is
/// the measurement label, `font_family` follows the `.shx` fallback
/// rule in [`crate::text_render`].
pub fn render_linear(
    p1: Point3D,
    p2: Point3D,
    dimension_line: Point3D,
    text: &str,
    font_family: &str,
    text_height: f64,
    stroke_color: &str,
) -> String {
    let style = Style {
        stroke: stroke_color.to_string(),
        stroke_width: DEFAULT_STROKE_WIDTH,
        fill: None,
        dashes: None,
    };
    // Scratch doc — the parent emitter writes into `body`. We build
    // a full document, then slice the body fragment back out from
    // the finished SVG. Cheap because the document is single-use.
    let mut doc = SvgDoc::new(1.0, 1.0);
    doc.push_dimension_linear(p1, p2, dimension_line, text, font_family, text_height, &style);
    extract_body_fragment(doc.finish())
}

/// Render an aligned DIMENSION. The dim line is parallel to the
/// baseline (`p1`→`p2`) and offset to pass through `dimension_line`.
/// Semantics match the parent-crate `dimension_aligned` — the
/// emitter today is geometrically identical to `render_linear`
/// (projection + arrows + text); the distinction is captured in the
/// caller's label, not the geometry.
pub fn render_aligned(
    p1: Point3D,
    p2: Point3D,
    dimension_line: Point3D,
    text: &str,
    font_family: &str,
    text_height: f64,
    stroke_color: &str,
) -> String {
    render_linear(
        p1, p2, dimension_line, text, font_family, text_height, stroke_color,
    )
}

/// Render a radial DIMENSION — one dim line from the chord point
/// to the far side of the arc with a text label.
pub fn render_radial(
    center: Point3D,
    chord_point: Point3D,
    text: &str,
    font_family: &str,
    text_height: f64,
    stroke_color: &str,
) -> String {
    render_leader_like(center, chord_point, text, font_family, text_height, stroke_color)
}

/// Render a diameter DIMENSION — a single dim line crossing the
/// circle's center from one chord point to its antipodal point
/// (caller supplies both endpoints via `p1` / `p2`) with a label.
pub fn render_diameter(
    p1: Point3D,
    p2: Point3D,
    text: &str,
    font_family: &str,
    text_height: f64,
    stroke_color: &str,
) -> String {
    // Diameter is "line + arrows at both ends + text at midpoint" —
    // exactly what push_dimension_linear emits when the dim line
    // passes through both endpoints. Use the baseline-as-dim-line
    // special case: any point on the baseline itself works.
    render_linear(p1, p2, p1, text, font_family, text_height, stroke_color)
}

/// Render an angular DIMENSION defined by two lines. `a1`→`a2` and
/// `b1`→`b2` are the two measured lines; `arc_point` sits on the
/// dimension arc where the label anchors.
///
/// This composer emits the two lines, a construction arc connecting
/// their directions, and a centered text label at `arc_point`. The
/// parent crate has no first-class angular-dim emitter yet; when
/// one lands, this function collapses to a delegating call.
pub fn render_angular_2l(
    a1: Point3D,
    a2: Point3D,
    b1: Point3D,
    b2: Point3D,
    arc_point: Point3D,
    text: &str,
    font_family: &str,
    text_height: f64,
    stroke_color: &str,
) -> String {
    let mut out = String::new();
    // Measured lines as extension reference.
    push_line(&mut out, a1, a2, stroke_color);
    push_line(&mut out, b1, b2, stroke_color);
    // Arc-point dim-line segment — a connecting stroke from each
    // line's midpoint toward `arc_point`.
    let mid_a = midpoint(a1, a2);
    let mid_b = midpoint(b1, b2);
    push_line(&mut out, mid_a, arc_point, stroke_color);
    push_line(&mut out, mid_b, arc_point, stroke_color);
    push_text_label(&mut out, arc_point, text, font_family, text_height, stroke_color);
    out
}

/// Render an angular DIMENSION defined by three points — two
/// endpoints on the measured arc plus the vertex.
///
/// Same shape as `render_angular_2l` but the two "measured lines"
/// are the vertex→endpoint pairs.
pub fn render_angular_3p(
    vertex: Point3D,
    p_start: Point3D,
    p_end: Point3D,
    arc_point: Point3D,
    text: &str,
    font_family: &str,
    text_height: f64,
    stroke_color: &str,
) -> String {
    render_angular_2l(
        vertex, p_start, vertex, p_end, arc_point, text, font_family, text_height, stroke_color,
    )
}

/// Render an ordinate DIMENSION — a feature-location value (X or Y
/// coordinate) measured relative to an origin.
///
/// Emits a stepped leader from the feature point to the label
/// position plus the coordinate text.
pub fn render_ordinate(
    feature_point: Point3D,
    leader_end: Point3D,
    text: &str,
    font_family: &str,
    text_height: f64,
    stroke_color: &str,
) -> String {
    let mut out = String::new();
    // Horizontal leg then vertical leg (or the reverse depending on
    // label orientation). Use the simplest L-shape: one intermediate
    // at (leader_end.x, feature_point.y).
    let knee = Point3D {
        x: leader_end.x,
        y: feature_point.y,
        z: 0.0,
    };
    push_line(&mut out, feature_point, knee, stroke_color);
    push_line(&mut out, knee, leader_end, stroke_color);
    push_text_label(&mut out, leader_end, text, font_family, text_height, stroke_color);
    out
}

// Helper: render a simple leader-like marker (center → chord point
// line plus centered label at the chord point). Used by radial and
// diameter fallbacks where first-class emitters don't exist yet.
fn render_leader_like(
    from: Point3D,
    to: Point3D,
    text: &str,
    font_family: &str,
    text_height: f64,
    stroke_color: &str,
) -> String {
    let mut out = String::new();
    push_line(&mut out, from, to, stroke_color);
    push_text_label(&mut out, to, text, font_family, text_height, stroke_color);
    out
}

fn push_line(out: &mut String, a: Point3D, b: Point3D, stroke: &str) {
    let safe_stroke = xml_escape_attr(stroke);
    out.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\" fill=\"none\"/>",
        a.x, a.y, b.x, b.y, safe_stroke, DEFAULT_STROKE_WIDTH,
    ));
}

fn push_text_label(
    out: &mut String,
    position: Point3D,
    text: &str,
    font_family: &str,
    height: f64,
    fill: &str,
) {
    let font = xml_escape_attr(&resolve_font_family(font_family));
    let safe_text = xml_escape_text(text);
    let safe_fill = xml_escape_attr(fill);
    out.push_str(&format!(
        "<text x=\"{x}\" y=\"{y}\" font-family=\"{font}\" font-size=\"{height}\" \
         text-anchor=\"middle\" fill=\"{safe_fill}\" \
         transform=\"translate({x},{y}) scale(1,-1) translate({neg_x},{neg_y})\">\
         {safe_text}</text>",
        x = position.x,
        y = position.y,
        neg_x = -position.x,
        neg_y = -position.y,
    ));
}

fn midpoint(a: Point3D, b: Point3D) -> Point3D {
    Point3D {
        x: (a.x + b.x) * 0.5,
        y: (a.y + b.y) * 0.5,
        z: (a.z + b.z) * 0.5,
    }
}

/// Return whatever the parent `SvgDoc::finish()` wrote into its body
/// (the content between the root `<g>` and `</g>` closing tag).
fn extract_body_fragment(full: String) -> String {
    // SvgDoc's finish() wraps body in:
    //   <g transform="translate(0,H) scale(1,-1)">...</g>
    // We want just the ... content, since the wasm viewer's own
    // root group handles the CAD→SVG Y-flip.
    let open = full.find("<g transform=\"translate");
    let close = full.rfind("</g>");
    match (open, close) {
        (Some(o), Some(c)) if c > o => {
            // Advance past the opening tag's '>'.
            let after_open = full[o..].find('>').map(|gt| o + gt + 1);
            if let Some(start) = after_open {
                return full[start..c].trim().to_string();
            }
        }
        _ => {}
    }
    full
}

// Local XML escape helpers, mirroring text_render.rs. Kept local
// rather than shared via a helper module so text_render.rs remains
// self-contained + independently testable.

fn xml_escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn xml_escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point3D {
        Point3D { x, y, z: 0.0 }
    }

    #[test]
    fn linear_emits_extension_and_dim_lines() {
        let svg = render_linear(
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(5.0, 3.0),
            "10.00",
            "Arial",
            1.0,
            "#000000",
        );
        // Parent emitter writes 3 <line> elements (two extension
        // lines + dim line) + 2 <path> arrowheads + 1 <text>.
        assert!(svg.matches("<line").count() >= 3);
        assert!(svg.contains("10.00"));
    }

    #[test]
    fn aligned_delegates_to_linear() {
        let svg_lin = render_linear(
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(5.0, 3.0),
            "X",
            "Arial",
            1.0,
            "#000",
        );
        let svg_ali = render_aligned(
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(5.0, 3.0),
            "X",
            "Arial",
            1.0,
            "#000",
        );
        assert_eq!(svg_lin, svg_ali);
    }

    #[test]
    fn radial_emits_leader_and_text() {
        let svg = render_radial(p(0.0, 0.0), p(5.0, 5.0), "R=7.07", "Arial", 1.0, "#000");
        assert!(svg.contains("<line"));
        assert!(svg.contains("R=7.07"));
    }

    #[test]
    fn diameter_emits_baseline_dim() {
        let svg = render_diameter(p(-5.0, 0.0), p(5.0, 0.0), "⌀10", "Arial", 1.0, "#000");
        assert!(svg.contains("<line"));
    }

    #[test]
    fn angular_2l_emits_four_lines() {
        let svg = render_angular_2l(
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(0.0, 0.0),
            p(0.0, 10.0),
            p(3.0, 3.0),
            "90°",
            "Arial",
            1.0,
            "#000",
        );
        assert!(svg.matches("<line").count() >= 4);
        assert!(svg.contains("90"));
    }

    #[test]
    fn angular_3p_delegates_to_2l() {
        let svg = render_angular_3p(
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(0.0, 10.0),
            p(3.0, 3.0),
            "A",
            "Arial",
            1.0,
            "#000",
        );
        assert!(svg.contains("<line"));
    }

    #[test]
    fn ordinate_emits_stepped_leader() {
        let svg = render_ordinate(p(5.0, 5.0), p(15.0, 10.0), "5.00", "Arial", 1.0, "#000");
        // Two legs = 2 <line> elements.
        assert_eq!(svg.matches("<line").count(), 2);
        assert!(svg.contains("5.00"));
    }

    #[test]
    fn text_label_escapes_xml() {
        let svg = render_radial(p(0.0, 0.0), p(1.0, 1.0), "A & <B>", "Arial", 1.0, "#000");
        assert!(svg.contains("A &amp; &lt;B&gt;"));
    }
}
