//! SORTENTSTABLE (§20.4.93) — the draw order of a block's entities.
//!
//! The class is `AcDbSortentsTable`, DXF name `SORTENTSTABLE`, a custom class
//! whose object type code is assigned per file through `AcDb:Classes`. The
//! table belongs to a block record (model space, a paper space or a block);
//! it maps entity handles to the handle each is sorted by.
//!
//! # Field list
//!
//! §20.4.93 prints the data stream as `BL numentries` followed by that many
//! sort handles, typed `H` with "CODE 0, i.e. part of the main bit stream,
//! not of the handle bit stream". Their entity handles are the entity
//! handles of the handle stream, in the same order, and the DXF writes them
//! as pairs (group 331, the entity, then group 5, the sort handle).
//!
//! A sort handle has handle code 0, so its counter bytes are the absolute
//! handle; a handle with another code, which would be an offset from this
//! record's own handle, is refused rather than read as a handle.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

/// Fewest bits a handle takes: its code and counter nibbles.
const MIN_HANDLE_BITS: usize = 8;

/// A decoded SORTENTSTABLE.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SortEntsTable {
    /// The sort handle of each entry, in stream order (DXF group 5 after the
    /// record's own handle). The entity each sorts is the matching handle of
    /// the handle stream.
    pub sort_handles: Vec<u64>,
}

fn read_body(
    c: &mut BitCursor<'_>,
    _strings: &mut Option<StringReader<'_>>,
) -> Result<SortEntsTable> {
    let count = c.read_bl_u()? as usize;
    if count > c.remaining_bits() / MIN_HANDLE_BITS {
        return Err(Error::SectionMap(format!(
            "SORTENTSTABLE claims {count} entries in {} remaining bits",
            c.remaining_bits()
        )));
    }
    let mut sort_handles = Vec::with_capacity(count);
    for _ in 0..count {
        let h = c.read_handle()?;
        if h.code != 0 {
            return Err(Error::SectionMap(format!(
                "SORTENTSTABLE sort handle has code {}, not the absolute code 0",
                h.code
            )));
        }
        sort_handles.push(h.value);
    }
    Ok(SortEntsTable { sort_handles })
}

/// Decode a SORTENTSTABLE from its raw object payload (§20.4.93): `BL`
/// numentries and that many code-0 `H` sort handles, ending exactly on the
/// data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<SortEntsTable> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings)?;
    split.finish("SORTENTSTABLE")?;
    Ok(out)
}
