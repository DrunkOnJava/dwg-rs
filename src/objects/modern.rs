//! Shared plumbing for the non-entity object decoders — one field list
//! serving both the pre-R2007 inline layout and the R2007+ split-stream
//! layout (ODA Open Design Specification v5.4.1 §19.1 + §19.4.2).
//!
//! # Why this module exists
//!
//! A non-entity object (DICTIONARY, XRECORD, LAYOUT, GROUP, the
//! `*_CONTROL` owners, the ACAD_* dictionary objects) is laid out as
//!
//! ```text
//! object header | common object data | type-specific fields
//! ```
//!
//! and from R2007 onward the type-specific `TV` fields move out of the
//! data stream into the object's string stream ([`crate::string_stream`]),
//! while its `H` references move into the handle stream. Reading `TV`
//! from the data cursor on such a file does not error — it returns
//! whatever bits happen to sit at that offset, which is why the 65
//! DICTIONARY records of `sample_AC1032.dwg` returned junk keys before
//! this module existed.
//!
//! [`ObjectStream`] hides that difference behind [`ObjectStream::read_tv`],
//! so each decoder writes its field list exactly once.
//!
//! # The self-validating boundary — measured, not assumed
//!
//! Every record carries a bit offset at which its data fields *must*
//! end, and [`ObjectStream::finish`] turns any other landing spot into
//! an error rather than a plausible-looking struct:
//!
//! | Release band | Boundary |
//! |---|---|
//! | R2010+, record with strings | first bit of the string stream |
//! | R2010+, record without strings | handle-stream start minus the one `B` "strings present" trailer bit |
//! | R2000-R2007 | the `RL` object-data-size-in-bits from the object prologue ([`crate::object::RawObject::obj_size_bits`]) |
//! | R13/R14 | unknown — no check |
//!
//! The "minus one bit" row is measured, not assumed: §19.1 puts a `B`
//! *strings present* flag at the very end of the data area, and when it
//! is clear that single bit is the whole trailer. Evidence — the
//! ACDB_PLACEHOLDER record of `arc_2013.dwg` (handle 15) has a provably
//! empty body, and its budget from the end of the common object data to
//! the handle-stream start is exactly 1 bit. The nine `*_CONTROL`
//! records of the same file agree: each closes on `BL numentries` with
//! exactly one bit to spare.
//!
//! # The common object prefix carries the `BL` reactor count
//!
//! §19.4.2 gives the shape below, and every field of it is present on
//! R2007+ too:
//!
//! ```text
//! EED chain
//! BL   num_reactors
//! B    no_xdictionary_handle   -- R2004+
//! B    has_ds_binary_data      -- R2013+ (no further bits; see below)
//! ```
//!
//! Measured three independent ways on `sample_AC1032.dwg` (R2018),
//! `arc_2013.dwg`, `arc_2010.dwg` and `arc_2004.dwg`:
//!
//! 1. **DICTIONARY closes exactly** — with the `BL` read, the record's
//!    `BL numitems` decodes to the number of strings its string stream
//!    actually holds (23 for the named-object dictionary, 4 for the
//!    layout dictionary, 2 for the group dictionary, 0 for the empty
//!    ones) and `BS cloning + RC hard-owner` land precisely on the
//!    string-stream start. Dropping the `BL` shifts `numitems` to 0 on
//!    a dictionary that demonstrably has entries.
//! 2. **ACDB_PLACEHOLDER has no body at all** and closes with the `BL`
//!    read and 2 bits left over without it.
//! 3. **The `*_CONTROL` objects** close on their single `BL numentries`
//!    only with the reactor count consumed first.
//!
//! Note that [`crate::tables::modern::skip_object_prefix`] — the R2007+
//! symbol-table variant — deliberately does *not* read this `BL`, and
//! compensates with a different flag order in
//! [`crate::tables::modern::read_entry_flags`]. The two readings sum to
//! the same total for an APPID record, so both satisfy the boundary
//! check; which one assigns the right values to the right fields is not
//! determinable from the 16 bits an APPID record spends there, so this
//! module does not touch the table path.
//!
//! # The R2013+ AcDs binary-data flag carries no further bits
//!
//! The spec's common-object-data table gives only the `B` — "indicates
//! whether the object has associated binary data in the data store
//! section" — and stops. It does not say what, if anything, follows
//! when the bit is set, and until #61 no record whose field list this
//! crate closes exercised it on the *entity* side, so the width could
//! not be measured against a self-validating list. This module read 16
//! bits there, `tables::modern` read 8 and `common_entity` read 0 —
//! three readings of one field (#54).
//!
//! Issue #61 settled it. The three ACIS entity records of
//! `sample_AC1032.dwg` — 3DSOLID `0xD65` / `0xD6A` and REGION `0xD69` —
//! set the bit, and their §20.4.41 field list
//! ([`crate::entities::modeler`]) closes on the data-stream boundary
//! with **zero** bits consumed after the flag. The list is corroborated
//! four independent ways (three real coordinate triples, `num isolines
//! = 4` on every record, a valid RFC-4122 version-4 GUID, and the
//! spec's own grammar for everything else), and it cannot close with a
//! 16-bit marker: the record would then be 16 bits short and the
//! wireframe point would decode to nonsense.
//!
//! So the flag is a flag. The 16 bits four LAYOUT records of the same
//! file need are LAYOUT's own — a data-store block at the head of its
//! §20.4.84 field list, moved to
//! [`crate::objects::acad_layout`] where the evidence for it lives.
//! (`tables::modern`'s 8-bit `RC` is DIMSTYLE's counterpart; that path
//! also omits the `BL num_reactors` this one reads, so its bit
//! accounting is a separate open question and is left alone here.)
//! Three record types needing three different widths is itself the
//! argument that the width belongs to the record, not to the flag.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::string_stream::{self, StringReader};
use crate::version::Version;

