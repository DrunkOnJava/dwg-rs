//! DICTIONARYVAR object (`AcDbDictionaryVar`) — a single named string
//! setting stored under a dictionary.
//!
//! `ACAD_DICTIONARYVAR` entries hold the drawing's per-document
//! variables that are not header variables — `CANNOSCALE`,
//! `CTABLESTYLE`, `DIMASSOC`, `HPCOLOR` and friends. Each record is one
//! value; its name is the key its parent dictionary filed it under.
//!
//! `DICTIONARYVAR` is a custom class, so its object type code is
//! assigned per file through `AcDb:Classes` and the dispatcher resolves
//! it by DXF class name.
//!
//! # Stream shape — measured
//!
//! ```text
//! RC    schema
//! TV    value
//! ```
//!
//! `arc_2013.dwg` handles 138-142 each leave exactly 8 bits between the
//! end of the common object data and the start of the string stream —
//! one `RC`, reading 0 — while the string stream holds the single value
//! (`"STANDARD"`, `"Metric50"`, `"1:1"`, …). `arc_2004.dwg`'s
//! DICTIONARYVAR records spend the same `RC` and then the value inline
//! ahead of their `RL` object-data-size boundary.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

/// A decoded DICTIONARYVAR record.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DictionaryVar {
    /// Leading `RC`; 0 in every record observed. DXF calls the
    /// equivalent group code the "object schema number".
    pub schema: u8,
    pub value: String,
}

/// Read the DICTIONARYVAR field list from whichever streams hold it.
fn read_body(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<DictionaryVar> {
    let schema = c.read_rc()?;
    let value = modern::read_tv(c, strings, version)?;
    Ok(DictionaryVar { schema, value })
}

/// Decodes the `DictionaryVar` payload that follows the common object header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<DictionaryVar> {
    read_body(c, &mut None, version)
}

/// Decode a DICTIONARYVAR straight from its raw object payload, taking
/// the value from the R2007+ string stream when the file has one and
/// checking the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<DictionaryVar> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings, version)?;
    split.finish("DICTIONARYVAR")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_inline_value() {
        let mut w = BitWriter::new();
        w.write_rc(0);
        w.write_bs_u(8);
        for b in b"Metric50" {
            w.write_rc(*b);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let v = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(v.schema, 0);
        assert_eq!(v.value, "Metric50");
    }

    #[test]
    fn r2018_split_stream_var_reads_its_value_from_the_string_stream() {
        let mut body = modern::tests::r2018_object_prefix(1);
        body.write_rc(0);
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["STANDARD"]);
        let v = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(v.schema, 0);
        assert_eq!(v.value, "STANDARD");
    }

    #[test]
    fn r2018_split_stream_var_rejects_a_misaligned_body() {
        let mut body = modern::tests::r2018_object_prefix(1);
        body.write_rc(0);
        body.write_rc(0);
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["STANDARD"]);
        assert!(decode_object(&payload, 8, None, Version::R2018).is_err());
    }
}
