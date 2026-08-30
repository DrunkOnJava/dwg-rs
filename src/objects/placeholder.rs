//! ACDB_PLACEHOLDER object (type code 0x50) — a body-less object.
//!
//! A placeholder is what a DICTIONARY points at when it needs a valid,
//! ownable object but has nothing to store — the `ACAD_PLOTSTYLENAME`
//! dictionary's entries are the canonical example. It carries the
//! common object data and its handle references and nothing else.
//!
//! # Measured: the body really is empty
//!
//! This is the cleanest confirmation in the crate that the R2007+
//! common object data still carries its `BL` reactor count. In
//! `arc_2013.dwg` the ACDB_PLACEHOLDER record (handle 15) has exactly
//! 15 bits between the end of its object header and the start of its
//! handle stream, and they are consumed by
//!
//! ```text
//! BS  0    EED terminator          2 bits
//! BL  1    num_reactors           10 bits
//! B   1    no xdictionary          1 bit
//! B   0    no AcDs binary data     1 bit
//! B   0    strings present         1 bit  (the §19.1 trailer)
//! ```
//!
//! — 15 bits, nothing left for a field. `arc_2004.dwg` agrees: its
//! placeholder's `RL` object-data-size lands exactly on the end of
//! `B no xdictionary`, 13 bits in.

use crate::error::Result;
use crate::objects::modern;
use crate::version::Version;

/// The decoded ACDB_PLACEHOLDER — a marker with no fields of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Placeholder {
    /// `BL num_reactors` from the common object data (§19.4.2), the
    /// only number a placeholder record actually carries.
    pub num_reactors: u32,
}

/// Decode an ACDB_PLACEHOLDER from its raw object payload, checking
/// that its common object data lands exactly on the data-stream
/// boundary — the whole content of the record.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<Placeholder> {
    let split = modern::open(payload, body_start, inline_data_end, version)?;
    split.finish("ACDB_PLACEHOLDER")?;
    Ok(Placeholder {
        num_reactors: split.num_reactors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn r2018_placeholder_is_exactly_the_common_object_data() {
        let body = modern::tests::r2018_object_prefix(1);
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = modern::tests::build_payload_without_strings(&bits);
        let p = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(p.num_reactors, 1);
    }

    #[test]
    fn r2018_placeholder_rejects_a_record_with_a_body() {
        let mut body = modern::tests::r2018_object_prefix(1);
        body.write_bl(7); // a field a placeholder must not have
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = modern::tests::build_payload_without_strings(&bits);
        assert!(decode_object(&payload, 8, None, Version::R2018).is_err());
    }

    #[test]
    fn r2004_placeholder_closes_on_the_inline_object_size() {
        let mut w = BitWriter::new();
        w.write_bs_u(0);
        w.write_bl(1);
        w.write_b(true);
        let end = w.position_bits();
        let bytes = w.into_bytes();
        let p = decode_object(&bytes, 0, Some(end), Version::R2004).unwrap();
        assert_eq!(p.num_reactors, 1);
    }
}
