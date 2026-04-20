//! Measurement tools for V-22 — Euclidean distance + polygon area.
//!
//! Pure Rust math surfaced through wasm-bindgen for the browser
//! viewer. Callers supply drawing-space coordinates (model-space or
//! paper-space; the caller is responsible for consistency — the
//! measurement functions don't apply any viewport transform).
//!
//! Distances return drawing units. Polygon areas use the shoelace
//! formula and return unsigned drawing-units². Winding order is
//! auto-detected (sign of the shoelace sum) so either CW or CCW
//! polygons produce a positive area.

use wasm_bindgen::prelude::*;

/// Euclidean distance between two 2D points in drawing units.
///
/// ```text
/// d = √((x2 − x1)² + (y2 − y1)²)
/// ```
#[wasm_bindgen(js_name = "measureDistance2D")]
pub fn measure_distance_2d(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    (dx * dx + dy * dy).sqrt()
}

/// Euclidean distance between two 3D points in drawing units.
#[wasm_bindgen(js_name = "measureDistance3D")]
pub fn measure_distance_3d(
    x1: f64,
    y1: f64,
    z1: f64,
    x2: f64,
    y2: f64,
    z2: f64,
) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let dz = z2 - z1;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Signed shoelace area of a 2D polygon given as interleaved
/// `[x0, y0, x1, y1, ..., xN, yN]` coordinates.
///
/// Returns the unsigned magnitude — winding order (CW / CCW) does
/// not affect the reported area.
///
/// Returns `0.0` if the input has fewer than 3 points (not a
/// polygon), or if the input length is odd (malformed coord pair).
#[wasm_bindgen(js_name = "measurePolygonArea")]
pub fn measure_polygon_area(xys: &[f64]) -> f64 {
    if xys.len() < 6 || xys.len() % 2 != 0 {
        return 0.0;
    }
    let n = xys.len() / 2;
    let mut sum: f64 = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        let xi = xys[2 * i];
        let yi = xys[2 * i + 1];
        let xj = xys[2 * j];
        let yj = xys[2 * j + 1];
        sum += xi * yj - xj * yi;
    }
    (sum / 2.0).abs()
}

/// Total polyline length: sum of Euclidean distances between
/// consecutive points. Returns 0.0 for < 2 points or malformed
/// (odd-length) input.
#[wasm_bindgen(js_name = "measurePolylineLength")]
pub fn measure_polyline_length(xys: &[f64]) -> f64 {
    if xys.len() < 4 || xys.len() % 2 != 0 {
        return 0.0;
    }
    let n = xys.len() / 2;
    let mut total: f64 = 0.0;
    for i in 0..(n - 1) {
        let dx = xys[2 * (i + 1)] - xys[2 * i];
        let dy = xys[2 * (i + 1) + 1] - xys[2 * i + 1];
        total += (dx * dx + dy * dy).sqrt();
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_2d_pythagorean_3_4_5() {
        assert!((measure_distance_2d(0.0, 0.0, 3.0, 4.0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn distance_3d_classic_1_2_2_triple() {
        // (0,0,0) to (1,2,2) = √(1+4+4) = 3
        assert!((measure_distance_3d(0.0, 0.0, 0.0, 1.0, 2.0, 2.0) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn polygon_area_unit_square_is_one() {
        let sq = [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        assert!((measure_polygon_area(&sq) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn polygon_area_reversed_winding_still_positive() {
        let cw = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        assert!((measure_polygon_area(&cw) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn polygon_area_triangle_half_base_times_height() {
        // Right triangle with legs 3 and 4, area = 6.
        let tri = [0.0, 0.0, 3.0, 0.0, 0.0, 4.0];
        assert!((measure_polygon_area(&tri) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn polygon_area_rejects_degenerate_inputs() {
        assert_eq!(measure_polygon_area(&[]), 0.0);
        assert_eq!(measure_polygon_area(&[1.0, 2.0]), 0.0); // 1 point
        assert_eq!(measure_polygon_area(&[1.0, 2.0, 3.0, 4.0]), 0.0); // 2 points
        assert_eq!(measure_polygon_area(&[1.0, 2.0, 3.0, 4.0, 5.0]), 0.0); // odd len
    }

    #[test]
    fn polyline_length_accumulates_segments() {
        // (0,0)-(3,0)-(3,4) = 3 + 4 = 7
        let poly = [0.0, 0.0, 3.0, 0.0, 3.0, 4.0];
        assert!((measure_polyline_length(&poly) - 7.0).abs() < 1e-9);
    }

    #[test]
    fn polyline_length_rejects_degenerate() {
        assert_eq!(measure_polyline_length(&[]), 0.0);
        assert_eq!(measure_polyline_length(&[1.0, 2.0]), 0.0);
        assert_eq!(measure_polyline_length(&[1.0, 2.0, 3.0]), 0.0);
    }
}