/// Maximum EED sub-records honoured before the chain is called malformed.
const MAX_EED_ITERATIONS: usize = 256;

/// One non-entity object's data cursor plus the source its `TV` fields
/// actually come from.
pub(crate) struct ObjectStream<'a> {
    /// Cursor over the object's non-string fields.
    pub data: BitCursor<'a>,
    /// R2007+ string-stream reader; `None` on the inline layout.
    pub strings: Option<StringReader<'a>>,
    /// Bit at which the data fields must end, when it is knowable.
    data_end: Option<usize>,
    /// `BL num_reactors` from the common object data (§19.4.2).
    pub num_reactors: u32,
    /// R2013+ `has AcDs binary data` bit from the common object data
    /// (§20.4.2). The bit itself consumes no further bits — see the
    /// module docs — but a record that sets it may carry a
    /// data-store block of its own, as LAYOUT does.
    pub has_ds_binary_data: bool,
}

/// Read a `TV` from whichever stream actually holds it.
///
/// `Some(reader)` is the R2007+ split layout: the characters live in
/// the string stream and the slot consumes no data-stream bits, so `c`
/// is left untouched. `None` is the pre-R2007 inline layout.
///
/// This is what lets one field-list implementation serve both layouts.
pub(crate) fn read_tv(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<String> {
    match strings.as_mut() {
        Some(reader) => reader.read_tv(),
        None => crate::tables::read_tv(c, version),
    }
}

impl ObjectStream<'_> {
    /// The bit at which the data fields must end, when it is knowable.
    ///
    /// Decoders that close their field list use
    /// [`finish`](Self::finish); this is for the ones that only measure
    /// the budget a future field list will have to fill.
    pub(crate) fn data_end(&self) -> Option<usize> {
        self.data_end
    }

    /// Verify the data cursor consumed exactly the data stream, no more
    /// and no less. `what` names the record for the error message.
    ///
    /// A decoder that lands anywhere else has mis-read a field, so this
    /// is an error rather than a plausible-looking return value.
    pub fn finish(&self, what: &str) -> Result<()> {
        let Some(end) = self.data_end else {
            return Ok(());
        };
        let at = self.data.position_bits();
        if at == end {
            return Ok(());
        }
        Err(Error::SectionMap(format!(
            "{what} data fields ended at bit {at}, data stream ends at {end} (delta {})",
            at as isize - end as isize
        )))
    }
}

