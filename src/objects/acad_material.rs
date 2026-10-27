//! MATERIAL object (`AcDbMaterial`) — a rendering material.
//!
//! ODA spec v5.4.1 lists `MATERIAL` in §20.3's table of non-fixed object
//! types and prescribes no field list for it: §20.4 runs from 20.4.1 to
//! 20.4.104 and has no entry. The layout here is derived from AutoCAD's
//! own output under the method recorded in `CLEANROOM.md` (2026-10-26):
//! the data bits of every MATERIAL record in the pinned public corpus,
//! compared with the `AcDbMaterial` object AutoCAD exported to DXF for the
//! same handle. ARCHITECTURE.md §7au holds the table of what each field was
//! checked against.
//!
//! # Field list — derived, 212 records on seven releases
//!
//! ```text
//! TV    name                                  -- group 1
//! TV    description                           -- group 2
//! colour group x3 (ambient, diffuse, specular):
//!   RC  method (0 or 1)                       -- groups 70, 71, 76
//!   BD  factor                                -- always 1.0, never in the DXF
//!   BL  colour, only when method is 1         -- groups 90, 91, 92
//! map block x6 (diffuse, specular, reflection, opacity, bump, refraction):
//!   BD  blend
//!   RC  x3  unknown (2, 1, 1 in every record)
//!   BD  x16 transform matrix
//!   RC  source (0 none, 1 file)               -- groups 72, 77, 171, 175, 179
//!   TV  file name, only when source is 1      -- groups 3 and 8 seen
//! ```
//!
//! The colour groups and map blocks interleave as: ambient, diffuse,
//! diffuse map, specular, specular map, `BD` gloss (group 44), reflection
//! map, `BD` opacity (group 140), opacity map, bump map, `BD` refraction
//! index (group 145), refraction map. From R2007 the list ends with `BD`
//! (group 148), one `BD` that is always zero, `BD` (group 468), `BS` (group
//! 93), `BS` (group 94, the channel flags) and one more zero `BD`.
//! R14, R2000 and R2004 have no tail. Every release has the same list up to
//! it; before R2007 the `TV`s sit inline in the data stream, from R2007 in
//! the string stream.
//!
//! # What is and is not claimed
//!
//! A field is checked by value when the DXF writes its group in some
//! record; the field names say which. Fields the DXF never shows in this
//! corpus (the three colour factors, five of the six map blends, the three
//! `RC` after each blend, the matrix and file of the maps no record
//! gives a non-default value, the two always-zero tail `BD`) have their
//! width claimed and nothing else, and are named `unknown_*` or documented
//! as such. Their layout was observed for the values 0 and 1 only: a colour
//! method or a map source other than 0 or 1, and a tail slot that is not
//! the 2-bit zero code, are refused with [`Error::Unsupported`] rather than
//! read on a guess.
//!
//! The three colour groups (90, 91, 92) and the three method groups (70,
//! 71, 76) carry the same value in every record, so which of ambient,
//! diffuse and specular owns which group is positional, not corroborated
//! by value. The same holds for the six map blocks' matrices where the
//! DXF writes the same matrix for several maps.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

/// One of the three colour groups of a MATERIAL (ambient, diffuse,
/// specular).
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialColor {
    /// `RC` colour method: 0 leaves the colour unset, 1 overrides it.
    /// DXF groups 70 / 71 / 76.
    pub method: u8,
    /// `BD` at the position of the colour factor. 1.0 in every corpus
    /// record and written by no DXF group there: unverified by value.
    pub unknown_factor: f64,
    /// `BL` colour word, present only when `method` is 1. DXF groups
    /// 90 / 91 / 92, compared as a signed 32-bit integer.
    pub color: Option<u32>,
}

/// One of the six map blocks of a MATERIAL.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialMap {
    /// `BD` at the head of the block. Verified against DXF group 143 on
    /// the bump map; 1.0 and unverified on the other five.
    pub blend: f64,
    /// Three `RC` after the blend, 2, 1 and 1 in every corpus record. No
    /// DXF group shows them: unverified by value, and the split of the 24
    /// bits into three bytes is itself unverified.
    pub unknown_a: u8,
    pub unknown_b: u8,
    pub unknown_c: u8,
    /// The 4x4 transform matrix as 16 `BD`, DXF groups 43, 47, 49, 142,
    /// 144 and 147 for the six maps in block order.
    pub matrix: [f64; 16],
    /// `RC` map source: 0 none, 1 file. DXF groups 72, 77, 171, 175 and
    /// 179; the DXF omits the group when the source is 1.
    pub source: u8,
    /// `TV` file name, read only when `source` is 1. DXF group 3 for the
    /// diffuse map and 8 for the bump map.
    pub file: Option<String>,
}

