//! The handle references of a symbol-table entry (ODA Open Design
//! Specification v5.4.1 §20.4.x "Handle refs").
//!
//! Every symbol-table entry ends with its handle stream. The first handles are
//! common to all of them: the table's control object (soft pointer, the owner),
//! one soft pointer per persistent reactor, and, unless R2004+'s `XDic Missing
//! Flag` is set, the extension dictionary (hard owner). The handles of the
//! entry's own fields follow, and each §20.4.x section lists them in order.
//!
//! [`refs`] reads the common ones and returns the rest as absolute handles, so a
//! caller that knows its table's list picks them by position. Relative handle
//! codes (6, 8, A, C) are resolved against the record's own handle, and a NULL
//! reference reads as 0.
//!
//! The reactor count and the xdictionary flag come from the common object data
//! of the data stream ([`crate::common_entity::read_common_object_data_full`]);
//! the handle stream starts where [`super::block_record::handle_stream_start`]
//! puts it on each release.

use crate::bitcursor::{BitCursor, Handle};
use crate::common_entity::read_common_object_data_full;
use crate::error::{Error, Result};
use crate::object::{RawObject, body_cursor};
use crate::string_stream;
use crate::tables::block_record::handle_stream_start;
use crate::version::Version;

/// Bound on the handles of one entry: a model space owns every entity of the
/// drawing, so the cap only stops a runaway read.
const MAX_REFS: usize = 4_000_000;

/// The handle references of one table entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRefs {
    /// The table's control object.
    pub owner: u64,
    /// The persistent reactors, in stored order.
    pub reactors: Vec<u64>,
    /// The extension dictionary, when the entry has one.
    pub xdictionary: Option<u64>,
    /// The handles of the entry's own fields, in file order, as absolute
    /// handles (0 for a NULL reference).
    pub rest: Vec<u64>,
}

fn resolve(h: Handle, own: u64) -> u64 {
    match h.code {
        0x6 => own.wrapping_add(1),
        0x8 => own.wrapping_sub(1),
        0xA => own.wrapping_add(h.value),
        0xC => own.wrapping_sub(h.value),
        _ => h.value,
    }
}

/// The handle references of the walked table entry `raw`.
///
/// Fails on R13 and R14 (no anchor for the handle stream) and when a handle
/// does not parse.
pub fn refs(raw: &RawObject, version: Version) -> Result<EntryRefs> {
    let mut body = body_cursor(raw, version)?;
    let common = read_common_object_data_full(&mut body, version)?;
    let start = handle_stream_start(raw, version)
        .ok_or_else(|| Error::SectionMap("table entry has no handle stream anchor".into()))?;
    let mut c = BitCursor::new(&raw.raw);
    string_stream::seek(&mut c, start)?;
    let own = raw.handle.value;
    let owner = resolve(c.read_handle()?, own);
    let mut reactors = Vec::new();
    for _ in 0..common.num_reactors {
        if reactors.len() >= MAX_REFS {
            return Err(Error::SectionMap(
                "table entry claims too many reactors".into(),
            ));
        }
        reactors.push(resolve(c.read_handle()?, own));
    }
    let xdictionary = if version.is_r2004_plus() && common.no_xdictionary {
        None
    } else {
        Some(resolve(c.read_handle()?, own)).filter(|h| *h != 0)
    };
    let mut rest = Vec::new();
    while c.remaining_bits() >= 8 {
        if rest.len() >= MAX_REFS {
            return Err(Error::SectionMap(
                "table entry holds too many handles".into(),
            ));
        }
        rest.push(resolve(c.read_handle()?, own));
    }
    Ok(EntryRefs {
        owner,
        reactors,
        xdictionary,
        rest,
    })
}
