//! DICTIONARY object (§19.5.19) — string-keyed handle map.
//!
//! DICTIONARY is the backbone of the DWG extension mechanism: every
//! custom object type (groups, layouts, materials, table styles,
//! multileader styles, visual styles) is attached to the drawing
//! via a chain of dictionaries rooted at the "named object
//! dictionary" (NOD). Each dictionary maps a string name to a
//! handle.
//!
//! # Stream shape
//!
//! ```text
//! BL         num_items
//! BS         cloning_flag     -- 1=keep, 2=ignore, 3=replace, 4=xref
//! RC         hard_owner_flag  -- R2000+
//! TV × N     entry names
//! ```
//!
//! # Measured: the names are a block, not interleaved with handles
//!
//! An earlier cut of this decoder read `TV name` and `H value` as a
//! pair per entry. The bytes say otherwise: the value handles live in
//! the object's handle references, past the end of its data stream, so
//! the data stream carries the names only and the *i*-th name pairs
//! with the *i*-th item handle.
//!
//! Evidence, `arc_2004.dwg` (AC1018) handle 16 — the linetype
//! dictionary. Its `RL` object-data-size puts the data stream's end 255
//! bits past the object header. Reading `BL numitems = 3`, `BS cloning
//! = 1`, `RC hard-owner = 0` and then three `TV`s consumes exactly
//! those bits, and the first `TV` reads length 8 followed by the ASCII
//! bytes of `ByBlock` plus its NUL. Reading a handle after each name
//! instead desynchronises on the second entry.
//!
//! On R2007+ the same names live in the object's string stream
//! (§19.1). `sample_AC1032.dwg` has 65 DICTIONARY records; in every one
//! of them `BL numitems` decoded through
//! `objects::modern` equals the number of strings the record's
//! string stream actually holds — 23 for the named-object dictionary,
//! 4 for the layout dictionary, 0 for the empty ones.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

/// Sanity cap on the entry count claimed by a DICTIONARY record.
const MAX_DICTIONARY_ITEMS: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dictionary {
    pub cloning_flag: i16,
    pub hard_owner: bool,
    /// Entry names in stream order.
    ///
    /// The matching value handles are *not* in the object's data
    /// stream — they are handle references that follow it — so the
    /// index of a name here is the index of its handle in that list.
    pub keys: Vec<String>,
}

impl Dictionary {
    /// Position of `name` in [`keys`](Self::keys) — also the position of
    /// its value handle in the object's item-handle list. Case-sensitive.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.keys.iter().position(|k| k == name)
    }

    /// Does the dictionary carry this key? Case-sensitive.
    pub fn contains(&self, name: &str) -> bool {
        self.index_of(name).is_some()
    }

    /// Count entries.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Is the dictionary empty?
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Read the DICTIONARY field list from whichever streams hold it.
fn read_body(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<Dictionary> {
    let num_items = c.read_bl()? as usize;
    if num_items > MAX_DICTIONARY_ITEMS {
        return Err(Error::SectionMap(format!(
            "DICTIONARY claims {num_items} items (>{MAX_DICTIONARY_ITEMS} sanity cap)"
        )));
    }
    // §20.4.44 splits the two bytes after `Numitems` by release: R14
    // writes one `RC` glossed "Unknown R14 byte, has always been 0";
    // R2000+ write `BS Cloning flag` (DXF 281) + `RC Hard Owner flag`
    // (DXF 280). Reading the R2000+ pair on R14 overshoots by the width
    // of the `BS` — 18 bits on the R14 corpus, whose cloning value would
    // have to take the `00` tag — and every one of the 73 DICTIONARY
    // records per R14 file missed its data-stream boundary by exactly
    // that.
    let (cloning_flag, hard_owner_flag) = if matches!(version, Version::R14) {
        let _unknown_r14 = c.read_rc()?;
        (0i16, 0u8)
    } else {
        let cloning = c.read_bs()?;
        (cloning, c.read_rc()?)
    };
    let mut keys = Vec::with_capacity(num_items.min(1024));
    for _ in 0..num_items {
        keys.push(modern::read_tv(c, strings, version)?);
    }
    Ok(Dictionary {
        cloning_flag,
        hard_owner: hard_owner_flag != 0,
        keys,
    })
}

/// Decodes the `Dictionary` payload that follows the common object header.
///
/// This is the pre-R2007 inline layout; on R2007+ use the dispatcher,
/// which routes through the split-stream reader instead.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Dictionary> {
    read_body(c, &mut None, version)
}

/// Decode a DICTIONARY straight from its raw object payload, taking the
/// entry names from the R2007+ string stream when the file has one and
/// checking the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<Dictionary> {
    decode_object_inner(payload, body_start, inline_data_end, version, false)
}

