//! LAYER table entry (§19.5.57) — named drawing layer with
//! display flags, color, linetype, lineweight, plot style.
//!
//! # Stream shape (R2004+)
//!
//! ```text
//! common entity preamble     -- handled by caller
//! TV     name                -- handled by [`read_table_entry_header`]
//! B      is_xref_dependent
//! BS     xref_index_plus_1
//! B      is_xref_resolved
//! BS     flags               -- frozen/locked/plot/xref bits
//! B      plotflag            -- R2000+
//! BS     lineweight           -- R2000+ (enum codes, not mm)
//! CMC    color                -- indexed or RGB
//! ```

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
    /// Bit 0x01 of `flags`: the layer is frozen.
    pub fn is_frozen(&self) -> bool {
        self.flags & 0x01 != 0
    }
    /// Bit 0x04 of `flags`: the layer is locked.
    pub fn is_locked(&self) -> bool {
        self.flags & 0x04 != 0
    }
    /// The layer's plot flag (plotted when set).
    pub fn is_plottable(&self) -> bool {
        self.plot_flag
    }
}

/// Decodes a `Layer` table entry that follows the common object header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Layer> {
    let header = read_table_entry_header(c, version)?;
    let flags = c.read_bs()?;
    let plot_flag = if matches!(
        version,
        Version::R2000
            | Version::R2004
            | Version::R2007
            | Version::R2010
            | Version::R2013
            | Version::R2018
    ) {
        c.read_b()?
    } else {
        true
    };
    let lineweight = c.read_bs()?;
    let color_index = c.read_bs()?; // CMC simplified to BS
    Ok(Layer {
        header,
        flags,
        plot_flag,
        lineweight,
        color_index,
    })
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
/// this is *not* the DXF group-70 flag word, where locked is `0x04`:
/// [`Layer::is_locked`] still tests `0x04` and so under-reports on
/// R2007+ files. Fixing it changes pre-R2007 behaviour too, so it is
/// left for a separate change.
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
        // Body
        w.write_bs(0); // flags
        w.write_b(true); // plottable
        w.write_bs(-3); // lineweight = BYBLOCK
        w.write_bs(7); // color = white
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(l.header.name, "0");
        assert!(l.is_plottable());
        assert_eq!(l.color_index, 7);
        assert!(!l.is_frozen());
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
        w.write_bs(0x05); // frozen + locked
        w.write_b(false); // not plottable
        w.write_bs(0);
        w.write_bs(1);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert!(l.is_frozen());
        assert!(l.is_locked());
        assert!(!l.is_plottable());
    }
}
