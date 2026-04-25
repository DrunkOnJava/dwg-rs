//! V-07 — linetype pattern → SVG stroke-dasharray.
//!
//! DWG LTYPE entries carry an alternating dash / gap / dot pattern
//! encoded as signed-f64 lengths: positive = dash, negative = gap,
//! zero = dot. SVG's `stroke-dasharray` expects unsigned lengths
//! with implicit alternation (first value = dash length, second =
//! gap length, third = dash, etc.). This module converts between
//! the two representations.

use wasm_bindgen::prelude::*;

/// Convert a DWG LTYPE pattern to an SVG stroke-dasharray string.
///
/// Input: alternating signed lengths `[dash_or_gap, ...]` where
/// positive values are dashes, negative are gaps, and zero-valued
/// entries are rendered as dots (SVG: a pair `(0.001, gap_length)`
/// approximates a dot).
///
/// Output: space-separated unsigned length list suitable for the
/// SVG `stroke-dasharray` attribute. The first element is always a
/// dash; if the input starts with a gap (negative), a leading
/// zero-length dash is prepended so SVG's implicit alternation
/// stays correct.
#[wasm_bindgen(js_name = "linetypeToDasharray")]
pub fn linetype_to_dasharray(pattern: &[f64]) -> String {
    if pattern.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut first = true;
    for &v in pattern {
        if first && v < 0.0 {
            out.push('0');
            first = false;
        }
        if !first {
            out.push(' ');
        }
        first = false;
        if v > 0.0 {
            out.push_str(&format_length(v));
        } else if v < 0.0 {
            out.push_str(&format_length(-v));
        } else {
            // Dot — SVG has no dedicated dot, emit a tiny dash.
            out.push_str("0.001");
        }
    }
    out
}

/// Format an f64 length for SVG attribute output. Trims trailing
/// zeros and uses fixed notation (no scientific) for values that
/// would otherwise print as `1e-5` style.
fn format_length(v: f64) -> String {
    if v.abs() < 0.01 {
        format!("{v:.4}")
    } else if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pattern_empty_output() {
        assert_eq!(linetype_to_dasharray(&[]), "");
    }

    #[test]
    fn continuous_dash_is_just_number() {
        assert_eq!(linetype_to_dasharray(&[1.0]), "1");
    }

    #[test]
    fn simple_dashed_pattern() {
        // 0.5 dash, 0.25 gap
        assert_eq!(linetype_to_dasharray(&[0.5, -0.25]), "0.5 0.25");
    }

    #[test]
    fn gap_first_pattern_prepends_zero_dash() {
        assert_eq!(linetype_to_dasharray(&[-0.25, 0.5]), "0 0.25 0.5");
    }

    #[test]
    fn dash_dot_dash_pattern() {
        // Classic DASHDOT: 0.5 dash, 0.25 gap, 0 dot, 0.25 gap
        assert_eq!(
            linetype_to_dasharray(&[0.5, -0.25, 0.0, -0.25]),
            "0.5 0.25 0.001 0.25"
        );
    }

    #[test]
    fn integer_lengths_no_decimal_noise() {
        assert_eq!(linetype_to_dasharray(&[2.0, -1.0, 1.0, -1.0]), "2 1 1 1");
    }

    #[test]
    fn long_pattern_preserved() {
        let pattern = [1.0, -0.5, 0.25, -0.5, 0.0, -0.5];
        let out = linetype_to_dasharray(&pattern);
        assert_eq!(out.split(' ').count(), 6);
    }
}