/// Open the streams of one non-entity object.
///
/// `body_start` is the bit just past the object header — where the EED
/// chain of the common object data begins, i.e. the position
/// [`crate::object::body_cursor`] leaves its cursor at. `inline_data_end`
/// is [`crate::object::RawObject::obj_size_bits`], used as the boundary
/// on the pre-R2007 inline layout and ignored on R2007+.
///
/// The returned cursor is positioned past the common object data.
pub(crate) fn open<'a>(
    payload: &'a [u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<ObjectStream<'a>> {
    let (strings, mut data_end) = if version.is_r2007_plus() {
        match string_stream::locate(payload, version) {
            Some(stream) => (
                Some(StringReader::new(payload, stream)?),
                Some(stream.start_bit),
            ),
            None => {
                // The lone `B` "strings present" trailer bit is the whole
                // trailer when it is clear, so the data fields end one
                // bit before the handle stream — the rule
                // `string_stream::data_field_end` states once for both
                // the object and the entity paths.
                let end = string_stream::data_field_end(payload, version).ok_or_else(|| {
                    Error::SectionMap("object has no R2007+ data/handle stream split".into())
                })?;
                (Some(StringReader::empty(payload)), Some(end))
            }
        }
    } else {
        (None, inline_data_end)
    };

    let mut data = BitCursor::new(payload);
    string_stream::seek(&mut data, body_start)?;
    let prefix = read_common_object_prefix(&mut data, version)?;
    // R13/R14 record the boundary inside the common object data rather
    // than in the object prologue, so it is only knowable once the prefix
    // has been read (§20.1 — see [`read_common_object_prefix`]).
    if data_end.is_none() {
        data_end = prefix.obj_size_bits.map(|b| b as usize);
    }

    Ok(ObjectStream {
        data,
        strings,
        data_end,
        num_reactors: prefix.num_reactors,
        has_ds_binary_data: prefix.has_ds_binary_data,
    })
}

/// What [`read_common_object_prefix`] recovers from §19.4.2 / §20.1.
struct CommonObjectPrefix {
    /// `BL num_reactors`.
    num_reactors: u32,
    /// R2013+ `has AcDs binary data`.
    has_ds_binary_data: bool,
    /// R13/R14 `RL` object-data-size-in-bits — the record's data-stream
    /// boundary, which those releases write here instead of in the
    /// object prologue.
    obj_size_bits: Option<u32>,
}

