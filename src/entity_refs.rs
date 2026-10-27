//! The handle references of a drawing entity (ODA Open Design Specification
//! v5.4.1 §20.4.2 "Common Entity Handle Data").
//!
//! An entity's handle stream starts with its owner, when its entity mode says it
//! has one, one soft pointer per reactor and the extension dictionary, then the
//! handles the common data announces (a colour book colour, the layer, the
//! linetype, the material, the plot style and the R2010 visual styles), and last
//! the handles of the entity's own fields. [`entity_refs`] reads the common ones
//! and returns the rest, as absolute handles with relative codes resolved against
//! the entity's own handle.

use crate::bitcursor::{BitCursor, Handle};
use crate::common_entity::{CommonEntityData, read_common_entity_data};
use crate::error::{Error, Result};
use crate::object::{RawObject, body_cursor};
use crate::string_stream;
use crate::tables::block_record::handle_stream_start;
use crate::version::Version;

const MAX_REFS: usize = 4_000_000;

/// The common handle references of one entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRefs {
    /// The raw entity mode (`BB`): 0 when an owner handle is stored, 1 when the
    /// entity is in the paper space block, 2 in the model space block.
    pub entity_mode: u8,
    /// The owner (a block record), when the entity mode stores one.
    pub owner: Option<u64>,
    /// The persistent reactors.
    pub reactors: Vec<u64>,
    /// The extension dictionary, when there is one.
    pub xdictionary: Option<u64>,
    /// The colour book colour handle (R2004+, when the colour flags announce it).
    pub color_book: Option<u64>,
    /// The layer.
    pub layer: u64,
    /// The linetype, when the linetype flags announce a handle.
    pub linetype: Option<u64>,
    /// The material flags (R2007+): 0 by layer, 1 by block, 3 a handle follows.
    pub material_flags: u8,
    /// The material (R2007+, when the material flags announce one).
    pub material: Option<u64>,
    /// The plot style, when the plot style flags announce one.
    pub plotstyle: Option<u64>,
    /// The full, face and edge visual styles (R2010+), when announced.
    pub visual_styles: [Option<u64>; 3],
    /// The previous and next entity handles of an R13 to R2000 entity whose
    /// `Nolinks` bit is clear; `None` when they are implicit (the handles before
    /// and after its own).
    pub links: Option<(u64, u64)>,
    /// The handles of the entity's own fields, in file order.
    pub rest: Vec<u64>,
}

fn resolve(h: Handle, own: u64) -> u64 {
    match h.code {
        0x6 => own.wrapping_add(1),
        0x8 => own.wrapping_sub(1),
        0xA => own.wrapping_add(h.value),
        0xC => own.wrapping_sub(h.value),
        _ => h.value,
    }
}

/// The common data of `raw` and its handle references.
///
/// Fails before R2000 (no anchor for the handle stream), on an entity whose
/// common data does not parse, and when a handle does not.
pub fn entity_refs(raw: &RawObject, version: Version) -> Result<(CommonEntityData, EntityRefs)> {
    if matches!(version, Version::R14) {
        return Err(Error::SectionMap(
            "entity has no handle stream anchor before R2000".into(),
        ));
    }
    let mut body = body_cursor(raw, version)?;
    let common = read_common_entity_data(&mut body, version)?;
    let start = handle_stream_start(raw, version)
        .ok_or_else(|| Error::SectionMap("entity has no handle stream anchor".into()))?;
    let mut c = BitCursor::new(&raw.raw);
    string_stream::seek(&mut c, start)?;
    let own = raw.handle.value;
    let next = |c: &mut BitCursor<'_>| -> Result<u64> { Ok(resolve(c.read_handle()?, own)) };
    let owner = if common.raw_mode == 0 {
        Some(next(&mut c)?)
    } else {
        None
    };
    let mut reactors = Vec::new();
    for _ in 0..common.num_reactors {
        if reactors.len() >= MAX_REFS {
            return Err(Error::SectionMap("entity claims too many reactors".into()));
        }
        reactors.push(next(&mut c)?);
    }
    let xdictionary = if version.is_r2004_plus() && common.no_xdictionary {
        None
    } else {
        Some(next(&mut c)?).filter(|h| *h != 0)
    };
    let links = if common.nolinks {
        None
    } else {
        Some((next(&mut c)?, next(&mut c)?))
    };
    let color_book = if version.is_r2004_plus() && (common.color_raw >> 8) & 0x40 != 0 {
        Some(next(&mut c)?)
    } else {
        None
    };
    let layer = next(&mut c)?;
    let linetype = if common.ltype_flags == 3 {
        Some(next(&mut c)?)
    } else {
        None
    };
    // From R2007 the flag pair the common data names `plotstyle_flag` first
    // and `material_flag` second is read the other way round: the first is
    // the material's (a handle that names a MATERIAL object, and DXF writes it
    // as 347) and the second the plot style's.
    let (material_flags, plotstyle_flags) = if version.is_r2007_plus() {
        (common.plotstyle_flag, common.material_flag)
    } else {
        (0, common.plotstyle_flag)
    };
    let material = if material_flags == 3 {
        Some(next(&mut c)?)
    } else {
        None
    };
    let plotstyle = if plotstyle_flags == 3 {
        Some(next(&mut c)?)
    } else {
        None
    };
    let mut visual_styles = [None; 3];
    if version.is_r2010_plus() {
        for (i, on) in common.visual_style_flags.iter().enumerate() {
            if *on {
                visual_styles[i] = Some(next(&mut c)?);
            }
        }
    }
    let mut rest = Vec::new();
    while c.remaining_bits() >= 8 {
        if rest.len() >= MAX_REFS {
            return Err(Error::SectionMap("entity holds too many handles".into()));
        }
        rest.push(next(&mut c)?);
    }
    Ok((
        common.clone(),
        EntityRefs {
            entity_mode: common.raw_mode,
            owner,
            reactors,
            xdictionary,
            color_book,
            layer,
            linetype,
            material_flags,
            material,
            plotstyle,
            visual_styles,
            links,
            rest,
        },
    ))
}
