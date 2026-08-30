//! Shared plumbing for R2007+ split-stream symbol-table decoders
//! (ODA Open Design Specification v5.4.1 §19.1 + §20.4.x).
//!
//! Every table entry in an R2007+ file is laid out the same way: the
//! object header, then the object's common prefix (EED, xdictionary
//! flag, binary-data flag), then the entry's non-string fields, and
//! finally — in the separate string stream located by
//! [`crate::string_stream`] — the entry's `TV` fields in field-table
//! order.
//!
//! The critical invariant this module enforces is that the data-stream
//! fields end *exactly* where the string stream begins. A decoder that
//! lands anywhere else has mis-read a field, so [`SplitStream::finish`]
//! turns that into an error instead of returning plausible-looking
//! garbage.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::string_stream::{self, StringReader, StringStream};
use crate::version::Version;

/// A table entry's data cursor plus its string-stream reader.
pub(crate) struct SplitStream<'a> {
    /// Cursor over the object's non-string fields.
    pub data: BitCursor<'a>,
    /// Reader over the object's `TV` fields.
    pub strings: StringReader<'a>,
    /// Bit at which the data fields must end.
    pub string_start: usize,
}

impl SplitStream<'_> {
    /// Verify the data cursor consumed exactly the data stream, no more.
    ///
    /// `what` names the record for the error message.
    pub fn finish(&self, what: &str) -> Result<()> {
        let at = self.data.position_bits();
        if at == self.string_start {
            return Ok(());
        }
        Err(Error::SectionMap(format!(
            "{what} data fields ended at bit {at}, string stream starts at {} \
             (delta {})",
            self.string_start,
            at as isize - self.string_start as isize
        )))
    }
}

/// Open the split streams of an R2007+ symbol-table entry.
///
/// `object_body_start` is the bit just past the object header (type
/// code + handle), i.e. where the object's EED loop begins. The data
/// cursor is returned positioned past the common object prefix.
pub(crate) fn open_table_entry<'a>(
    payload: &'a [u8],
    object_body_start: usize,
    version: Version,
) -> Result<SplitStream<'a>> {
    let stream = locate_stream(payload, version)?;
    let mut data = BitCursor::new(payload);
    string_stream::seek(&mut data, object_body_start)?;
    skip_object_prefix(&mut data, version)?;
    Ok(SplitStream {
        data,
        strings: StringReader::new(payload, stream)?,
        string_start: stream.start_bit,
    })
}

fn locate_stream(payload: &[u8], version: Version) -> Result<StringStream> {
    string_stream::locate(payload, version)
        .ok_or_else(|| Error::SectionMap("object has no R2007+ string stream".into()))
}

/// Skip the common object prefix: EED chain, xdictionary flag (R2004+),
/// binary-data flag (R2013+). Per §19.4.2 non-entity objects carry no
/// graphics block and no reactor count at this point.
pub(crate) fn skip_object_prefix(c: &mut BitCursor<'_>, version: Version) -> Result<()> {
    const MAX_XDATA_ITERATIONS: usize = 256;
    for _ in 0..MAX_XDATA_ITERATIONS {
        let size = c.read_bs_u()? as usize;
        if size == 0 {
            break;
        }
        let _appid = c.read_handle()?;
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
    }
    if version.is_r2004_plus() {
        let _no_xdictionary = c.read_b()?;
    }
    if matches!(version, Version::R2013 | Version::R2018) {
        let _has_binary_data = c.read_b()?;
    }
    Ok(())
}

/// Read the three non-string fields every table entry shares once its
/// name has moved to the string stream: the 64-flag, the
/// xref-dependent flag, and the xref index (§20.4.x "common table
/// entry fields").
///
/// # Measured field order
///
/// The pre-R2007 inline layout is `TV name, B 64-flag, BS xrefindex+1,
/// B xdep`. With the name gone the remaining three are observed in the
/// order `B, B, BS` — not `B, BS, B`. Evidence: in every STYLE record
/// of `sample_AC1032.dwg` only this order leaves the two-bit `BS` on a
/// `10` (value 0) code and lands the field body exactly on the string
/// stream; the alternative puts the `BS` on an `01` code and yields an
/// xref index of 83 for `Standard`.
pub(crate) fn read_entry_flags(c: &mut BitCursor<'_>) -> Result<(bool, i16, bool)> {
    let flag64 = c.read_b()?;
    let is_xref_dependent = c.read_b()?;
    let xref_index_plus_1 = c.read_bs()?;
    Ok((flag64, xref_index_plus_1, is_xref_dependent))
}