/// The fields from R2007 on that follow the last map block.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialTail {
    /// `BD`, DXF group 148.
    pub dxf_148: f64,
    /// `BD`, DXF group 468.
    pub dxf_468: f64,
    /// `BS`, DXF group 93.
    pub dxf_93: u8,
    /// `BS`, DXF group 94 (a bit per map with a file).
    pub dxf_94: u8,
}

/// A decoded MATERIAL record.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub description: String,
    pub ambient: MaterialColor,
    pub diffuse: MaterialColor,
    pub diffuse_map: MaterialMap,
    pub specular: MaterialColor,
    pub specular_map: MaterialMap,
    /// `BD` between the specular map and the reflection map, DXF group 44.
    pub gloss: f64,
    pub reflection_map: MaterialMap,
    /// `BD` before the opacity map, DXF group 140.
    pub opacity: f64,
    pub opacity_map: MaterialMap,
    pub bump_map: MaterialMap,
    /// `BD` before the refraction map, DXF group 145.
    pub refraction_index: f64,
    pub refraction_map: MaterialMap,
    /// `None` before R2007.
    pub tail: Option<MaterialTail>,
}

fn unsupported(what: &str) -> Error {
    Error::Unsupported {
        feature: format!("MATERIAL {what}"),
    }
}

/// One `BL`/`BS`-coded small integer: the zero code or the `RC` form, the
/// only two the corpus holds. Anything wider is refused, not read on a
/// guess (CLEANROOM.md, 2026-10-26).
fn read_small(c: &mut BitCursor<'_>, what: &str) -> Result<u8> {
    match c.read_bb()? {
        0b10 => Ok(0),
        0b01 => c.read_rc(),
        _ => Err(unsupported(what)),
    }
}

/// A 2-bit field that is the zero code of a `BD` in every record.
fn read_zero(c: &mut BitCursor<'_>, what: &str) -> Result<()> {
    if c.read_bb()? == 0b10 {
        Ok(())
    } else {
        Err(unsupported(what))
    }
}

fn read_color(c: &mut BitCursor<'_>, what: &str) -> Result<MaterialColor> {
    let method = c.read_rc()?;
    let unknown_factor = c.read_bd()?;
    let color = match method {
        0 => None,
        1 => Some(c.read_bl_u()?),
        _ => return Err(unsupported(&format!("{what} colour method {method}"))),
    };
    Ok(MaterialColor {
        method,
        unknown_factor,
        color,
    })
}

fn read_map(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
    what: &str,
) -> Result<MaterialMap> {
    let blend = c.read_bd()?;
    let unknown_a = c.read_rc()?;
    let unknown_b = c.read_rc()?;
    let unknown_c = c.read_rc()?;
    let mut matrix = [0.0; 16];
    for m in &mut matrix {
        *m = c.read_bd()?;
    }
    let source = c.read_rc()?;
    let file = match source {
        0 => None,
        1 => Some(modern::read_tv(c, strings, version)?),
        _ => return Err(unsupported(&format!("{what} map source {source}"))),
    };
    Ok(MaterialMap {
        blend,
        unknown_a,
        unknown_b,
        unknown_c,
        matrix,
        source,
        file,
    })
}

