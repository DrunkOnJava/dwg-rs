//! MLEADER entity (§19.4.22) — multileader (modern leader).
//!
//! MLEADER replaces LEADER in R2008+ drawings. It encodes a leader
//! line (possibly spline-based), an optional block or text
//! attachment, dogleg geometry, and arrowhead + line style
//! references. The full stream is ~60 fields with deep nesting
//! (per-leader-line parameters, per-dogleg parameters, per-block-
//! transform parameters).
//!
//! This header-level decoder captures the class version, content
//! type, leader-line style, scale, and block-reference fields —
//! enough to classify a multileader and identify its style; leader
//! geometry is preserved in the raw object bytes.
//!
//! # Stream shape (partial)
//!
//! ```text
//! BS    class_version        -- 0 or 1
//! BS    content_type          -- 1 = block, 2 = text, 3 = tolerance
//! BS    leader_line_type      -- 0 = invisible, 1 = straight, 2 = spline
//! BL    leader_line_color_raw
//! BD    arrow_size
//! BD    landing_gap
//! BS    leader_line_weight    -- lineweight enum
//! B     enable_landing
//! B     enable_dogleg
//! BD    dogleg_length
//! ```

use crate::bitcursor::BitCursor;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct MLeader {
    pub class_version: i16,
    pub content_type: i16,
    pub leader_line_type: i16,
    pub leader_line_color: u32,
    pub arrow_size: f64,
    pub landing_gap: f64,
    pub leader_line_weight: i16,
    pub enable_landing: bool,
    pub enable_dogleg: bool,
    pub dogleg_length: f64,
}

pub fn decode_header(c: &mut BitCursor<'_>) -> Result<MLeader> {
    let class_version = c.read_bs()?;
    let content_type = c.read_bs()?;
    let leader_line_type = c.read_bs()?;
    let leader_line_color = c.read_bl()? as u32;
    let arrow_size = c.read_bd()?;
    let landing_gap = c.read_bd()?;
    let leader_line_weight = c.read_bs()?;
    let enable_landing = c.read_b()?;
    let enable_dogleg = c.read_b()?;
    let dogleg_length = c.read_bd()?;
    Ok(MLeader {
        class_version,
        content_type,
        leader_line_type,
        leader_line_color,
        arrow_size,
        landing_gap,
        leader_line_weight,
        enable_landing,
        enable_dogleg,
        dogleg_length,
    })
}

pub use decode_header as decode;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_mleader_header_text() {
        let mut w = BitWriter::new();
        w.write_bs(1); // class version
        w.write_bs(2); // text content
        w.write_bs(1); // straight
        w.write_bl(256); // ByLayer color
        w.write_bd(0.18);
        w.write_bd(0.09);
        w.write_bs(-2);
        w.write_b(true);
        w.write_b(true);
        w.write_bd(0.5);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c).unwrap();
        assert_eq!(m.class_version, 1);
        assert_eq!(m.content_type, 2);
        assert_eq!(m.leader_line_type, 1);
        assert!(m.enable_landing);
        assert!(m.enable_dogleg);
        assert_eq!(m.dogleg_length, 0.5);
    }
}
