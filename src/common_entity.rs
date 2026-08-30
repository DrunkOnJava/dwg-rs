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
//! B   graphics_present     -- if true, a size field + that many bytes
//!                             follow: RL up to R2007, BLL from R2010
//! BB  entmode              -- entity mode (see [`EntityMode`])
//! BL  num_reactors
//! B   no_xdictionary_handle (R2004+)
//! B   has_ds_binary_data   (R2013+)
//! CMC color                -- BS index + optional BL rgb + B name_flag + TV name
//! BD  linetype_scale
//! BB  ltype_flags
//! BB  plotstyle_flag       (R2000+)
//! BB  material_flag        (R2007+)
//! RC  shadow_flags         (R2007+)
//! B   has_full_visualstyle (R2010+)
//! B   has_face_visualstyle (R2010+)
//! B   has_edge_visualstyle (R2010+)
//! BS  invisibility
//! RC  lineweight           (R2000+)
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
    /// R2013+: whether DS binary data follows.
    ///
    /// Kept as `binary_chain` for API compatibility with the earlier
    /// experimental reader.
    pub binary_chain: bool,
    /// Legacy pre-R2004 linetype marker compatibility field. Modern R2004+
    /// entity streams do not carry a separate "is on layer" data bit here.
    pub is_on_layer: bool,
    /// Legacy pre-R2004 linetype marker compatibility field. Modern R2004+
    /// streams use `ltype_flags` after linetype scale instead.
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

/// Consume the extended entity data (EED) chain — a stream of
/// `<BS size, H appid, RC*size payload>` groups terminated by
/// `size == 0` (§19.4.1). Returns whether any group was present.
///
/// Defensive cap: real XDATA payloads per object rarely exceed a few
/// hundred bytes per iteration. The `size` field is a `BS`, so one
/// iteration can read up to 64 KB. Bounding total iterations at 256
/// gives a per-object worst case of ~16 MB of XDATA reads — an order
/// of magnitude past anything observed in real drawings but still
/// bounded against adversarial streams that would otherwise spin the
/// loop indefinitely.
fn skip_extended_data(c: &mut BitCursor<'_>) -> Result<bool> {
    const MAX_XDATA_ITERATIONS: usize = 256;
    let mut had_extended = false;
    for _ in 0..MAX_XDATA_ITERATIONS {
        let size = c.read_bs_u()?;
        if size == 0 {
            return Ok(had_extended);
        }
        had_extended = true;
        // Appid handle (may be absolute or offset).
        let _appid: Handle = c.read_handle()?;
        // App-data payload: `size` raw chars (bounded above via iteration cap).
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
    }
    Err(crate::error::Error::SectionMap(format!(
        "common object XDATA loop exceeded {MAX_XDATA_ITERATIONS} iterations; \
         malformed or adversarial payload"
    )))
}

/// Size in bytes of the graphics-preview block that follows a set
/// `graphics present` flag in the common entity preamble (§19.4.1).
///
/// # Measured
///
/// Up to R2007 the size is an `RL`. From R2010 it is a `BLL` (§2.4).
/// On `sample_AC1032.dwg` (R2018) reading it as an `RL` yields sizes in
/// the millions of bytes for records only a few hundred bytes long, and
/// the preamble runs off the end of the record — the 20 errors of #42.
/// Reading it as a `BLL` lands every one of those records on a preamble
/// whose remaining fields match the values every plain entity in the
/// same file carries (`entmode = 2`, `num_reactors = 0`,
/// `no_xdictionary = true`, colour `0x0100`, linetype scale `1.0`, all
/// flag fields `0`, `invisibility = 0`, `lineweight = 0x1D`):
///
/// | record            | `BLL` prefix bits | size (bytes) | preamble ends |
/// |-------------------|-------------------|--------------|---------------|
/// | IMAGE `0x662`     | `001` + `8C`      | 140          | bit 1213      |
/// | WIPEOUT `0x44D`   | `001` + `8C`      | 140          | bit 1213      |
/// | MULTILEADER `0x66E` | `010` + `50 03` | 848          | bit 6885      |
/// | MESH `0x343`      | `010` + `28 25`   | 9512         | bit 76197     |
///
/// The preview block is what makes these records custom-class-only in
/// practice: AutoCAD writes proxy graphics for entities whose class is
/// not built in, so the `graphics present` flag is set on exactly the
/// MULTILEADER / MESH / ACAD_TABLE / WIPEOUT / IMAGE records and clear
/// on LINE / TEXT / MTEXT. Nothing about the *class* changes the
/// preamble; the graphics block is simply never exercised without one.
///
/// `examples/probe_entity_preamble.rs` prints both readings side by side.
fn read_graphics_size(c: &mut BitCursor<'_>, version: Version) -> Result<u64> {
    if version.is_r2010_plus() {
        c.read_bll()
    } else {
        Ok(c.read_rl()? as u64)
    }
}