/// Decode an `ACDBDICTIONARYWDFLT` (§20.4.45 — "same as the DICTIONARY
/// object" plus a default-entry handle).
///
/// From R2000 on the extra field is a handle, so it costs no data-stream
/// bits and the record shares DICTIONARY's field list exactly. On R13/R14
/// it costs **one extra `TV`**.
///
/// # Measured
///
/// Handle `0xE` of `line_R14.dwg`, `arc_R14.dwg` and `circle_R14.dwg`
/// (class 500, `ACDBDICTIONARYWDFLT`) declares `numitems = 1` and its
/// `RL` boundary sits at bit 172. Reading one name leaves the cursor at
/// bit 106; reading two lands on 172 exactly. The two names are `""`
/// and `"Normal"` — and `"Normal"` is the one item, because that is the
/// plot-style name this dictionary holds in every other release of the
/// same drawing. So the empty slot precedes the item list.
///
/// Every one of the 73 plain DICTIONARY records in the same files reads
/// exactly `numitems` names and closes on its own boundary, which is
/// what makes the extra slot attributable to this class rather than to
/// R14 dictionaries in general. One record type across three files is
/// thin evidence for what the slot *means*, so it is consumed and not
/// named.
pub(crate) fn decode_object_with_default(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<Dictionary> {
    decode_object_inner(
        payload,
        body_start,
        inline_data_end,
        version,
        matches!(version, Version::R14),
    )
}

fn decode_object_inner(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
    leading_extra_tv: bool,
) -> Result<Dictionary> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let num_items = split.data.read_bl()? as usize;
    if num_items > MAX_DICTIONARY_ITEMS {
        return Err(Error::SectionMap(format!(
            "DICTIONARY claims {num_items} items (>{MAX_DICTIONARY_ITEMS} sanity cap)"
        )));
    }
    let (cloning_flag, hard_owner_flag) = if matches!(version, Version::R14) {
        let _unknown_r14 = split.data.read_rc()?;
        (0i16, 0u8)
    } else {
        let cloning = split.data.read_bs()?;
        (cloning, split.data.read_rc()?)
    };
    if leading_extra_tv {
        let _default_slot = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    }
    let mut keys = Vec::with_capacity(num_items.min(1024));
    for _ in 0..num_items {
        keys.push(modern::read_tv(
            &mut split.data,
            &mut split.strings,
            version,
        )?);
    }
    split.finish("DICTIONARY")?;
    Ok(Dictionary {
        cloning_flag,
        hard_owner: hard_owner_flag != 0,
        keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_empty_dict() {
        let mut w = BitWriter::new();
        w.write_bl(0); // no items
        w.write_bs(1); // keep
        w.write_rc(1); // hard-owner
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode(&mut c, Version::R2000).unwrap();
        assert!(d.is_empty());
        assert_eq!(d.cloning_flag, 1);
        assert!(d.hard_owner);
    }

    #[test]
    fn roundtrip_dict_with_entries() {
        let mut w = BitWriter::new();
        w.write_bl(2);
        w.write_bs(1);
        w.write_rc(1);
        for key in [b"ACAD_LAYOUT".as_slice(), b"ACAD_MATERIAL".as_slice()] {
            w.write_bs_u(key.len() as u16);
            for b in key {
                w.write_rc(*b);
            }
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d.index_of("ACAD_LAYOUT"), Some(0));
        assert_eq!(d.index_of("ACAD_MATERIAL"), Some(1));
        assert!(!d.contains("NONEXISTENT"));
    }

    #[test]
    fn rejects_absurd_item_count() {
        let mut w = BitWriter::new();
        w.write_bl((MAX_DICTIONARY_ITEMS + 1) as i32);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("DICTIONARY")));
    }

    #[test]
    fn r2018_split_stream_dict_reads_keys_from_string_stream() {
        let mut body = modern::tests::r2018_object_prefix(1);
        body.write_bl(2); // num_items
        body.write_bs(1); // cloning flag
        body.write_rc(0); // hard-owner flag
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["*A1", "*A2"]);
        let d = decode_object(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(d.keys, vec!["*A1".to_string(), "*A2".to_string()]);
        assert_eq!(d.cloning_flag, 1);
        assert!(!d.hard_owner);
    }

    /// A body one field short of the data-stream boundary must error,
    /// not return a plausible-looking dictionary.
    #[test]
    fn r2018_split_stream_dict_rejects_a_misaligned_body() {
        let mut body = modern::tests::r2018_object_prefix(1);
        body.write_bl(1);
        body.write_bs(1);
        body.write_rc(0);
        body.write_rc(0); // one field too many on the wire
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["only"]);
        let err = decode_object(&payload, 8, None, Version::R2018).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("DICTIONARY")));
    }
}
