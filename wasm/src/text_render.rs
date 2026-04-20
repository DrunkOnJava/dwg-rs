//! V-09 — text rendering with font fallback.
//!
//! Produces a standalone SVG `<text>` fragment for a TEXT entity.
//! Delegates to [`dwg::svg::SvgDoc::push_text`] when the caller has a
//! live document; this module provides the standalone fragment path
//! used by the wasm viewer's `renderTextBlock` JS method (single
//! `<text>` element, no surrounding `<svg>` envelope, no Y-flip — the
//! viewer composes the envelope itself).
//!
//! # Font fallback rule
//!
//! AutoCAD's `.shx` shape files are vector-only formats no browser
//! can render. When the input `font_family` ends in `.shx`
//! (case-insensitive), the emitted `font-family` is
//! `Arial, sans-serif`. Otherwise the supplied family is used verbatim
//! with `, sans-serif` appended as a terminal fallback so any browser
//! without the exact font still renders something readable.
//!
//! This mirrors the parent-crate convention in `dwg::svg` exactly;
//! the two implementations must stay in sync. A unit test in this
//! module pins both sides of the rule so drift is immediately
//! visible.
//!
//! # SVG shape
//!
//! Output is one `<text>` element with:
//!
//! - `x` / `y` attributes in CAD-space coordinates (caller's space).
//! - `font-family` + `font-size` attributes.
//! - `transform="translate(...) scale(1,-1) rotate(deg) translate(...)"`
//!   so the text reads right-side-up under the viewer's CAD→SVG
//!   Y-flip, matching `SvgDoc::push_text`'s counter-flip discipline.
//! - XML-escaped text content.
//!
//! # Errors
//!
//! None at this layer. The function returns a `String` directly;
//! malformed input produces best-effort output with escaped content.

use dwg::entities::Point3D;

/// Render a single TEXT block as a standalone `<text>` SVG fragment.
///
/// `position.x`/`.y` are in CAD coordinates (Y-up). `height` is the
/// text height in CAD units → emitted as `font-size`. `rotation` is
/// in radians; emitted as degrees in the `transform` attribute.
/// `font_family` follows the `.shx → Arial` fallback rule; any other
/// family is passed through with `, sans-serif` as terminal fallback.
pub fn render_text_block(
    text: &str,
    position: Point3D,
    height: f64,
    rotation_radians: f64,
    font_family: &str,
) -> String {
    let resolved_font = resolve_font_family(font_family);
    let rotation_deg = rotation_radians.to_degrees();
    let escaped = xml_escape_text(text);
    let font_attr = xml_escape_attr(&resolved_font);
    // Counter-flip Y so glyphs read right-side-up under the CAD→SVG
    // Y-flip at the viewer's root group. Mirrors SvgDoc::push_text.
    format!(
        "<text x=\"{x}\" y=\"{y}\" font-family=\"{font_attr}\" font-size=\"{height}\" \
         transform=\"translate({x},{y}) scale(1,-1) rotate({neg_deg}) translate({neg_x},{neg_y})\">\
         {escaped}</text>",
        x = position.x,
        y = position.y,
        neg_deg = -rotation_deg,
        neg_x = -position.x,
        neg_y = -position.y,
    )
}

/// Resolve a DWG font family into a browser-safe CSS `font-family`.
///
/// `.shx` → `Arial, sans-serif` (SHX is not renderable in browsers).
/// Everything else is passed through with `, sans-serif` appended so
/// a missing font still produces legible glyphs.
pub fn resolve_font_family(family: &str) -> String {
    let trimmed = family.trim();
    if trimmed.is_empty() {
        return "Arial, sans-serif".to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with(".shx") {
        return "Arial, sans-serif".to_string();
    }
    format!("{trimmed}, sans-serif")
}

/// Minimal XML escape for element content (`&`, `<`, `>`).
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

/// Minimal XML escape for attribute values (adds quote escapes).
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

    #[test]
    fn shx_family_falls_back_to_arial() {
        assert_eq!(resolve_font_family("txt.shx"), "Arial, sans-serif");
        assert_eq!(resolve_font_family("ROMANS.SHX"), "Arial, sans-serif");
        assert_eq!(resolve_font_family("CustomFont.Shx"), "Arial, sans-serif");
    }

    #[test]
    fn non_shx_family_passes_through_with_terminal_fallback() {
        assert_eq!(resolve_font_family("Arial"), "Arial, sans-serif");
        assert_eq!(resolve_font_family("Times New Roman"), "Times New Roman, sans-serif");
    }

    #[test]
    fn empty_family_uses_arial() {
        assert_eq!(resolve_font_family(""), "Arial, sans-serif");
        assert_eq!(resolve_font_family("   "), "Arial, sans-serif");
    }

    #[test]
    fn render_text_block_emits_text_element() {
        let p = Point3D { x: 10.0, y: 20.0, z: 0.0 };
        let svg = render_text_block("Hello", p, 5.0, 0.0, "Arial");
        assert!(svg.starts_with("<text"));
        assert!(svg.ends_with("</text>"));
        assert!(svg.contains("font-size=\"5\""));
        assert!(svg.contains("font-family=\"Arial, sans-serif\""));
        assert!(svg.contains(">Hello</text>"));
    }

    #[test]
    fn render_text_block_escapes_xml_content() {
        let p = Point3D::default();
        let svg = render_text_block("A & <B>", p, 1.0, 0.0, "Arial");
        assert!(svg.contains("A &amp; &lt;B&gt;"));
    }

    #[test]
    fn render_text_block_shx_falls_back() {
        let p = Point3D::default();
        let svg = render_text_block("X", p, 1.0, 0.0, "ROMANS.SHX");
        assert!(svg.contains("font-family=\"Arial, sans-serif\""));
    }

    #[test]
    fn render_text_block_applies_rotation_in_degrees() {
        let p = Point3D { x: 0.0, y: 0.0, z: 0.0 };
        let svg = render_text_block("X", p, 1.0, std::f64::consts::FRAC_PI_2, "Arial");
        // 90 degrees → "-90" in the counter-flipped rotate()
        assert!(svg.contains("rotate(-90"));
    }
}
