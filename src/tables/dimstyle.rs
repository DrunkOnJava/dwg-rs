//! DIMSTYLE table entry (ODA Open Design Specification v5.4.1 §19.5.5,
//! L6-05) — dimension style.
//!
//! DIMSTYLE carries ~75 dimension-variable fields (dimscale, dimasz,
//! dimexo, dimexe, dimtxt, dimcen, ...). Covering 100% of them is
//! mostly mechanical — the full list is mirrored in the AutoCAD DIMVAR
//! table — and balloons the decoder without proportional value for
//! typical use.
//!
//! This decoder implements the 15 most-consulted fields in spec order
//! and surfaces them as a [`DimStyleEntry`]:
//!
//! | Slot | Field     | Type |
//! |------|-----------|------|
//! | 1    | dimscale  | BD   |
//! | 2    | dimasz    | BD   |
//! | 3    | dimexo    | BD   |
//! | 4    | dimexe    | BD   |
//! | 5    | dimtxt    | BD   |
//! | 6    | dimcen    | BD   |
//! | 7    | dimtfac   | BD   |
//! | 8    | dimlfac   | BD   |
//! | 9    | dimtih    | B    |
//! | 10   | dimtoh    | B    |
//! | 11   | dimtad    | RC   |
//! | 12   | dimtolj   | RC   |
//! | 13   | dimaltf   | BD   |
//! | 14   | dimaltrnd | BD   |
//! | 15   | dimupt    | B    |
//!
//! # Cutoff
//!
//! Fields past `dimupt` are left in the stream — callers that need the
//! full record must layer a more specific decoder on top. This is a
//! deliberate scope cut: §19.5.5 has no stable layout change between
//! R2000 and R2018 for these 15 slots, but fields past slot 15 shift
//! position by version and are best read as a version-gated second
//! pass.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::tables::{TableEntryHeader, read_table_entry_header};
use crate::version::Version;

/// Partial DIMSTYLE: 15 rendering-essential dimension variables plus the
/// entry header. See module docstring for the cutoff rationale.
#[derive(Debug, Clone, PartialEq)]
pub struct DimStyleEntry {
    pub header: TableEntryHeader,
    pub dimscale: f64,
    pub dimasz: f64,
    pub dimexo: f64,
    pub dimexe: f64,
    pub dimtxt: f64,
    pub dimcen: f64,
    pub dimtfac: f64,
    pub dimlfac: f64,
    pub dimtih: bool,
    pub dimtoh: bool,
    pub dimtad: u8,
    pub dimtolj: u8,
    pub dimaltf: f64,
    pub dimaltrnd: f64,
    pub dimupt: bool,
}

// Legacy alias retained so callers keep compiling while they migrate to
// [`DimStyleEntry`].
pub type DimStyle = DimStyleEntry;

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<DimStyleEntry> {
    let header = read_table_entry_header(c, version)?;
    let dimscale = c.read_bd()?;
    let dimasz = c.read_bd()?;
    let dimexo = c.read_bd()?;
    let dimexe = c.read_bd()?;
    let dimtxt = c.read_bd()?;
    let dimcen = c.read_bd()?;
    let dimtfac = c.read_bd()?;
    let dimlfac = c.read_bd()?;
    let dimtih = c.read_b()?;
    let dimtoh = c.read_b()?;
    let dimtad = c.read_rc()?;
    let dimtolj = c.read_rc()?;
    let dimaltf = c.read_bd()?;
    let dimaltrnd = c.read_bd()?;
    let dimupt = c.read_b()?;
    Ok(DimStyleEntry {
        header,
        dimscale,
        dimasz,
        dimexo,
        dimexe,
        dimtxt,
        dimcen,
        dimtfac,
        dimlfac,
        dimtih,
        dimtoh,
        dimtad,
        dimtolj,
        dimaltf,
        dimaltrnd,
        dimupt,
    })
}

// Legacy alias — the historical public API called this `decode_partial`.
pub use decode as decode_partial;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn write_header(w: &mut BitWriter, name: &[u8]) {
        w.write_bs_u(name.len() as u16);
        for b in name {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
    }

    #[test]
    fn roundtrip_standard_dimstyle() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"Standard");
        w.write_bd(1.0); // dimscale
        w.write_bd(0.18); // dimasz
        w.write_bd(0.0625); // dimexo
        w.write_bd(0.18); // dimexe
        w.write_bd(0.18); // dimtxt
        w.write_bd(0.09); // dimcen
        w.write_bd(1.0); // dimtfac
        w.write_bd(1.0); // dimlfac
        w.write_b(true); // dimtih
        w.write_b(true); // dimtoh
        w.write_rc(0); // dimtad
        w.write_rc(1); // dimtolj
        w.write_bd(25.4); // dimaltf
        w.write_bd(0.0); // dimaltrnd
        w.write_b(false); // dimupt
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(d.header.name, "Standard");
        assert_eq!(d.dimscale, 1.0);
        assert_eq!(d.dimasz, 0.18);
        assert_eq!(d.dimtxt, 0.18);
        assert_eq!(d.dimcen, 0.09);
        assert_eq!(d.dimtad, 0);
        assert_eq!(d.dimtolj, 1);
        assert!(d.dimtih);
        assert!(d.dimtoh);
        assert!(!d.dimupt);
        assert!((d.dimaltf - 25.4).abs() < 1e-12);
    }

    #[test]
    fn roundtrip_metric_dimstyle() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"ISO-25");
        w.write_bd(1.0); // dimscale
        w.write_bd(2.5); // dimasz mm
        w.write_bd(0.625);
        w.write_bd(1.25);
        w.write_bd(2.5); // dimtxt
        w.write_bd(0.0); // dimcen disabled
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_b(false);
        w.write_b(false);
        w.write_rc(1); // dimtad = above
        w.write_rc(0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_b(true); // dimupt = user positioning
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(d.header.name, "ISO-25");
        assert_eq!(d.dimasz, 2.5);
        assert!(!d.dimtih);
        assert_eq!(d.dimtad, 1);
        assert!(d.dimupt);
    }
}