/// Read the **non-entity** object's common data (§19.4.2), advancing
/// past it, and return the reactor count.
///
/// Symbol-table entries, dictionaries and the other non-drawable
/// objects share this shorter prefix — no graphics block and no
/// entity mode, but the same EED chain, reactor count and
/// xdictionary-missing flag an entity carries:
///
/// ```text
/// //  extended entity data: loops while size > 0
/// BL  num_reactors
/// B   no_xdictionary_handle  (R2004+)
/// B   has_ds_binary_data     (R2013+)
/// ```
///
/// The cursor must be positioned immediately after the object handle.
///
/// # Measured
///
/// `examples/probe_r2004_object_prefix.rs` reads the four candidate
/// prefixes at that position for every LAYER / STYLE / LTYPE / APPID
/// record of `line_2004.dwg`. Only `EED + BL + B` yields readable
/// names — `0`, `Standard`, `Annotative`, `ACAD`, `AcadAnnotative`,
/// `ByBlock`, `ByLayer`, `Continuous`. Dropping the `BL` (a 5-bit
/// prefix becoming 3) shifts every name into an unreadable length
/// prefix of 65-73 characters.
pub fn read_common_object_data(c: &mut BitCursor<'_>, version: Version) -> Result<u32> {
    skip_extended_data(c)?;
    let num_reactors = c.read_bl()? as u32;
    if version.is_r2004_plus() {
        let _no_xdictionary = c.read_b()?;
    }
    if matches!(version, Version::R2013 | Version::R2018) {
        let _has_ds_binary_data = c.read_b()?;
    }
    Ok(num_reactors)
}

