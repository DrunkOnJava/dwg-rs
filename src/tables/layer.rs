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
use crate::tables::{TableEntryHeader, read_table_entry_header};
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
    pub fn is_frozen(&self) -> bool {
        self.flags & 0x01 != 0
    }
    pub fn is_locked(&self) -> bool {
        self.flags & 0x04 != 0
    }
    pub fn is_plottable(&self) -> bool {
        self.plot_flag
    }
}

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