/// Consume the common object data of §19.4.2 / §20.1 and return the
/// reactor count, the R2013+ `has AcDs binary data` bit, and — on R13/R14
/// — the record's data-stream boundary.
///
/// The AcDs flag consumes **no further bits** — see the module docs for
/// the three entity records that measure that.
///
/// # R13/R14 put the object size here
///
/// §20.1 lists `RL Size of object data in bits` under "R13-R14",
/// immediately after the EED chain and before `BL Number of persistent
/// reactors`. R2000-R2007 write the same field in the object prologue
/// instead (where [`crate::object::ObjectWalker`] reads it), and R2010+
/// replace it with the leading `MC`.
///
/// Measured: on all 285 non-entity records of `line_R14.dwg`,
/// `arc_R14.dwg` and `circle_R14.dwg` the value read here is `> 0`, no
/// larger than the record's payload in bits, and past the cursor — e.g.
/// BLOCK_CONTROL `0x1` reads 72 of 144 payload bits, DICTIONARY `0xC`
/// reads 3144 of 3536. Skipping it desynchronises every field from
/// `num_reactors` on by exactly 32 bits, which is why every R14
/// DICTIONARY / APPID / LTYPE / DIMSTYLE / BLOCK_HEADER record errored
/// before this read existed.
fn read_common_object_prefix(
    c: &mut BitCursor<'_>,
    version: Version,
) -> Result<CommonObjectPrefix> {
    for _ in 0..MAX_EED_ITERATIONS {
        let size = c.read_bs_u()? as usize;
        if size == 0 {
            let obj_size_bits = if matches!(version, Version::R14) {
                Some(c.read_rl()?)
            } else {
                None
            };
            let num_reactors = c.read_bl()? as u32;
            if version.is_r2004_plus() {
                let _no_xdictionary = c.read_b()?;
            }
            let has_ds_binary_data = if matches!(version, Version::R2013 | Version::R2018) {
                c.read_b()?
            } else {
                false
            };
            return Ok(CommonObjectPrefix {
                num_reactors,
                has_ds_binary_data,
                obj_size_bits,
            });
        }
        let _appid = c.read_handle()?;
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
    }
    Err(Error::SectionMap(format!(
        "common object EED chain exceeded {MAX_EED_ITERATIONS} sub-records"
    )))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Build the common object data an R2018 non-entity object leads
    /// with: empty EED chain, `BL` reactor count, no xdictionary, no
    /// AcDs binary data.
    pub(crate) fn r2018_object_prefix(num_reactors: i32) -> BitWriter {
        let mut w = BitWriter::new();
        w.write_bs_u(0); // EED terminator
        w.write_bl(num_reactors);
        w.write_b(true); // no xdictionary
        w.write_b(false); // no AcDs binary data
        w
    }

    /// The common object data an R2000-R2007 non-entity object leads
    /// with: empty EED chain, `BL` reactor count, and — from R2004 —
    /// the xdictionary-missing flag. No R2013+ AcDs binary-data bit.
    pub(crate) fn r2004_object_prefix(num_reactors: i32) -> BitWriter {
        let mut w = BitWriter::new();
        w.write_bs_u(0); // EED terminator
        w.write_bl(num_reactors);
        w.write_b(true); // no xdictionary
        w
    }

    /// Write one pre-R2007 inline `TV`: `BS` length then that many bytes.
    pub(crate) fn write_inline_tv(w: &mut BitWriter, s: &str) {
        w.write_bs_u(s.len() as u16);
        for b in s.as_bytes() {
            w.write_rc(*b);
        }
    }

    /// Build an R2010+ object payload whose *strings present* trailer
    /// bit is clear — the shape every `*_CONTROL`, XRECORD and
    /// ACDB_PLACEHOLDER record actually has. The data fields then end
    /// one bit before the handle stream.
    pub(crate) fn build_payload_without_strings(body: &[bool]) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.write_rc(0x00); // MC placeholder, patched below
        for bit in body {
            w.write_b(*bit);
        }
        w.write_b(false); // strings present = no
        let pad = (8 - w.position_bits() % 8) % 8;
        for _ in 0..pad {
            w.write_b(false);
        }
        let mut bytes = w.into_bytes();
        bytes[0] = (8 + pad) as u8;
        bytes
    }

    #[test]
    fn finish_rejects_a_short_read() {
        let mut body = r2018_object_prefix(0);
        body.write_bl(1);
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["x"]);
        let mut split = open(&payload, 8, None, Version::R2018).unwrap();
        // The body wrote a BL the decoder below never reads.
        assert!(split.finish("TEST").is_err());
        split.data.read_bl().unwrap();
        split.finish("TEST").unwrap();
        assert_eq!(
            read_tv(&mut split.data, &mut split.strings, Version::R2018).unwrap(),
            "x"
        );
    }

    #[test]
    fn inline_layout_reads_tv_from_the_data_cursor() {
        let mut w = BitWriter::new();
        w.write_bs_u(0); // EED terminator
        w.write_bl(2); // num_reactors
        w.write_b(true); // no xdictionary (R2004+)
        w.write_bs_u(4); // TV length
        for b in b"ACAD" {
            w.write_rc(*b);
        }
        let end = w.position_bits();
        let bytes = w.into_bytes();
        let mut split = open(&bytes, 0, Some(end), Version::R2004).unwrap();
        assert_eq!(split.num_reactors, 2);
        assert_eq!(
            read_tv(&mut split.data, &mut split.strings, Version::R2004).unwrap(),
            "ACAD"
        );
        split.finish("TEST").unwrap();
    }
}
