//! R2007+ per-object string stream (ODA Open Design Specification
//! v5.4.1 §19.1 "Object data" / R2007 split-stream layout).
//!
//! # Why this module exists
//!
//! Up to R2004 an object's `TV` (variable text) fields are stored
//! inline, at the position the object's field table lists them. From
//! R2007 (`AC1021`) onward the same field tables still list the `TV`
//! slots inline, but the *characters* are moved out into a separate
//! **string stream** that sits between the object's data stream and
//! its handle stream:
//!
//! ```text
//! +--------------+----------------+----------------+
//! | data stream  | string stream  | handle stream  |
//! +--------------+----------------+----------------+
//!                ^                ^
//!                |                `- string-stream trailer ends here
//!                `- data-stream fields end exactly here
//! ```
//!
//! A decoder that reads `TV` from the data cursor therefore reads
//! random bits — the symptom is "table entry name is not valid UTF-16"
//! or a bit cursor that runs off the end of the object.
//!
//! # Locating the string stream (§19.1)
//!
//! The spec describes a trailer at the very end of the data area:
//!
//! ```text
//! ... string data ... [ hi RS ] [ lo RS ] [ B strings-present ]
//!                                                             ^ end
//! ```
//!
//! - the last bit before the handle stream is a *strings present* flag;
//! - the 16 bits before it are the string-stream size in bits;
//! - if bit 15 of that size is set, a second 16-bit word immediately
//!   before it supplies the high bits: `size = (lo & 0x7FFF) | (hi << 15)`;
//! - the string data occupies exactly `size` bits ending at the trailer.
//!
//! # Where the trailer ends — measured, not assumed
//!
//! For R2010+ the object payload leads with an `MC` field holding the
//! handle-stream size in bits. The naive `payload_bits - handle_bits`
//! lands 8 or 16 bits *short* of the real trailer end. Measured over
//! 59 objects across 11 object types in `sample_AC1032.dwg` (R2018),
//! `line_2013.dwg` (R2013) and `arc_2010.dwg` (R2010) — see
//! `examples/probe_string_stream.rs` — the correction is exactly the
//! width of that `MC` field:
//!
//! ```text
//! trailer_end = payload_bits - handle_stream_bits + mc_field_bits
//! ```
//!
//! Every one of the 59 objects matched with zero deviation (the probe
//! prints `delta_vs_predicted`). Two readings are consistent with the
//! evidence: either the record's `MS` byte count excludes the `MC`
//! field, or the recorded handle-stream size counts the `MC` field
//! itself. This crate does not need to pick one — the offset above is
//! what the bytes say, and it is applied as a measured constant.
//!
//! R2007 (`AC1021`) proper has no such `MC` field and its object stream
//! is not walkable in this crate yet (see `STATUS.md` #104); [`locate`]
//! returns `None` there rather than guessing.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::version::Version;

/// Bit range occupied by one object's string stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringStream {
    /// First bit of the string data — also the bit at which the
    /// object's data-stream fields must end.
    pub start_bit: usize,
    /// One past the last bit of the string data (the trailer starts here).
    pub end_bit: usize,
}

impl StringStream {
    /// Length of the string data in bits.
    pub fn len_bits(&self) -> usize {
        self.end_bit.saturating_sub(self.start_bit)
    }
}

/// Locate the R2007+ string stream inside an object payload, per §19.1.
///
/// Returns `None` when the version predates the split layout, when the
/// object carries no strings, or when the trailer does not describe a
/// range inside the payload.
pub fn locate(payload: &[u8], version: Version) -> Option<StringStream> {
    let trailer_end = data_section_end(payload, version)?;
    if trailer_end < 17 {
        return None;
    }
    if !read_bit_at(payload, trailer_end - 1)? {
        return None;
    }
    let lo = read_rs_at(payload, trailer_end - 17)?;
    let (size, end_bit) = if lo & 0x8000 != 0 {
        let hi_at = trailer_end.checked_sub(33)?;
        let hi = read_rs_at(payload, hi_at)?;
        (((lo & 0x7FFF) as usize) | ((hi as usize) << 15), hi_at)
    } else {
        (lo as usize, trailer_end - 17)
    };
    let start_bit = end_bit.checked_sub(size)?;
    Some(StringStream { start_bit, end_bit })
}

/// Bit offset at which an object's **data fields** must end.
///
/// With a string stream present that is its first bit. With none, the
/// `strings present` trailer bit is still written — it is the last bit
/// before the handle stream, and it is not one of the record's own
/// fields — so the fields end one bit earlier than
/// [`data_section_end`].
///
/// # Measured
///
/// Every LWPOLYLINE (20 records), 3DFACE (1), INSERT (4) and SPLINE (2)
/// of `sample_AC1032.dwg` carries no string stream, and every one of
/// them ends its field list exactly one bit short of
/// [`data_section_end`]. Treating that bit as a trailing entity field
/// would put a fabricated `B` at the end of four different field lists;
/// it is the trailer flag, once, for all of them.
pub fn data_field_end(payload: &[u8], version: Version) -> Option<usize> {
    match locate(payload, version) {
        Some(stream) => Some(stream.start_bit),
        None => data_section_end(payload, version)?.checked_sub(1),
    }
}

