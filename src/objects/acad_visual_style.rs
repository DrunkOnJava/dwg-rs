//! ACAD_VISUALSTYLE object (spec §19.6.10 — L6-17) — named display
//! style (face lighting model, edge rendering, silhouette, etc.).
//! R2007+ only.
//!
//! Visual styles are the R2007+ replacement for the earlier
//! "shademode" enumeration. Each style packages face-shading
//! parameters (lighting model, opacity, monochrome colour), edge
//! rendering parameters (intersection / obscured / silhouette
//! colours, edge model), and shadow flags. The full on-wire stream
//! includes upwards of 50 fields; this decoder reads a **documented
//! subset** — the most commonly-used face + edge base properties —
//! and stops.
//!
//! # Field cutoff
//!
//! After `edge_silhouette_color` the stream continues with
//! halo-gap, crease/intersection/obscured line style, jitter, wiggle,
//! silhouette line width, halo intensity, and display settings —
//! each of which is usable only in conjunction with sibling fields
//! not surfaced here. A full decoder belongs in a dedicated
//! extension; this subset is sufficient for matching a visual style
//! by name and extracting the handful of properties that most
//! consumers actually use (base face colour mode, silhouette
//! colour, edge model).
//!
//! # Stream shape (subset decoded)
//!
//! ```text
//! TV   description
//! BS   face_lighting_model
//! BS   face_lighting_quality
//! BS   face_color_mode
//! BD   face_opacity
//! BD   face_specular
//! BS   face_mono_color
//! RC   face_modifier
//! BS   edge_model
//! BL   edge_style_apply_flags
//! BS   edge_intersection_color
//! BS   edge_obscured_color
//! BS   edge_color
//! BS   edge_silhouette_color
//! ```
//!
//! Returns [`Error::Unsupported`] for pre-R2007 versions.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::tables::read_tv;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct AcadVisualStyle {
    pub description: String,
    pub face_lighting_model: i16,
    pub face_lighting_quality: i16,
    pub face_color_mode: i16,
    pub face_opacity: f64,
    pub face_specular: f64,
    pub face_mono_color: i16,
    pub face_modifier: u8,
    pub edge_model: i16,
    pub edge_style_apply_flags: i32,
    pub edge_intersection_color: i16,
    pub edge_obscured_color: i16,
    pub edge_color: i16,
    pub edge_silhouette_color: i16,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AcadVisualStyle> {
    if !version.is_r2007_plus() {
        return Err(Error::Unsupported {
            feature: format!(
                "ACAD_VISUALSTYLE requires R2007 or newer; got {}",
                version.release()
            ),
        });
    }
    let description = read_tv(c, version)?;
    let face_lighting_model = c.read_bs()?;
    let face_lighting_quality = c.read_bs()?;
    let face_color_mode = c.read_bs()?;
    let face_opacity = c.read_bd()?;
    let face_specular = c.read_bd()?;
    let face_mono_color = c.read_bs()?;
    let face_modifier = c.read_rc()?;
    let edge_model = c.read_bs()?;
    let edge_style_apply_flags = c.read_bl()?;
    let edge_intersection_color = c.read_bs()?;
    let edge_obscured_color = c.read_bs()?;
    let edge_color = c.read_bs()?;
    let edge_silhouette_color = c.read_bs()?;
    Ok(AcadVisualStyle {
        description,
        face_lighting_model,
        face_lighting_quality,
        face_color_mode,
        face_opacity,
        face_specular,
        face_mono_color,
        face_modifier,
        edge_model,
        edge_style_apply_flags,
        edge_intersection_color,
        edge_obscured_color,
        edge_color,
        edge_silhouette_color,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn encode_tv_utf16(w: &mut BitWriter, s: &str) {
        let units: Vec<u16> = s.encode_utf16().collect();
        w.write_bs_u(units.len() as u16);
        for u in units {
            w.write_rc((u & 0xFF) as u8);
            w.write_rc((u >> 8) as u8);
        }
    }

    #[test]
    fn rejects_pre_r2007() {
        let bytes = [0u8; 32];
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(
            matches!(&err, Error::Unsupported { feature } if feature.contains("ACAD_VISUALSTYLE"))
        );
    }

    #[test]
    fn roundtrip_shaded_visual_style_r2013() {
        let mut w = BitWriter::new();
        encode_tv_utf16(&mut w, "Shaded with edges");
        w.write_bs(1); // face_lighting_model (1 = Phong)
        w.write_bs(1); // face_lighting_quality (1 = Smooth)
        w.write_bs(2); // face_color_mode (2 = Desaturate)
        w.write_bd(1.0); // face_opacity
        w.write_bd(0.5); // face_specular
        w.write_bs(7); // face_mono_color (ACI white)
        w.write_rc(0); // face_modifier
        w.write_bs(1); // edge_model (1 = isolines)
        w.write_bl(0x00FF); // edge_style_apply_flags
        w.write_bs(7); // edge_intersection_color
        w.write_bs(8); // edge_obscured_color
        w.write_bs(256); // edge_color (ByLayer sentinel)
        w.write_bs(3); // edge_silhouette_color
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let v = decode(&mut c, Version::R2013).unwrap();
        assert_eq!(v.description, "Shaded with edges");
        assert_eq!(v.face_lighting_model, 1);
        assert_eq!(v.face_color_mode, 2);
        assert!((v.face_opacity - 1.0).abs() < 1e-12);
        assert!((v.face_specular - 0.5).abs() < 1e-12);
        assert_eq!(v.edge_style_apply_flags, 0x00FF);
        assert_eq!(v.edge_silhouette_color, 3);
    }
}
