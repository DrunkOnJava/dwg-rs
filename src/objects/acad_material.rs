//! ACAD_MATERIAL object (spec §19.6.9 — L6-16) — rendering material
//! definition. R2007+ only.
//!
//! MATERIAL records describe how a surface is shaded during a
//! rendered-viewport plot: diffuse/ambient/specular terms, bump
//! mapping amplitude, refraction index, self-illumination, etc.
//! The full stream runs to ~65 fields including per-channel texture
//! sub-records; this decoder reads the **first ~25 most-rendering-
//! relevant fields** and stops.
//!
//! # Field cutoff
//!
//! The cutoff is deliberate: once `luminance_mode` is consumed, the
//! remaining fields are all texture sub-records (diffuse map, bump
//! map, reflection map, opacity map, specular map, refraction map,
//! normal map, …) each of which is itself a multi-field struct with
//! version-gated extensions. A proper decoder for those belongs in
//! a dedicated `MaterialTextures` module; mixing them into the
//! base-property decoder would triple the surface area for a first
//! cut. The fields captured here are sufficient to drive a flat
//! shading preview or a DXF export of the material's base colour
//! properties.
//!
//! # Stream shape (subset decoded)
//!
//! ```text
//! TV   name
//! TV   description
//! BL   ambient_color_method       -- 0=ByObject, 1=Override
//! BS   ambient_color              -- ACI index (simplified CMC)
//! BD   ambient_color_factor
//! BL   diffuse_color_method
//! BS   diffuse_color
//! BD   diffuse_color_factor
//! BD   specular_color_factor
//! BD   specular_gloss_factor
//! BD   reflection_factor
//! BD   opacity_factor
//! BD   bump_amount
//! BD   refraction_factor
//! BD   translucence
//! BL   self_illumination
//! BD   luminance
//! BL   luminance_mode
//! ```
//!
//! Returns [`Error::Unsupported`] for pre-R2007 versions.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::tables::read_tv;
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct AcadMaterial {
    pub name: String,
    pub description: String,
    pub ambient_color_method: i32,
    pub ambient_color: i16,
    pub ambient_color_factor: f64,
    pub diffuse_color_method: i32,
    pub diffuse_color: i16,
    pub diffuse_color_factor: f64,
    pub specular_color_factor: f64,
    pub specular_gloss_factor: f64,
    pub reflection_factor: f64,
    pub opacity_factor: f64,
    pub bump_amount: f64,
    pub refraction_factor: f64,
    pub translucence: f64,
    pub self_illumination: i32,
    pub luminance: f64,
    pub luminance_mode: i32,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AcadMaterial> {
    if !version.is_r2007_plus() {
        return Err(Error::Unsupported {
            feature: format!(
                "ACAD_MATERIAL requires R2007 or newer; got {}",
                version.release()
            ),
        });
    }
    let name = read_tv(c, version)?;
    let description = read_tv(c, version)?;
    let ambient_color_method = c.read_bl()?;
    let ambient_color = c.read_bs()?;
    let ambient_color_factor = c.read_bd()?;
    let diffuse_color_method = c.read_bl()?;
    let diffuse_color = c.read_bs()?;
    let diffuse_color_factor = c.read_bd()?;
    let specular_color_factor = c.read_bd()?;
    let specular_gloss_factor = c.read_bd()?;
    let reflection_factor = c.read_bd()?;
    let opacity_factor = c.read_bd()?;
    let bump_amount = c.read_bd()?;
    let refraction_factor = c.read_bd()?;
    let translucence = c.read_bd()?;
    let self_illumination = c.read_bl()?;
    let luminance = c.read_bd()?;
    let luminance_mode = c.read_bl()?;
    Ok(AcadMaterial {
        name,
        description,
        ambient_color_method,
        ambient_color,
        ambient_color_factor,
        diffuse_color_method,
        diffuse_color,
        diffuse_color_factor,
        specular_color_factor,
        specular_gloss_factor,
        reflection_factor,
        opacity_factor,
        bump_amount,
        refraction_factor,
        translucence,
        self_illumination,
        luminance,
        luminance_mode,
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
        let bytes = [0u8; 16];
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(
            matches!(&err, Error::Unsupported { feature } if feature.contains("ACAD_MATERIAL"))
        );
    }

    #[test]
    fn roundtrip_plastic_material_r2010() {
        let mut w = BitWriter::new();
        // R2010 is in the is_r2007_plus / uses_utf16_text family.
        encode_tv_utf16(&mut w, "Plastic - Red");
        encode_tv_utf16(&mut w, "Smooth red plastic");
        w.write_bl(1); // ambient_color_method (1 = Override)
        w.write_bs(1); // ambient_color (ACI red)
        w.write_bd(1.0); // ambient_color_factor
        w.write_bl(1); // diffuse_color_method
        w.write_bs(1); // diffuse_color
        w.write_bd(1.0); // diffuse_color_factor
        w.write_bd(0.5); // specular_color_factor
        w.write_bd(0.85); // specular_gloss_factor
        w.write_bd(0.0); // reflection_factor
        w.write_bd(1.0); // opacity_factor (fully opaque)
        w.write_bd(0.0); // bump_amount
        w.write_bd(1.5); // refraction_factor
        w.write_bd(0.0); // translucence
        w.write_bl(0); // self_illumination
        w.write_bd(0.0); // luminance
        w.write_bl(0); // luminance_mode
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let m = decode(&mut c, Version::R2010).unwrap();
        assert_eq!(m.name, "Plastic - Red");
        assert_eq!(m.description, "Smooth red plastic");
        assert_eq!(m.ambient_color_method, 1);
        assert_eq!(m.diffuse_color, 1);
        assert!((m.specular_gloss_factor - 0.85).abs() < 1e-12);
        assert!((m.opacity_factor - 1.0).abs() < 1e-12);
        assert!((m.refraction_factor - 1.5).abs() < 1e-12);
    }
}
