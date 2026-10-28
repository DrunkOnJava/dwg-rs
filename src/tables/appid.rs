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
/// After the common object prefix the data stream carries only
/// `B 64-flag`, `B xdep` and the trailing unknown `RC`; the name is the
/// single entry in the string stream. Verified against every APPID
/// record in `sample_AC1032.dwg`, `line_2013.dwg` and `arc_2010.dwg`,
/// where those fields land exactly on the string-stream start.
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
            flag_64: flag64,
            xref_index_plus_1,
            is_xref_dependent,
        },
        unknown,
    })
}
