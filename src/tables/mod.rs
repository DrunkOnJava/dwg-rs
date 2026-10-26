//! Symbol-table entry decoders (spec §19.5.x).
//!
//! Symbol tables map a 32-bit handle to a named record. Each table
//! kind has its own entry shape:
//!
//! | Table        | Record type   | Module      |
//! |--------------|---------------|-------------|
//! | LAYER        | LAYER entry   | [`layer`]   |
//! | LTYPE        | LTYPE entry   | [`ltype`]   |
//! | STYLE        | STYLE entry   | [`style`]   |
//! | VIEW         | VIEW entry    | [`view`]    |
//! | UCS          | UCS entry     | [`ucs`]     |
//! | VPORT        | VPORT entry   | [`vport`]   |
//! | APPID        | APPID entry   | [`appid`]   |
//! | DIMSTYLE     | DIMSTYLE entry| [`dimstyle`]|
//! | BLOCK_HEADER | block record  | [`block_record`] |
//!
//! All entries share a small header (flag byte + TV name + xref
//! dependency bits); the per-kind trailer carries the
//! kind-specific attributes.

pub mod appid;
pub mod block_record;
pub mod dimstyle;
pub mod handles;
pub mod layer;
pub mod ltype;
pub(crate) mod modern;
pub mod style;
pub mod ucs;
pub mod view;
pub mod vport;

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::version::Version;

/// Header shared by every symbol-table entry.
///
/// Immediately after the common entity preamble (handled by the
/// caller), each table entry writes:
///
/// ```text
/// TV    name                  (2)
/// B     64-flag               (70, bit 64)
/// BS    xrefindex+1           -- 0 if not from an xref
/// B     Xdep                  (70, bit 16)  -- dependent on an xref
/// ```
///
/// That is the order §20.4.54 (and every other table section) prints,
/// and the order R2000 and R2004 records measurably hold. From R2007
/// the name is in the string stream and two bits remain, read as
/// `B 64-flag, B Xdep` with no `xrefindex+1`
/// (`tables::modern::read_entry_flags`, #40). The
/// DXF value oracle (#99) measures both flags on every table record of
/// the corpus: the 64-flag is set on every entry AutoCAD writes to DWG
/// (its DXF export never writes bit 64), and the Xdep bit equals bit 16
/// of the exported group 70.
///
/// This struct captures those shared fields. Per-kind decoders append
/// to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableEntryHeader {
    pub name: String,
    /// The 64-flag: bit 64 of the entry's DXF group 70.
    pub flag_64: bool,
    /// `xrefindex+1`: 0 when the entry did not come from an xref. No
    /// field of an R2007 or later record is read as it, so it is 0 there.
    pub xref_index_plus_1: i16,
    /// Xdep: the entry depends on an external reference (bit 16 of the
    /// entry's DXF group 70).
    pub is_xref_dependent: bool,
}

/// Reads the name / 64-flag / xref-index / Xdep header shared by every table entry (§20.4.54).
pub fn read_table_entry_header(
    c: &mut BitCursor<'_>,
    version: Version,
) -> Result<TableEntryHeader> {
    let name = read_tv(c, version)?;
    let flag_64 = c.read_b()?;
    let xref_index_plus_1 = c.read_bs()?;
    let is_xref_dependent = c.read_b()?;
    Ok(TableEntryHeader {
        name,
        flag_64,
        xref_index_plus_1,
        is_xref_dependent,
    })
}

/// Shared TV reader used by every table-entry module. Version-aware:
/// R2007+ uses UTF-16LE, earlier versions use 8-bit ASCII/MBCS.
pub(crate) fn read_tv(c: &mut BitCursor<'_>, version: Version) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    if version.is_r2007_plus() {
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            let lo = c.read_rc()? as u16;
            let hi = c.read_rc()? as u16;
            units.push((hi << 8) | lo);
        }
        if units.last() == Some(&0) {
            units.pop();
        }
        String::from_utf16(&units)
            .map_err(|_| Error::SectionMap("table entry name is not valid UTF-16".into()))
    } else {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(c.read_rc()?);
        }
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_entry_header() {
        let mut w = BitWriter::new();
        // TV name
        let s = b"MyLayer";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_b(false); // 64-flag
        w.write_bs(0);
        w.write_b(false); // not xref dependent
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = read_table_entry_header(&mut c, Version::R2000).unwrap();
        assert_eq!(h.name, "MyLayer");
        assert!(!h.flag_64);
        assert_eq!(h.xref_index_plus_1, 0);
        assert!(!h.is_xref_dependent);
    }
}