/// Read the common entity preamble from `c`, advancing past it.
///
/// The cursor must be positioned at the start of the extended-data
/// loop — i.e. immediately after the object header handle. On return,
/// it points at the entity-specific payload.
///
/// This is version-aware: fields added in R2004 / R2007 / R2010 are
/// read only for versions that include them.
pub fn read_common_entity_data(
    c: &mut BitCursor<'_>,
    version: Version,
) -> Result<CommonEntityData> {
    // -- Extended data loop --------------------------------------------------
    let had_extended = skip_extended_data(c)?;

    // -- Graphics preview ----------------------------------------------------
    // The size of the preview block is an `RL` up to R2007 and a `BLL`
    // (§2.4 — three-bit byte count, then that many little-endian bytes)
    // from R2010 on. See [`read_graphics_size`] for the measurement.
    let had_graphics = c.read_b()?;
    if had_graphics {
        let gfx_size = read_graphics_size(c, version)?;
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
    let no_xdictionary = if version.is_r2004_plus() {
        c.read_b()?
    } else {
        true
    };
    let binary_chain = if matches!(version, Version::R2013 | Version::R2018) {
        c.read_b()?
    } else {
        false
    };

    // R13/R14 carry a separate by-layer linetype marker before the modern
    // ltype_flags field existed. R2004+ does not; those releases encode this
    // in ltype_flags below, after linetype_scale.
    let (is_on_layer, non_fixed_ltype) = if matches!(version, Version::R14) {
        let is_by_layer_linetype = c.read_b()?;
        (true, !is_by_layer_linetype)
    } else {
        (true, false)
    };

    // R13-R2000 carry a "nolinks" bit before color. The current public struct
    // has no stable place for it, but it must still be consumed on legacy
    // streams so color starts at the right bit.
    if matches!(version, Version::R14 | Version::R2000) {
        let _nolinks = c.read_b()?;
    }

    // -- CMC entity color (§2.11) -------------------------------------------
    // R2004+ stores a raw BS whose high bits flag optional alpha/RGB/name
    // suffixes. Color handles live in the handle stream, so they are noted by
    // the flags but do not consume data-stream bits here.
    let color_raw = c.read_bs_u()?;
    if version.is_r2004_plus() {
        let color_flags = color_raw >> 8;
        if color_flags & 0x20 != 0 {
            let _alpha_raw = c.read_bl()?;
        }
        if color_flags & 0x40 == 0 && color_flags & 0x80 != 0 {
            let _rgb = c.read_bl()?;
        }
        if color_flags & 0x41 == 0x41 {
            let _name = crate::tables::read_tv(c, version)?;
        }
        if color_flags & 0x42 == 0x42 {
            let _book_name = crate::tables::read_tv(c, version)?;
        }
    }

    // -- BD linetype_scale (default 1.0 → BB tag 01, 2 bits) ----------------
    let _linetype_scale = c.read_bd()?;

    // -- BB ltype_flags (how layer/linetype handles are encoded) ------------
    let _ltype_flags = c.read_bb()?;

    let plotstyle_flag = c.read_bb()?;

    // -- Material + shadow (R2007+) -----------------------------------------
    // shadow_flags is an RC in modern DWG streams. Earlier local experiments
    // tried BB, but that masked a separate two-bit overread before CMC color
    // and left R2013/R2018 entity bodies misaligned.
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
        // no_xdictionary = true; has_ds_data is R2013+.
        w.write_b(true);
        w.write_b(false);
        // CMC color — BS index, BYLAYER (tag 10 → 2 bits, value 0)
        w.write_bs(0);
        // BD linetype_scale — 1.0 (tag 01 → 2 bits)
        w.write_bd(1.0);
        // BB ltype_flags
        w.write_bb(0b00);
        // plotstyle_flag
        w.write_bb(0b00);
        // material_flag, shadow_flags (R2007+)
        w.write_bb(0b00);
        w.write_rc(0b00);
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
        // Graphics: present, 3 bytes. R2010+ sizes the block with a BLL.
        w.write_b(true);
        w.write_bll(3).unwrap();
        w.write_rc(0x11);
        w.write_rc(0x22);
        w.write_rc(0x33);
        // Entity mode: InBlock (0b10).
        w.write_bb(0b10);
        w.write_bl(2); // 2 reactors
        w.write_b(false); // has xdict
        w.write_b(true); // has_ds_data
        // CMC color — BYLAYER
        w.write_bs(0);
        // BD linetype_scale — 1.0
        w.write_bd(1.0);
        // BB ltype_flags
        w.write_bb(0b00);
        w.write_bb(0b01);
        w.write_bb(0b10);
        w.write_rc(0b11);
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
        assert!(!ce.non_fixed_ltype);
        assert_eq!(ce.plotstyle_flag, 0b01);
        assert_eq!(ce.material_flag, 0b10);
        assert_eq!(ce.shadow_flags, 0b11);
        assert_eq!(ce.invisibility, 1);
        assert_eq!(ce.lineweight, 0x05);
    }

    /// The R2018 shape measured on `sample_AC1032.dwg`: a 140-byte
    /// graphics-preview block sized by a `BLL` (`001` + `0x8C`), then a
    /// preamble carrying the values every entity in that file shares.
    ///
    /// Written with the same size field read as an `RL` this record
    /// claims millions of bytes of preview and the reader runs off the
    /// end — the failure mode of #42.
    #[test]
    fn r2018_graphics_block_is_sized_by_a_bll() {
        let mut w = BitWriter::new();
        w.write_bs_u(0); // no XDATA
        w.write_b(true); // graphics present
        w.write_bll(140).unwrap();
        for i in 0..140u16 {
            w.write_rc(i as u8);
        }
        let after_graphics = w.position_bits();
        w.write_bb(0b10); // entmode = InBlock
        w.write_bl(0); // num_reactors
        w.write_b(true); // no xdictionary
        w.write_b(false); // no AcDs binary data
        w.write_bs_u(0x0100); // CMC colour, no suffixes
        w.write_bd(1.0); // linetype scale
        w.write_bb(0b00); // ltype flags
        w.write_bb(0b00); // plotstyle flag
        w.write_bb(0b00); // material flag
        w.write_rc(0); // shadow flags
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
        w.write_bs(0); // invisibility
        w.write_rc(0x1D); // lineweight (BYLAYER)
        let end = w.position_bits();

        // 1 BS tag + 1 flag bit + 3 count bits + 8 length bits + 140 bytes.
        assert_eq!(after_graphics, 2 + 1 + 3 + 8 + 140 * 8);

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ce = read_common_entity_data(&mut c, Version::R2018).unwrap();
        assert!(ce.had_graphics);
        assert!(!ce.had_extended_data);
        assert_eq!(ce.mode, EntityMode::InBlock);
        assert_eq!(ce.num_reactors, 0);
        assert!(ce.no_xdictionary);
        assert_eq!(ce.invisibility, 0);
        assert_eq!(ce.lineweight, 0x1D);
        assert_eq!(c.position_bits(), end);
    }

    /// Up to R2007 the same block is sized by an `RL`.
    #[test]
    fn r2004_graphics_block_is_sized_by_an_rl() {
        let mut w = BitWriter::new();
        w.write_bs_u(0);
        w.write_b(true);
        w.write_rl(4);
        for _ in 0..4 {
            w.write_rc(0xEE);
        }
        w.write_bb(0b00);
        w.write_bl(0);
        w.write_b(true);
        w.write_bs_u(0x0100);
        w.write_bd(1.0);
        w.write_bb(0b00);
        w.write_bb(0b00);
        w.write_bs(0);
        w.write_rc(0x1D);
        let end = w.position_bits();

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ce = read_common_entity_data(&mut c, Version::R2004).unwrap();
        assert!(ce.had_graphics);
        assert_eq!(ce.mode, EntityMode::ByLayer);
        assert_eq!(ce.lineweight, 0x1D);
        assert_eq!(c.position_bits(), end);
    }
}
