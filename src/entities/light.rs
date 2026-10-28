//! LIGHT entity: the point, spot and distant lights of AutoCAD's rendering.
//!
//! ODA spec v5.4.1 prints no layout for LIGHT. The field list is derived from AutoCAD's own
//! output under the method in `CLEANROOM.md` (2026-10-28, ARCHITECTURE.md §7br): the one
//! LIGHT record of the pinned corpus, a point light in `3d.dwg` (AC1014), closes exactly
//! on its data-stream boundary with it. The corpus holds no DXF export of that drawing, so no
//! value is scored and the state is `decoder`.
//!
//! # Stream shape
//!
//! ```text
//! BL   version                 -- 1
//! TV   name                    -- inline before R2007, in the string stream from R2007
//! BL   light_type              -- 2 on the point light
//! B    is_on
//! CMC  colour                  -- BS before R2004; as the ACSH nodes have it from R2004
//! B    plot_glyph
//! BD   x7                      -- intensity, position (3), target (3)
//! BL   attenuation_type
//! B    use_attenuation_limits
//! BD   x4                      -- attenuation start and end, hotspot and falloff angles
//! B    cast_shadows
//! BL   shadow_type
//! BS   shadow_map_size
//! RC   shadow_map_softness
//! ```
//!
//! What the bytes settle is the width and order of every field: the list ends on the
//! boundary, and the values read as plain numbers (a 45 degree hotspot and a 50 degree
//! falloff as radians, attenuation limits 1 and 10, a shadow map of 256). What they do not
//! settle is how the seven `BD` of the middle split: the record closes
//! equally with the intensity first (the order Autodesk's public DXF Reference lists the
//! groups in, recalled) and with the position and target first, so that grouping is
//! unverified. The point light carries a target as well as a position, and the light type
//! 2 is the point light by the light's own name; types 1, 3 and 4 are not in the corpus.
//!
//! From R2004 the colour takes the form [`crate::objects::sh_history::ShColor`] reads, which the
//! ACSH nodes and DBCOLOR measure; no LIGHT record of that release is in the corpus.
//!
//! # The name comes from the string stream from R2007
//!
//! From R2007 every `TV` moves out of the data stream (§19.1), so the name is read from the
//! object's string stream there ([`decode_modern_split_stream`]) and inline before it.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::objects::sh_history::{ShColor, read_color};
use crate::string_stream::{self, StringReader};
use crate::tables::modern;
use crate::version::Version;

/// Discriminator for the kind of light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightType {
    Distant,
    Point,
    Spot,
    Web,
    /// Any other code: the raw value is kept so callers can see what they are dealing with.
    Unknown(u32),
}

impl LightType {
    /// Maps the light-type code (1 distant, 2 point, 3 spot, 4 web); only 2 is observed in
    /// the corpus. Unknown codes are preserved as `Unknown`.
    pub fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Distant,
            2 => Self::Point,
            3 => Self::Spot,
            4 => Self::Web,
            other => Self::Unknown(other),
        }
    }
}

/// Decoded LIGHT entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    /// `BL`: 1 in the corpus record.
    pub version: u32,
    pub name: String,
    pub light_type: LightType,
    pub is_on: bool,
    /// The light's colour (an ACI index of 7 in the corpus record).
    pub color: ShColor,
    pub plot_glyph: bool,
    /// The first of the seven `BD` after the plot glyph (5.0 in the corpus record); unverified,
    /// see the module docs.
    pub intensity: f64,
    /// The next three `BD` (20, -50, 50 in the corpus record).
    pub position: Point3D,
    /// The last three `BD` of the seven (20, -50, 40 in the corpus record).
    pub target: Point3D,
    /// `BL`: 0 in the corpus record.
    pub attenuation_type: u32,
    pub use_attenuation_limits: bool,
    /// `BD`: 1.0 in the corpus record.
    pub attenuation_start: f64,
    /// `BD`: 10.0 in the corpus record.
    pub attenuation_end: f64,
    /// `BD`, in radians: 0.7853981633974483 (45 degrees) in the corpus record.
    pub hotspot_angle: f64,
    /// `BD`, in radians: 0.8726646259971648 (50 degrees) in the corpus record.
    pub falloff_angle: f64,
    pub cast_shadows: bool,
    /// `BL`: 0 in the corpus record.
    pub shadow_type: u32,
    /// `BS`: 256 in the corpus record.
    pub shadow_map_size: i16,
    /// `RC`: 1 in the corpus record.
    pub shadow_map_softness: u8,
}

/// Decode a LIGHT payload from an inline stream.
///
/// The cursor must already be positioned past the common entity preamble.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Light> {
    read_body(c, &mut None, version)
}

/// Decode a LIGHT from its raw payload, taking the light name from the
/// object's R2007+ string stream (§19.1) and checking the data fields
/// end exactly where that stream begins.
pub fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<Light> {
    let (strings, string_start) = modern::open_entity(payload, version)?;
    let mut strings = Some(strings);
    let mut c = BitCursor::new(payload);
    string_stream::seek(&mut c, object_body_start)?;
    crate::common_entity::read_common_entity_data(&mut c, version)?;
    let light = read_body(&mut c, &mut strings, version)?;
    let at = c.position_bits();
    if at != string_start {
        return Err(modern::misaligned("LIGHT", at, string_start));
    }
    Ok(light)
}

/// Read the LIGHT field list of the module docs from whichever streams hold it: `strings`
/// is the R2007+ string stream, or `None` on the inline layout.
pub(crate) fn read_body(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<Light> {
    let version_field = c.read_bl_u()?;
    let name = crate::objects::modern::read_tv(c, strings, version)?;
    let light_type = LightType::from_code(c.read_bl_u()?);
    let is_on = c.read_b()?;
    let color = read_color(c, strings, version)?;
    let plot_glyph = c.read_b()?;
    let intensity = c.read_bd()?;
    let position = read_bd3(c)?;
    let target = read_bd3(c)?;
    let attenuation_type = c.read_bl_u()?;
    let use_attenuation_limits = c.read_b()?;
    let attenuation_start = c.read_bd()?;
    let attenuation_end = c.read_bd()?;
    let hotspot_angle = c.read_bd()?;
    let falloff_angle = c.read_bd()?;
    let cast_shadows = c.read_b()?;
    let shadow_type = c.read_bl_u()?;
    let shadow_map_size = c.read_bs()?;
    let shadow_map_softness = c.read_rc()?;
    Ok(Light {
        version: version_field,
        name,
        light_type,
        is_on,
        color,
        plot_glyph,
        intensity,
        position,
        target,
        attenuation_type,
        use_attenuation_limits,
        attenuation_start,
        attenuation_end,
        hotspot_angle,
        falloff_angle,
        cast_shadows,
        shadow_type,
        shadow_map_size,
        shadow_map_softness,
    })
}