fn read_body(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<Material> {
    let name = modern::read_tv(c, strings, version)?;
    let description = modern::read_tv(c, strings, version)?;
    let ambient = read_color(c, "ambient")?;
    let diffuse = read_color(c, "diffuse")?;
    let diffuse_map = read_map(c, strings, version, "diffuse")?;
    let specular = read_color(c, "specular")?;
    let specular_map = read_map(c, strings, version, "specular")?;
    let gloss = c.read_bd()?;
    let reflection_map = read_map(c, strings, version, "reflection")?;
    let opacity = c.read_bd()?;
    let opacity_map = read_map(c, strings, version, "opacity")?;
    let bump_map = read_map(c, strings, version, "bump")?;
    let refraction_index = c.read_bd()?;
    let refraction_map = read_map(c, strings, version, "refraction")?;
    let tail = if version.is_r2007_plus() {
        let dxf_148 = c.read_bd()?;
        read_zero(c, "tail slot after group 148")?;
        let dxf_468 = c.read_bd()?;
        let dxf_93 = read_small(c, "group 93")?;
        let dxf_94 = read_small(c, "group 94")?;
        read_zero(c, "last tail slot")?;
        Some(MaterialTail {
            dxf_148,
            dxf_468,
            dxf_93,
            dxf_94,
        })
    } else {
        None
    };
    Ok(Material {
        name,
        description,
        ambient,
        diffuse,
        diffuse_map,
        specular,
        specular_map,
        gloss,
        reflection_map,
        opacity,
        opacity_map,
        bump_map,
        refraction_index,
        refraction_map,
        tail,
    })
}

/// Decode a MATERIAL straight from its raw object payload: the field list
/// of the module docs, derived from AutoCAD's own output (CLEANROOM.md,
/// 2026-10-26; ARCHITECTURE.md §7au), ending exactly on the data-stream
/// boundary and, from R2007, with every string-stream entry consumed.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<Material> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings, version)?;
    split.finish("MATERIAL")?;
    if let Some(strings) = split.strings.as_ref() {
        if !strings.is_exhausted() {
            return Err(Error::SectionMap(
                "MATERIAL string stream holds entries its field list never read".into(),
            ));
        }
    }
    Ok(out)
}

/// The strings and data-field budget of an ACAD_MATERIAL record, as
/// [`read_strings`] measured them before the field list was derived.
/// [`Material`] supersedes it; it is kept with its tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcadMaterial {
    /// Material name — `ByLayer`, `ByBlock` and `Global` in every
    /// corpus file. Taken from the R2007+ string stream.
    pub name: String,
    /// Material description; empty in every corpus record.
    pub description: String,
    /// Further string-stream entries. R2018 records carry six empty
    /// slots here, one per texture map.
    pub extra_strings: Vec<String>,
    /// Bits between the end of the common object data and the record's
    /// data-stream boundary — the budget a future field list has to
    /// fill exactly. `None` on R13/R14, which carry no boundary.
    pub data_field_bits: Option<usize>,
}

/// Read the strings and the measured data-field budget of one
/// ACAD_MATERIAL record, without its data fields: the measurement entry
/// point of the module before `decode_object` closed the field list
/// (ARCHITECTURE.md §7au). It does not run the boundary check.
pub fn read_strings(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadMaterial> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let name = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let description = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let data_field_bits = split
        .data_end()
        .map(|end| end.saturating_sub(split.data.position_bits()));
    let mut extra_strings = Vec::new();
    if let Some(strings) = split.strings.as_mut() {
        while !strings.is_exhausted() {
            match strings.read_tv() {
                Ok(text) => extra_strings.push(text),
                Err(_) => break,
            }
        }
    }
    Ok(AcadMaterial {
        name,
        description,
        extra_strings,
        data_field_bits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn r2018_split_stream_material_reads_its_names_from_the_string_stream() {
        let mut body = modern::tests::r2018_object_prefix(1);
        // Stand-in for the undecoded data fields.
        for _ in 0..40 {
            body.write_b(false);
        }
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload =
            crate::string_stream::tests::build_payload(&bits, &["Global", "", "", "", "", "", ""]);
        let material = read_strings(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(material.name, "Global");
        assert_eq!(material.description, "");
        assert_eq!(material.extra_strings.len(), 5);
        assert_eq!(material.data_field_bits, Some(40));
    }

    #[test]
    fn inline_layout_reads_its_names_from_the_data_cursor() {
        let mut w = BitWriter::new();
        w.write_bs_u(0); // EED terminator
        w.write_bl(1); // num_reactors
        w.write_b(true); // no xdictionary (R2004+)
        w.write_bs_u(8);
        for b in b"ByLayer\0" {
            w.write_rc(*b);
        }
        w.write_bs_u(0); // empty description
        let after_names = w.position_bits();
        for _ in 0..24 {
            w.write_b(false);
        }
        let end = w.position_bits();
        let bytes = w.into_bytes();
        let material = read_strings(&bytes, 0, Some(end), Version::R2004).unwrap();
        assert_eq!(material.name, "ByLayer");
        assert_eq!(material.description, "");
        assert!(material.extra_strings.is_empty());
        assert_eq!(material.data_field_bits, Some(end - after_names));
    }
}
