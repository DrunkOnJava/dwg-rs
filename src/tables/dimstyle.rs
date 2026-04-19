//! DIMSTYLE table entry (§19.5.52) — dimension style.
//!
//! DIMSTYLE has ~75 fields (arrow size, extension line offsets, text
//! gap, every aspect of dim geometry). For the initial release we
//! surface the handful of fields most commonly consulted for
//! compatibility + rendering, and skip the rest by reading them but
//! not storing.
//!
//! The spec lists every field explicitly — to do a full DIMSTYLE
//! we'd recreate 75 `read_bd/bs/b/rc` calls. That's both mechanical
//! and low-value for the typical tooling use case. This decoder
//! covers the core dimensional values.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::tables::{TableEntryHeader, read_table_entry_header, read_tv};
use crate::version::Version;

/// Commonly-used DIMSTYLE attributes.
///
/// Callers who need the full 75-field record should call
/// [`decode_partial`] to advance past the preamble and then manually
/// read the remaining BDs in spec order.
#[derive(Debug, Clone, PartialEq)]
pub struct DimStyle {
    pub header: TableEntryHeader,
    pub dimpost: String,
    pub dimapost: String,
    pub dimscale: f64,
    pub dimasz: f64,
    pub dimexo: f64,
    pub dimdli: f64,
    pub dimexe: f64,
    pub dimrnd: f64,
    pub dimdle: f64,
    pub dimtp: f64,
    pub dimtm: f64,
    pub dimtxt: f64,
    pub dimcen: f64,
}

pub fn decode_partial(c: &mut BitCursor<'_>, version: Version) -> Result<DimStyle> {
    let header = read_table_entry_header(c, version)?;
    let dimpost = read_tv(c, version)?;
    let dimapost = read_tv(c, version)?;
    let dimscale = c.read_bd()?;
    let dimasz = c.read_bd()?;
    let dimexo = c.read_bd()?;
    let dimdli = c.read_bd()?;
    let dimexe = c.read_bd()?;
    let dimrnd = c.read_bd()?;
    let dimdle = c.read_bd()?;
    let dimtp = c.read_bd()?;
    let dimtm = c.read_bd()?;
    let dimtxt = c.read_bd()?;
    let dimcen = c.read_bd()?;
    Ok(DimStyle {
        header,
        dimpost,
        dimapost,
        dimscale,
        dimasz,
        dimexo,
        dimdli,
        dimexe,
        dimrnd,
        dimdle,
        dimtp,
        dimtm,
        dimtxt,
        dimcen,
    })
}

pub use decode_partial as decode;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_standard_dimstyle() {
        let mut w = BitWriter::new();
        let s = b"Standard";
        w.write_bs_u(s.len() as u16);
        for b in s { w.write_rc(*b); }
        w.write_b(false); w.write_bs(0); w.write_b(false);
        w.write_bs_u(0); // empty dimpost
        w.write_bs_u(0); // empty dimapost
        for v in [1.0, 0.18, 0.0625, 0.38, 0.18, 0.0, 0.0, 0.0, 0.0, 0.18, 0.09] {
            w.write_bd(v);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode_partial(&mut c, Version::R2000).unwrap();
        assert_eq!(d.header.name, "Standard");
        assert_eq!(d.dimscale, 1.0);
        assert_eq!(d.dimasz, 0.18);
        assert_eq!(d.dimtxt, 0.18);
        assert_eq!(d.dimcen, 0.09);
    }
}
