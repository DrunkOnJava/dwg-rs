//! LAYER table entry (§20.4.54) — named drawing layer with display
//! flags, colour, linetype, lineweight and plot style.
//!
//! # Stream shape
//!
//! ```text
//! common object data         -- handled by caller
//! TV     name                -- handled by [`read_table_entry_header`]
//! B      64-flag
//! BS     xref_index_plus_1
//! B      is_xref_dependent
//! R13-R14:  B frozen, B on, B frozen-in-new, B locked
//! R2000+:   BS values        -- frozen / on / frozen-in-new / locked /
//!                                plot flag / 5-bit lineweight index
//! CMC    colour              -- bare BS index up to R2000 (§2.11),
//!                                BS + BL + RC from R2004
//! ```
//!
//! There is no separate plot flag and no separate lineweight field:
//! from R2000 both live inside `values`, and R13/R14 have neither.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::tables::{TableEntryHeader, modern, read_table_entry_header};
use crate::version::Version;

/// Decoded LAYER entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub header: TableEntryHeader,
    pub flags: i16,
    pub plot_flag: bool,
    pub lineweight: i16,
    /// Simplified color — returned as the color-book "index" value.
    /// Real CMC decoding (§2.9) is complex; for most drawings, color
    /// is a single small positive integer (ACI indexed color) or 0.
    pub color_index: i16,
}

impl Layer {
    /// Bit 0x01 of `flags`: the layer is frozen (§20.4.53).
    pub fn is_frozen(&self) -> bool {
        self.flags & LAYER_VALUE_FROZEN != 0
    }
    /// Bit 0x08 of `flags`: the layer is locked (§20.4.53).
    ///
    /// The DWG `values` word is not the DXF group-70 flag word: DXF
    /// puts locked at `0x04`, DWG puts *frozen in new viewports*
    /// there and locked at `0x08`. This accessor tested `0x04` until
    /// #26, which made it report every layer frozen-in-new-viewports
    /// as locked and every genuinely locked layer as unlocked.
    pub fn is_locked(&self) -> bool {
        self.flags & LAYER_VALUE_LOCKED != 0
    }

    /// Bit 0x04 of `flags`: the layer is frozen in newly created
    /// viewports (§20.4.53).
    pub fn is_frozen_in_new_viewports(&self) -> bool {
        self.flags & LAYER_VALUE_FROZEN_IN_NEW != 0
    }
    /// The layer's plot flag (plotted when set).
    pub fn is_plottable(&self) -> bool {
        self.plot_flag
    }
}

/// Decodes a `Layer` table entry that follows the common object header.
///
/// # Field list (§20.4.54)
///
/// ```text
/// TV   entry name          -- via read_table_entry_header
/// B    64-flag
/// BS   xrefindex + 1
/// B    xdep
/// R13-R14:  B frozen, B on, B frozen-in-new, B locked
/// R2000+:   BS values      -- frozen 0x01, on 0x02, frz-new 0x04,
///                             locked 0x08, plot 0x10, lineweight <<5
/// CMC  colour              -- a bare BS index up to R2000 (§2.11)
/// ```
///
/// An earlier revision read `BS values`, then a `B` plot flag, a `BS`
/// lineweight and a `BS` colour — three fields §20.4.54 does not list,
/// because `values` already carries the plot flag and the lineweight
/// index. That reading overshot every pre-R2007 LAYER record's
/// data-stream boundary; the `values` word itself was right (`0x03F0`
/// on layer `0` of every corpus file, matching the R2007+ split-stream
/// path exactly) but everything after it was noise — `line_2004.dwg`
/// reported `lineweight = -32765`, `colour = -31231`.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Layer> {
    let header = read_table_entry_header(c, version)?;
    let flags = if matches!(version, Version::R14) {
        // §20.4.54 gives R13-R14 four separate bits where R2000+ pack
        // the same states into `values`. Reassembled into the R2000+
        // word so `Layer::is_frozen` and friends mean one thing across
        // every release; the plot flag and lineweight index have no
        // R13/R14 counterpart and stay clear.
        let frozen = c.read_b()?;
        let on = c.read_b()?;
        let frozen_in_new = c.read_b()?;
        let locked = c.read_b()?;
        let mut v = 0i16;
        if frozen {
            v |= LAYER_VALUE_FROZEN;
        }
        if on {
            v |= LAYER_VALUE_OFF;
        }
        if frozen_in_new {
            v |= LAYER_VALUE_FROZEN_IN_NEW;
        }
        if locked {
            v |= LAYER_VALUE_LOCKED;
        }
        v
    } else {
        c.read_bs()?
    };
    // §2.11: "R15 and earlier: BS color index". R2004 introduced the
    // BS + BL + RC form.
    let color_index = if version.is_r2004_plus() {
        let (index, _rgb, _byte) = read_cmc_inline(c)?;
        index
    } else {
        c.read_bs()?
    };
    Ok(Layer {
        header,
        flags,
        plot_flag: flags & LAYER_VALUE_PLOT != 0,
        lineweight: (flags >> LAYER_VALUE_LINEWEIGHT_SHIFT) & 0x1F,
        color_index,
    })
}

