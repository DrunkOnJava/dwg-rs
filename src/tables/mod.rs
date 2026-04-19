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
pub mod layer;
pub mod ltype;
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
/// TV    name
/// B     is_xref_dependent     -- entry from an external reference
/// BS    xref_index_plus_1     -- 0 if not xref-dependent
/// B     is_xref_resolved
/// ```
///
/// This struct captures those shared fields. Per-kind decoders append
/// to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableEntryHeader {
    pub name: String,
    pub is_xref_dependent: bool,
    pub xref_index_plus_1: i16,
    pub is_xref_resolved: bool,
}

pub fn read_table_entry_header(
    c: &mut BitCursor<'_>,
    version: Version,
) -> Result<TableEntryHeader> {
    let name = read_tv(c, version)?;
    let is_xref_dependent = c.read_b()?;
    let xref_index_plus_1 = c.read_bs()?;
    let is_xref_resolved = c.read_b()?;
    Ok(TableEntryHeader {
        name,
        is_xref_dependent,
        xref_index_plus_1,
        is_xref_resolved,
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
        w.write_b(false); // not xref dependent
        w.write_bs(0);
        w.write_b(false);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let h = read_table_entry_header(&mut c, Version::R2000).unwrap();
        assert_eq!(h.name, "MyLayer");
        assert!(!h.is_xref_dependent);
        assert_eq!(h.xref_index_plus_1, 0);
        assert!(!h.is_xref_resolved);
    }
}