/// Bit offset at which the object's data + string area ends (and the
/// handle stream begins), per the measured rule in the module docs.
pub fn data_section_end(payload: &[u8], version: Version) -> Option<usize> {
    if !version.is_r2010_plus() {
        return None;
    }
    let (handle_bits, mc_bits) = read_handle_stream_size(payload)?;
    let total = payload.len().checked_mul(8)?;
    let end = total.checked_sub(handle_bits)?.checked_add(mc_bits)?;
    if end > total { None } else { Some(end) }
}

/// Read the leading `MC` handle-stream size, returning `(value, field_bits)`.
fn read_handle_stream_size(payload: &[u8]) -> Option<(usize, usize)> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for i in 0..10usize {
        let b = *payload.get(i)? as u64;
        value |= (b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Some((value as usize, (i + 1) * 8));
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

fn read_bit_at(bytes: &[u8], bit: usize) -> Option<bool> {
    let byte = *bytes.get(bit / 8)?;
    Some((byte >> (7 - (bit % 8))) & 1 != 0)
}

fn read_rs_at(bytes: &[u8], bit: usize) -> Option<u16> {
    let mut c = BitCursor::new(bytes);
    seek(&mut c, bit).ok()?;
    c.read_rs().ok().map(|v| v as u16)
}

/// Advance `c` to absolute bit `bit` (the cursor has no random access).
pub(crate) fn seek(c: &mut BitCursor<'_>, bit: usize) -> Result<()> {
    while c.position_bits() < bit {
        let _ = c.read_b()?;
    }
    Ok(())
}

/// Sequential reader over one object's string stream (§19.1).
///
/// Each `read_tv` consumes the next `TU` (length-prefixed UTF-16LE)
/// string. Once the stream is exhausted the reader yields empty
/// strings rather than erroring, because a field table may list more
/// `TV` slots than a given object actually wrote.
#[derive(Debug)]
pub struct StringReader<'a> {
    cursor: BitCursor<'a>,
    end_bit: usize,
}

impl<'a> StringReader<'a> {
    /// A reader over an object that carries no string stream — every
    /// `read_tv` yields `""` and consumes nothing.
    ///
    /// R2007+ records whose `strings present` trailer bit is clear
    /// still have the `TV` slots in their field table; the slots are
    /// simply empty and consume no data-stream bits. A decoder must
    /// therefore keep reading through the string stream rather than
    /// falling back to the inline layout, which would shift every
    /// field after the slot.
    pub fn empty(payload: &'a [u8]) -> Self {
        Self {
            cursor: BitCursor::new(payload),
            end_bit: 0,
        }
    }

    /// Open a reader positioned at the start of `stream` inside `payload`.
    pub fn new(payload: &'a [u8], stream: StringStream) -> Result<Self> {
        let mut cursor = BitCursor::new(payload);
        seek(&mut cursor, stream.start_bit)?;
        Ok(Self {
            cursor,
            end_bit: stream.end_bit,
        })
    }

    /// True once every bit of the string stream has been consumed.
    pub fn is_exhausted(&self) -> bool {
        self.cursor.position_bits() >= self.end_bit
    }

    /// Current absolute bit position inside the payload.
    pub fn position_bits(&self) -> usize {
        self.cursor.position_bits()
    }

    /// Bits of string data not yet consumed (0 once exhausted).
    pub fn remaining_bits(&self) -> usize {
        self.end_bit.saturating_sub(self.cursor.position_bits())
    }

    /// Read the next `TU` string, or `""` when the stream is exhausted.
    pub fn read_tv(&mut self) -> Result<String> {
        if self.is_exhausted() {
            return Ok(String::new());
        }
        let len = self.cursor.read_bs_u()? as usize;
        if len == 0 {
            return Ok(String::new());
        }
        if self.cursor.position_bits() + len * 16 > self.end_bit {
            return Err(Error::SectionMap(format!(
                "string stream TV of {len} units overruns the stream"
            )));
        }
        let mut units = Vec::with_capacity(len);
        for _ in 0..len {
            let lo = self.cursor.read_rc()? as u16;
            let hi = self.cursor.read_rc()? as u16;
            units.push((hi << 8) | lo);
        }
        if units.last() == Some(&0) {
            units.pop();
        }
        String::from_utf16(&units)
            .map_err(|_| Error::SectionMap("string stream TV is not valid UTF-16".into()))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Flatten a [`BitWriter`]'s contents into a bit vector.
    pub(crate) fn bits_of(w: &BitWriter) -> Vec<bool> {
        let bits = w.position_bits();
        let bytes = w.clone().into_bytes();
        (0..bits)
            .map(|i| (bytes[i / 8] >> (7 - (i % 8))) & 1 != 0)
            .collect()
    }

    /// Build a synthetic R2010+ object payload: `MC` handle-stream size,
    /// `body` bits, the string stream, its trailer, then `handle_bits`
    /// of filler standing in for the handle stream.
    pub(crate) fn build_payload(body: &[bool], strings: &[&str]) -> Vec<u8> {
        let mut sw = BitWriter::new();
        for s in strings {
            sw.write_bs_u(s.encode_utf16().count() as u16);
            for unit in s.encode_utf16() {
                sw.write_rc((unit & 0xFF) as u8);
                sw.write_rc((unit >> 8) as u8);
            }
        }
        let string_bits = sw.position_bits();
        let string_bytes = sw.into_bytes();

        let mut w = BitWriter::new();
        w.write_rc(0x00); // MC placeholder, patched below
        for bit in body {
            w.write_b(*bit);
        }
        for i in 0..string_bits {
            let byte = string_bytes[i / 8];
            w.write_b((byte >> (7 - (i % 8))) & 1 != 0);
        }
        w.write_rs(string_bits as i16);
        w.write_b(true);
        // Pad to the byte boundary; the pad stands in for the handle
        // stream. Per the measured rule the recorded MC value must be
        // `mc_field_bits + pad` so that
        // `payload_bits - handle_bits + mc_bits` lands on the trailer.
        let pad = (8 - w.position_bits() % 8) % 8;
        for _ in 0..pad {
            w.write_b(false);
        }
        let mut bytes = w.into_bytes();
        bytes[0] = (8 + pad) as u8;
        bytes
    }

    /// Build an R2010+ payload whose `strings present` trailer bit is
    /// **clear**: `MC` handle-stream size, `body` bits, the flag, then
    /// filler standing in for the handle stream.
    pub(crate) fn build_payload_without_strings(body: &[bool]) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.write_rc(0x00); // MC placeholder, patched below
        for bit in body {
            w.write_b(*bit);
        }
        w.write_b(false); // strings present = false
        let pad = (8 - w.position_bits() % 8) % 8;
        for _ in 0..pad {
            w.write_b(false);
        }
        let mut bytes = w.into_bytes();
        bytes[0] = (8 + pad) as u8;
        bytes
    }

    /// A record with no string stream still writes the `strings
    /// present` trailer bit, and that bit is not one of the record's
    /// own fields — so its data fields end one bit before
    /// [`data_section_end`].
    #[test]
    fn data_field_end_excludes_the_trailer_flag_when_no_strings() {
        let body = vec![true, false, true, true, false, true, false];
        let payload = build_payload_without_strings(&body);
        assert!(locate(&payload, Version::R2018).is_none());
        let section_end = data_section_end(&payload, Version::R2018).unwrap();
        let field_end = data_field_end(&payload, Version::R2018).unwrap();
        assert_eq!(field_end, section_end - 1);
        assert_eq!(field_end, 8 + body.len());
    }

    /// With a string stream present the two agree on nothing — the
    /// fields end where the string data begins, well before the
    /// trailer.
    #[test]
    fn data_field_end_is_the_string_start_when_strings_are_present() {
        let body = vec![true, false, true, true];
        let payload = build_payload(&body, &["Standard"]);
        let stream = locate(&payload, Version::R2018).expect("string stream");
        assert_eq!(
            data_field_end(&payload, Version::R2018),
            Some(stream.start_bit)
        );
    }

    #[test]
    fn locates_and_reads_two_strings() {
        let body = vec![true, false, true, true];
        let payload = build_payload(&body, &["Standard", "arial.ttf"]);
        let stream = locate(&payload, Version::R2018).expect("string stream");
        assert_eq!(stream.start_bit, 8 + body.len());
        let mut r = StringReader::new(&payload, stream).unwrap();
        assert_eq!(r.read_tv().unwrap(), "Standard");
        assert_eq!(r.read_tv().unwrap(), "arial.ttf");
        assert!(r.is_exhausted());
        assert_eq!(r.read_tv().unwrap(), "");
    }

    #[test]
    fn no_stream_before_r2010() {
        let payload = build_payload(&[], &["x"]);
        assert!(locate(&payload, Version::R2004).is_none());
        assert!(locate(&payload, Version::R2007).is_none());
    }

    #[test]
    fn absent_flag_yields_none() {
        // Flip the trailing "strings present" bit off.
        let mut payload = build_payload(&[], &["x"]);
        let end = data_section_end(&payload, Version::R2018).unwrap();
        let idx = (end - 1) / 8;
        payload[idx] &= !(1 << (7 - ((end - 1) % 8)));
        assert!(locate(&payload, Version::R2018).is_none());
    }
}