/// Read an R2004+ inline `CMC` (§2.11): `BS` index, `BL` RGB, `RC`
/// colour byte, plus an optional colour name and book name.
///
/// Returns `(index, rgb, colour byte)`. The index is the ACI when the
/// RGB word carries one (`0xC3` in its top byte), which is what every
/// corpus LAYER records.
fn read_cmc_inline(c: &mut BitCursor<'_>) -> Result<(i16, u32, u8)> {
    let index = c.read_bs()?;
    let rgb = c.read_bl()? as u32;
    let color_byte = c.read_rc()?;
    if color_byte & 0x01 != 0 {
        let _name = crate::tables::read_tv(c, Version::R2004)?;
    }
    if color_byte & 0x02 != 0 {
        let _book = crate::tables::read_tv(c, Version::R2004)?;
    }
    let index = if index == 0 { aci_from_cmc(rgb) } else { index };
    Ok((index, rgb, color_byte))
}

/// Bit 0x01 of the R2007+ `values` word: the layer is frozen.
pub const LAYER_VALUE_FROZEN: i16 = 0x01;
/// Bit 0x02 of the R2007+ `values` word: the layer is off.
pub const LAYER_VALUE_OFF: i16 = 0x02;
/// Bit 0x04 of the R2007+ `values` word: frozen in new viewports.
pub const LAYER_VALUE_FROZEN_IN_NEW: i16 = 0x04;
/// Bit 0x08 of the R2007+ `values` word: the layer is locked.
pub const LAYER_VALUE_LOCKED: i16 = 0x08;
/// Bit 0x10 of the R2007+ `values` word: the layer plots.
pub const LAYER_VALUE_PLOT: i16 = 0x10;
/// Shift of the 5-bit lineweight index inside the `values` word.
pub const LAYER_VALUE_LINEWEIGHT_SHIFT: u32 = 5;

/// Decode an R2007+ LAYER whose name lives in the object's string
/// stream (ODA v5.4.1 §19.1 split layout, §20.4.53 LAYER field table).
///
/// Data stream after the common object prefix:
///
/// ```text
/// B    64-flag
/// B    xref dependent
/// BS   xref index + 1
/// BS   values          -- state bits + lineweight index
/// CMC  colour          -- full BS/BL/RC form
/// ```
///
/// # Measured bit meanings
///
/// `sample_AC1032.dwg` carries layers deliberately named for their
/// state. `Layer1` reads `values = 0x03F0`; `Layer_Freeze` adds
/// `0x01`, `Layer_Off` adds `0x02`, `Layer_Lock` adds `0x08`. The
/// shared `0x03F0` decomposes as plot flag `0x10` plus lineweight
/// index `31` (the "by layer / default" sentinel) at bit 5. Note that
/// this is *not* the DXF group-70 flag word, where locked is `0x04`;
/// [`Layer::is_locked`] tests `0x08` and
/// [`Layer::is_frozen_in_new_viewports`] tests `0x04` accordingly
/// (#26).
///
/// The colour uses the same full `CMC` form as VIEW and VPORT; the
/// top byte of its `BL` selects the interpretation — `0xC3` marks an
/// ACI index in the low byte (`0xC3000007` = white on layer `0`),
/// `0xC2` marks a literal RGB triple.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<Layer> {
    let mut split = modern::open_table_entry(payload, object_body_start, version)?;
    let (flag64, xref_index_plus_1, is_xref_dependent) = modern::read_entry_flags(&mut split.data)?;
    let values = split.data.read_bs()?;
    let (_color_index_word, color_rgb, _color_byte) = modern::read_cmc_full(&mut split.data)?;
    split.finish("LAYER")?;
    let name = split.strings.read_tv()?;
    Ok(Layer {
        header: TableEntryHeader {
            name,
            is_xref_dependent,
            xref_index_plus_1,
            is_xref_resolved: flag64,
        },
        flags: values,
        plot_flag: values & LAYER_VALUE_PLOT != 0,
        lineweight: (values >> LAYER_VALUE_LINEWEIGHT_SHIFT) & 0x1F,
        color_index: aci_from_cmc(color_rgb),
    })
}

