//! APPID table entry (§19.5.50) — registered application name used
//! as a key in XDATA (extended entity data) appid handles.
//!
//! # Stream shape
//!
//! ```text
//! entry header (name + xref bits)
//! RC     unknown          -- always 0x00 in practice
//! ```
//!
//! APPID is the simplest symbol-table entry: its usefulness is
//! entirely in the `name` field, which acts as a lookup key when
//! decoding XDATA blocks on other entities.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::tables::{TableEntryHeader, read_table_entry_header};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppId {
    pub header: TableEntryHeader,
    pub unknown: u8,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AppId> {
    let header = read_table_entry_header(c, version)?;
    let unknown = c.read_rc()?;
    Ok(AppId { header, unknown })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_acad_appid() {
        let mut w = BitWriter::new();
        let s = b"ACAD";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
        w.write_rc(0x00);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let a = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(a.header.name, "ACAD");
    }
}
