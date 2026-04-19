//! Common entity data (spec §19.4.1) — the shared preamble every
//! drawable entity (LINE, CIRCLE, INSERT, TEXT, ...) writes before
//! its type-specific body.
//!
//! # Stream shape
//!
//! After the object header (handle + extended data + size bits), every
//! entity writes a fixed preamble roughly in this order (R2004+):
//!
//! ```text
//! BS  object_type          -- read by the walker
//! RL  object_size          -- bits, read by the walker
//! H   handle               -- read by the walker
//! //  extended entity data: loops while size > 0
//!       BS   size_bits
//!       H    appid_handle
//!       RC*  app_data
//! B   graphics_present     -- if true, RL size + bytes follow
//! BB  entmode              -- entity mode (see [`EntityMode`])
//! BL  num_reactors
//! B   no_xdictionary_handle
//! B   binary_chain_present  (R2004+)
//! B   is_on_layer
//! B   non_fixed_ltype
//! BB  plotstyle_flag
//! BB  material_flag         (R2007+)
//! RC  shadow_flags          (R2007+)
//! B   has_full_visualstyle  (R2010+)
//! B   has_face_visualstyle  (R2010+)
//! B   has_edge_visualstyle  (R2010+)
//! BS  invisibility
//! RC  lineweight            (R2000+)
//! ```
//!
//! After the preamble comes the entity-specific payload, then
//! handle references (owner/reactors/xdictionary/layer/linetype/...)
//! collected at the tail.
//!
//! # Scope
//!
//! This module decodes only the fields a viewer/writer realistically
//! needs: mode, layer flag, color indexing, lineweight, visibility.
//! Fields that are either redundant (they appear in the header) or
//! rarely consulted (visualstyle handles, material handles) are
//! *skipped* by advancing the cursor rather than surfaced in the
//! struct — the cursor ends up aligned on the entity payload no
//! matter which branch was taken, which is the important part.
//!
//! All non-preamble payload parsing is delegated to per-entity
//! modules (LINE, CIRCLE, etc.).

use crate::bitcursor::{BitCursor, Handle};
use crate::error::Result;
use crate::version::Version;

/// Entity mode — which table/block owns the entity and how its handle
/// references are encoded. Read from a 2-bit BB at the start of the
/// preamble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityMode {
    /// `00` — entity is "by layer" (owner is the model/paper-space
    /// block record; layer handle is explicit at end of record).
    ByLayer,
    /// `01` — entity follows the previous entity in the block
    /// (owner handle is implicit; saves ~6 bytes).
    ByPreviousEntity,
    /// `10` — entity is in a block (owner handle explicit at end).
    InBlock,
    /// `11` — reserved per spec; treat as [`EntityMode::ByLayer`]
    /// for practical purposes and surface via [`CommonEntityData::raw_mode`].
    Reserved,
}

impl EntityMode {
    fn from_bb(bb: u8) -> Self {
        match bb {
            0b00 => Self::ByLayer,
            0b01 => Self::ByPreviousEntity,
            0b10 => Self::InBlock,
            _ => Self::Reserved,
        }
    }
}

/// Decoded common entity preamble.
///
/// Captures the fields most callers need to correctly interpret the
/// subsequent entity-specific payload. Unused/skipped fields are
/// *consumed* from the cursor but not surfaced — the cursor position
/// on return is always at the first bit of the entity-specific body.
#[derive(Debug, Clone)]
pub struct CommonEntityData {
    /// Raw 2-bit entity-mode code.
    pub raw_mode: u8,
    /// Parsed entity mode.
    pub mode: EntityMode,
    /// Reactor count (number of back-reference handles at the tail).
    pub num_reactors: u32,
    /// Whether an xdictionary handle is absent.
    pub no_xdictionary: bool,
    /// R2004+: whether a binary chain follows.
    pub binary_chain: bool,
    /// Whether the "is on a layer?" flag is set. In practice always
    /// true for valid drawings; kept for diagnostic dumps.
    pub is_on_layer: bool,
    /// Whether the entity has a non-"BYLAYER" linetype.
    pub non_fixed_ltype: bool,
    /// Raw 2-bit plot-style flag.
    pub plotstyle_flag: u8,
    /// Raw 2-bit material flag (R2007+, else 0).
    pub material_flag: u8,
    /// R2007+ shadow flags byte (0 for earlier versions).
    pub shadow_flags: u8,
    /// 16-bit "invisibility" mask (spec §19.4.1 bit 0 = invisible).
    pub invisibility: i16,
    /// R2000+ lineweight byte (encoded — not millimeters, see
    /// `DxfLineweight` in §19.4.82). For pre-R2000, 0.
    pub lineweight: u8,
    /// Did a graphics preview block precede the mode bits? If true,
    /// the preview bytes have already been skipped past by this
    /// decoder.
    pub had_graphics: bool,
    /// Did extended entity data (XDATA) precede the mode bits? If
    /// true, it has been skipped past (appid + payload).
    pub had_extended_data: bool,
}

