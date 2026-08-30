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
use crate::tables::{TableEntryHeader, modern, read_table_entry_header};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppId {
    pub header: TableEntryHeader,
    pub unknown: u8,
}

/// Decodes a `AppId` table entry that follows the common object header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AppId> {
    let header = read_table_entry_header(c, version)?;
    let unknown = c.read_rc()?;
    Ok(AppId { header, unknown })
}

/// Decode an R2007+ APPID whose name lives in the object's string
/// stream (ODA v5.4.1 §19.1 split layout, §20.4.50 APPID field table).
///
/// The data stream carries only `B 64-flag`, `B xdep`, `BS xrefindex+1`
/// and the trailing unknown `RC`; the name is the single entry in the
/// string stream. Verified against every APPID record in
/// `sample_AC1032.dwg`, `line_2013.dwg` and `arc_2010.dwg`, where those
/// 16 bits land exactly on the string-stream start.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<AppId> {
    let mut split = modern::open_table_entry(payload, object_body_start, version)?;
    let (flag64, xref_index_plus_1, is_xref_dependent) = modern::read_entry_flags(&mut split.data)?;
    let unknown = split.data.read_rc()?;
    split.finish("APPID")?;
    let name = split.strings.read_tv()?;
    Ok(AppId {
        header: TableEntryHeader {
            name,
            is_xref_dependent,
            xref_index_plus_1,
            is_xref_resolved: flag64,
        },
        unknown,
    })
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

    #[test]
    fn r2007_split_stream_appid_reads_name_from_string_stream() {
        let mut body = BitWriter::new();
        body.write_bs_u(0); // no EED
        body.write_b(true); // no xdictionary
        body.write_b(false); // no binary data
        body.write_b(false); // 64-flag
        body.write_b(false); // xref dependent
        body.write_bs(0); // xref index + 1
        body.write_rc(0x00); // trailing unknown
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["AcadAnnotative"]);
        let a = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(a.header.name, "AcadAnnotative");
        assert_eq!(a.unknown, 0);
    }
}
