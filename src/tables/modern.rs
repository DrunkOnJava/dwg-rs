//! Shared plumbing for R2007+ split-stream symbol-table decoders
//! (ODA Open Design Specification v5.4.1 §19.1 + §20.4.x).
//!
//! Every table entry in an R2007+ file is laid out the same way: the
//! object header, then the object's common prefix (EED, reactor count,
//! xdictionary flag, binary-data flag), then the entry's non-string fields, and
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

/// Skip the common object prefix every §20.4.x table section prints:
/// the EED chain, `BL num_reactors`, `B` xdictionary-missing (R2004+)
/// and `B` has-AcDs-binary-data (R2013+). It is the prefix
/// [`crate::objects::modern`] reads for every other non-entity object.
///
/// # Measured (#40, ARCHITECTURE §7v)
///
/// This function used to skip the `BL`, so the reads after it started
/// two bits early. With no reactors the `BL` is the code `10`, and
/// the shifted reads consumed the same number of bits, so the record
/// still closed: the old 64-flag was the xdictionary-missing bit, and on
/// R2013 and R2018 the old Xdep was the binary-data bit. With reactors
/// it broke. On R2013 and R2018 the `BL`'s `01` code and its byte were
/// taken for the two flags and an `RC` said to follow them, and on
/// R2007 and R2010 the three DIMSTYLE records with reactors (`0x39A`,
/// `0x994`, `0xC88`: 8, 2 and 1) failed.
///
/// `examples/probe_table_prefix.rs` checks the prefix read here against
/// AutoCAD's DXF export of the same drawings. On every table entry of
/// the 32 R2000-R2018 files with a twin, the `BL` equals the number of
/// `330` groups in the entry's `{ACAD_REACTORS` bracket, and the R2004+
/// xdictionary bit equals the absence of a `{ACAD_XDICTIONARY` bracket
/// (apart from three round-trip dictionaries only the export holds).
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
    let _num_reactors = c.read_bl()?;
    if version.is_r2004_plus() {
        let _no_xdictionary = c.read_b()?;
    }
    if matches!(version, Version::R2013 | Version::R2018) {
        let _has_binary_data = c.read_b()?;
    }
    Ok(())
}

/// Read the non-string fields every table entry shares once its name
/// has moved to the string stream, returned as `(64-flag, xrefindex+1,
/// Xdep)`.
///
/// # Measured (#40, ARCHITECTURE §7v)
///
/// §20.4.x prints `TV name, B 64-flag, BS xrefindex+1, B Xdep` for every
/// release, and R2000 and R2004 records hold exactly that. From R2007,
/// once [`skip_object_prefix`] reads the `BL` reactor count, two bits
/// remain before the record's own fields on every table entry of the
/// corpus, so no `BS` is read here and `xrefindex+1` is returned as 0.
///
/// The two bits are `1, 0` on all 676 R2007-R2018 table entries of the
/// 32 files with a DXF twin, which is what the same entries hold in
/// their R2000 and R2004 saves (64-flag set, `xrefindex+1` 0, Xdep
/// clear on all 402), and Xdep equals bit 16 of the exported group 70
/// wherever it is compared. So they are read as `B 64-flag, B Xdep`;
/// the swapped order would set Xdep on every entry, which the export
/// rules out. A lone `BS xrefindex+1` of 0 is the same two bits, and the
/// corpus holds no xref-dependent entry to tell them apart (#290).
pub(crate) fn read_entry_flags(c: &mut BitCursor<'_>) -> Result<(bool, i16, bool)> {
    let flag64 = c.read_b()?;
    let is_xref_dependent = c.read_b()?;
    Ok((flag64, 0, is_xref_dependent))
}

/// Read the full `CMC` colour form used by the R2007+ VIEW / VPORT
/// records: `BS` colour index, `BL` true-colour word, `RC` colour byte.
///
/// # Measured, not assumed
///
/// §2.11 makes the `BL` and trailing byte conditional on flag bits in
/// the `BS`. The ambient-colour field of VIEW (`view_custom` in
/// `sample_AC1032.dwg`, data bits 377..421 of the object body) carries
/// index `0` — no flags set — yet is still followed by
/// `BL = 0xC2333333` (RGB 51/51/51) and `RC = 0` and only that reading
/// lands the record on its string stream. So this form is
/// unconditional wherever it is used.
pub(crate) fn read_cmc_full(c: &mut BitCursor<'_>) -> Result<(u16, u32, u8)> {
    let index = c.read_bs_u()?;
    let rgb = c.read_bl_u()?;
    let color_byte = c.read_rc()?;
    Ok((index, rgb, color_byte))
}

/// Read a `4BITS` field (§2.1) — four raw bits, MSB first. Used by the
/// VIEWMODE field of VIEW and VPORT.
pub(crate) fn read_4bits(c: &mut BitCursor<'_>) -> Result<u8> {
    let mut v = 0u8;
    for _ in 0..4 {
        v = (v << 1) | u8::from(c.read_b()?);
    }
    Ok(v)
}

/// Open the string stream of an R2007+ *entity*.
///
/// Entities carry their own common preamble
/// ([`crate::common_entity::read_common_entity_data`]), so unlike
/// [`open_table_entry`] this only returns the string reader and the bit
/// at which the entity's data fields must end.
///
/// A record whose `strings present` trailer bit is clear gets an empty
/// reader ([`StringReader::empty`]) and the start of its handle stream
/// as the bound: its `TV` slots exist but hold nothing, so a decoder
/// must still take them from the (empty) string stream rather than
/// inline.
pub(crate) fn open_entity(payload: &[u8], version: Version) -> Result<(StringReader<'_>, usize)> {
    let end = string_stream::data_field_end(payload, version)
        .ok_or_else(|| Error::SectionMap("object has no R2007+ data/handle stream split".into()))?;
    match string_stream::locate(payload, version) {
        Some(stream) => Ok((StringReader::new(payload, stream)?, end)),
        None => Ok((StringReader::empty(payload), end)),
    }
}

/// Read a `TV` field from whichever stream actually holds it.
///
/// `Some(reader)` means the caller opened the object's R2007+ string
/// stream: the field's characters live there and the slot consumes no
/// data-stream bits, so `c` is left untouched. `None` is the pre-R2007
/// inline layout.
///
/// This is what lets one field-list implementation serve both layouts
/// for records whose only R2007+ difference is where the text sits.
pub(crate) fn read_tv_field(
    c: &mut BitCursor<'_>,
    version: Version,
    strings: Option<&mut StringReader<'_>>,
) -> Result<String> {
    match strings {
        Some(reader) => reader.read_tv(),
        None => crate::tables::read_tv(c, version),
    }
}

/// Error describing a data cursor that did not land on the string stream.
pub(crate) fn misaligned(what: &str, at: usize, string_start: usize) -> Error {
    Error::SectionMap(format!(
        "{what} data fields ended at bit {at}, string stream starts at {string_start} \
         (delta {})",
        at as isize - string_start as isize
    ))
}