/// Read the common entity preamble from `c`, advancing past it.
///
/// The cursor must be positioned at the start of the extended-data
/// loop — i.e. immediately after the object header handle. On return,
/// it points at the entity-specific payload.
///
/// This is version-aware: fields added in R2004 / R2007 / R2010 are
/// read only for versions that include them.
pub fn read_common_entity_data(c: &mut BitCursor<'_>, version: Version) -> Result<CommonEntityData> {
    // -- Extended data loop --------------------------------------------------
    // Stream of <BS size, H appid, RC*size app-payload>. Loop
    // terminates when size == 0.
    let mut had_extended = false;
    loop {
        let size = c.read_bs_u()?;
        if size == 0 {
            break;
        }
        had_extended = true;
        // Appid handle (may be absolute or offset).
        let _appid: Handle = c.read_handle()?;
        // App-data payload: `size` raw chars.
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
        // Defensive stop: real files never have >1 KB of XDATA per
        // entity, so we bail after 4 loops to keep malformed streams
        // from spinning forever.
        if had_extended && size as usize > 4096 {
            break;
        }
    }

    // -- Graphics preview ----------------------------------------------------
    let had_graphics = c.read_b()?;
    if had_graphics {
        let gfx_size = c.read_rl()? as usize;
        // Skip exactly gfx_size bytes.
        for _ in 0..gfx_size {
            let _ = c.read_rc()?;
        }
    }

    // -- Entity mode ---------------------------------------------------------
    let raw_mode = c.read_bb()?;
    let mode = EntityMode::from_bb(raw_mode);

    // -- Reactors + object-dict markers -------------------------------------
    let num_reactors = c.read_bl()? as u32;
    let no_xdictionary = c.read_b()?;
    let binary_chain = if version.is_r2004_plus() {
        c.read_b()?
    } else {
        false
    };

    // -- Layer + linetype markers -------------------------------------------
    let is_on_layer = c.read_b()?;
    let non_fixed_ltype = c.read_b()?;
    let plotstyle_flag = c.read_bb()?;

    // -- Material + shadow (R2007+) -----------------------------------------
    let (material_flag, shadow_flags) = if version.is_r2007_plus() {
        (c.read_bb()?, c.read_rc()?)
    } else {
        (0u8, 0u8)
    };

    // -- Visual-style flags (R2010+) ----------------------------------------
    if version.is_r2010_plus() {
        let _has_full = c.read_b()?;
        let _has_face = c.read_b()?;
        let _has_edge = c.read_b()?;
    }

    // -- Invisibility + lineweight ------------------------------------------
    let invisibility = c.read_bs()?;
    let lineweight = if matches!(
        version,
        Version::R2000
            | Version::R2004
            | Version::R2007
            | Version::R2010
            | Version::R2013
            | Version::R2018
    ) {
        c.read_rc()?
    } else {
        0
    };

    Ok(CommonEntityData {
        raw_mode,
        mode,
        num_reactors,
        no_xdictionary,
        binary_chain,
        is_on_layer,
        non_fixed_ltype,
        plotstyle_flag,
        material_flag,
        shadow_flags,
        invisibility,
        lineweight,
        had_graphics,
        had_extended_data: had_extended,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Synthesize a minimal entity preamble (R2018, no graphics, no
    /// XDATA, ByLayer mode, no reactors, default flags), read it back,
    /// and verify round-trip.
    #[test]
    fn roundtrip_minimal_r2018_preamble() {
        let mut w = BitWriter::new();
        // Extended data: length 0 terminates the loop.
        w.write_bs_u(0);
        // Graphics present: false.
        w.write_b(false);
        // Entity mode: ByLayer (0b00).
        w.write_bb(0b00);
        // num_reactors = 0.
        w.write_bl(0);
        // no_xdictionary = true, binary_chain = false (R2004+).
        w.write_b(true);
        w.write_b(false);
        // is_on_layer, non_fixed_ltype, plotstyle_flag
        w.write_b(true);
        w.write_b(false);
        w.write_bb(0b00);
        // material_flag, shadow_flags (R2007+)
        w.write_bb(0b00);
        w.write_rc(0);
        // visualstyle full/face/edge (R2010+)
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
        // invisibility, lineweight
        w.write_bs(0);
        w.write_rc(0);

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ce = read_common_entity_data(&mut c, Version::R2018).unwrap();
        assert_eq!(ce.mode, EntityMode::ByLayer);
        assert!(!ce.had_graphics);
        assert!(!ce.had_extended_data);
        assert_eq!(ce.num_reactors, 0);
        assert!(ce.no_xdictionary);
        assert!(ce.is_on_layer);
        assert_eq!(ce.invisibility, 0);
        assert_eq!(ce.lineweight, 0);
    }

    #[test]
    fn roundtrip_with_graphics_and_xdata() {
        let mut w = BitWriter::new();
        // XDATA: one 2-byte payload + appid handle + terminator
        w.write_bs_u(2);
        w.write_handle(5, 0x42);
        w.write_rc(0xAA);
        w.write_rc(0xBB);
        w.write_bs_u(0); // terminator
        // Graphics: present, 3 bytes.
        w.write_b(true);
        w.write_rl(3);
        w.write_rc(0x11);
        w.write_rc(0x22);
        w.write_rc(0x33);
        // Entity mode: InBlock (0b10).
        w.write_bb(0b10);
        w.write_bl(2); // 2 reactors
        w.write_b(false); // has xdict
        w.write_b(true); // binary_chain
        w.write_b(true);
        w.write_b(true);
        w.write_bb(0b01);
        w.write_bb(0b10);
        w.write_rc(0x03);
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
        w.write_bs(1);
        w.write_rc(0x05);

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ce = read_common_entity_data(&mut c, Version::R2018).unwrap();
        assert!(ce.had_extended_data);
        assert!(ce.had_graphics);
        assert_eq!(ce.mode, EntityMode::InBlock);
        assert_eq!(ce.num_reactors, 2);
        assert!(!ce.no_xdictionary);
        assert!(ce.binary_chain);
        assert!(ce.is_on_layer);
        assert!(ce.non_fixed_ltype);
        assert_eq!(ce.plotstyle_flag, 0b01);
        assert_eq!(ce.material_flag, 0b10);
        assert_eq!(ce.shadow_flags, 0x03);
        assert_eq!(ce.invisibility, 1);
        assert_eq!(ce.lineweight, 0x05);
    }
}