/// Extract an AutoCAD colour index from the `BL` word of a full `CMC`.
///
/// Returns 0 for a literal RGB colour, which has no index.
pub fn aci_from_cmc(rgb: u32) -> i16 {
    if rgb >> 24 == 0xC3 {
        (rgb & 0xFF) as i16
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_simple_layer() {
        let mut w = BitWriter::new();
        // Entry header
        let s = b"0"; // default layer is always named "0"
        w.write_bs_u(s.len() as u16);
        w.write_rc(b'0');
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
        // Body — §20.4.54 R2000+: `BS values` then a bare `BS` colour
        // index (§2.11 "R15 and earlier: BS color index").
        w.write_bs(LAYER_VALUE_PLOT | (31 << LAYER_VALUE_LINEWEIGHT_SHIFT));
        w.write_bs(7); // color = white
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(l.header.name, "0");
        assert!(l.is_plottable());
        assert_eq!(l.lineweight, 31);
        assert_eq!(l.color_index, 7);
        assert!(!l.is_frozen());
    }

    /// §20.4.54 gives R13/R14 four separate state bits where R2000+
    /// pack one `values` word, and no plot flag or lineweight at all.
    #[test]
    fn r14_layer_reads_four_state_bits() {
        let mut w = BitWriter::new();
        let s = b"0";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_b(false); // 64-flag
        w.write_bs(0); // xrefindex + 1
        w.write_b(false); // xdep
        w.write_b(true); // frozen
        w.write_b(false); // on
        w.write_b(false); // frozen in new viewports
        w.write_b(true); // locked
        w.write_bs(7); // colour index
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R14).unwrap();
        assert_eq!(l.header.name, "0");
        assert!(l.is_frozen());
        assert!(l.is_locked());
        assert!(!l.is_plottable());
        assert_eq!(l.lineweight, 0);
        assert_eq!(l.color_index, 7);
    }

    #[test]
    fn r2007_split_stream_layer_reads_name_from_string_stream() {
        let mut body = BitWriter::new();
        body.write_bs_u(0); // no EED
        body.write_b(true); // no xdictionary
        body.write_b(false); // no binary data
        body.write_b(false); // 64-flag
        body.write_b(false); // xref dependent
        body.write_bs(0); // xref index + 1
        body.write_bs(0x03F2); // values: off + plot + lineweight 31
        body.write_bs(0); // colour index word
        body.write_bl(0xC300_0007u32 as i32); // colour: ACI 7
        body.write_rc(0); // colour byte
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["Layer_Off"]);
        let l = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(l.header.name, "Layer_Off");
        assert_eq!(l.flags & LAYER_VALUE_OFF, LAYER_VALUE_OFF);
        assert!(!l.is_frozen());
        assert!(l.is_plottable());
        assert_eq!(l.lineweight, 31);
        assert_eq!(l.color_index, 7);
    }

    #[test]
    fn roundtrip_frozen_locked_layer() {
        let mut w = BitWriter::new();
        let s = b"HIDDEN";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
        // 0x01 frozen | 0x04 frozen in new viewports | 0x08 locked
        w.write_bs(0x0D);
        w.write_b(false); // not plottable
        w.write_bs(0);
        w.write_bs(1);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert!(l.is_frozen());
        assert!(l.is_frozen_in_new_viewports());
        assert!(l.is_locked());
        assert!(!l.is_plottable());
    }

    /// `0x04` is frozen-in-new-viewports, not locked — the two bits
    /// must not be confused (#26).
    #[test]
    fn frozen_in_new_viewports_is_not_locked() {
        let mut w = BitWriter::new();
        let s = b"VpFrozen";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
        w.write_bs(LAYER_VALUE_FROZEN_IN_NEW);
        w.write_b(true);
        w.write_bs(0);
        w.write_bs(7);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert!(l.is_frozen_in_new_viewports());
        assert!(!l.is_locked());
        assert!(!l.is_frozen());
    }
}
