//! OLE2FRAME entity (§19.4.88) — embedded OLE 2 object.
//!
//! OLE2FRAME wraps an embedded OLE 2 (Compound Document / Structured
//! Storage) object such as an Excel worksheet, Word document, or
//! bitmap. The DWG only stores the raw OLE container bytes plus a few
//! positioning fields — actually interpreting the OLE blob requires
//! a separate COM-style reader and is *not* attempted here. This
//! decoder surfaces the blob as an opaque byte array so downstream
//! callers can hand it to an OLE decoder of their choice.
//!
//! Fixed object type code `0x4A` per ODA spec §5 Table 4.
//!
//! # Stream shape
//!
//! ```text
//! BS    ole_version        -- OLE2 container format rev (typically 2)
//! BL    data_length        -- size of the OLE blob in bytes (capped at 16 MiB)
//! RC[data_length]  data    -- opaque Compound Document bytes
//! BS    oleobject_type     -- 1 = link, 2 = embedded, 3 = static
//! RC    mode
//! ```
//!
//! The `data` bytes are an OLE 2 Compound Document stream. Parsing
//! that stream (FAT allocation tables, DirEntry nodes, IStorage /
//! IStream recovery) is the job of a separate decoder — see the
//! `OLEDECODE` roadmap entry. Surfacing the raw bytes here keeps
//! this decoder finite and deterministic.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};

/// Decoded OLE2FRAME payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Ole2Frame {
    pub ole_version: i16,
    /// Raw OLE 2 Compound Document bytes. Opaque at this layer.
    pub data: Vec<u8>,
    pub oleobject_type: i16,
    pub mode: u8,
}

/// 16 MiB cap on the embedded OLE blob. Real Excel/Word embeddings
/// in real drawings are a few hundred KiB to low MiB; 16 MiB leaves
/// plenty of headroom for richly-formatted spreadsheets without
/// letting an adversarial file claim a gigabyte of payload.
const OLE2FRAME_MAX_DATA_BYTES: usize = 16 * 1024 * 1024;

/// Decode an OLE2FRAME payload. The cursor must already be positioned
/// past the common entity preamble.
pub fn decode(c: &mut BitCursor<'_>) -> Result<Ole2Frame> {
    let ole_version = c.read_bs()?;
    let data_length_signed = c.read_bl()?;
    if data_length_signed < 0 {
        return Err(Error::SectionMap(format!(
            "OLE2FRAME negative data_length {data_length_signed}"
        )));
    }
    let data_length = data_length_signed as usize;
    if data_length > OLE2FRAME_MAX_DATA_BYTES {
        return Err(Error::SectionMap(format!(
            "OLE2FRAME data_length {data_length} exceeds cap {OLE2FRAME_MAX_DATA_BYTES}"
        )));
    }
    if data_length * 8 > c.remaining_bits() {
        return Err(Error::SectionMap(format!(
            "OLE2FRAME data_length {data_length} exceeds remaining_bits {}",
            c.remaining_bits()
        )));
    }
    let mut data = Vec::with_capacity(data_length);
    for _ in 0..data_length {
        data.push(c.read_rc()?);
    }
    let oleobject_type = c.read_bs()?;
    let mode = c.read_rc()?;
    Ok(Ole2Frame {
        ole_version,
        data,
        oleobject_type,
        mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_ole2frame_small_blob() {
        let mut w = BitWriter::new();
        w.write_bs(2); // ole_version
        // Four-byte synthetic "blob" — a real OLE 2 header would start
        // with the Compound Document magic (D0 CF 11 E0 A1 B1 1A E1),
        // but this test only cares that decode preserves the bytes.
        let blob = [0xD0u8, 0xCF, 0x11, 0xE0];
        w.write_bl(blob.len() as i32);
        for b in blob {
            w.write_rc(b);
        }
        w.write_bs(2); // embedded
        w.write_rc(0); // mode
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let o = decode(&mut c).unwrap();
        assert_eq!(o.ole_version, 2);
        assert_eq!(o.data, blob);
        assert_eq!(o.oleobject_type, 2);
        assert_eq!(o.mode, 0);
    }

    #[test]
    fn roundtrip_ole2frame_empty_blob() {
        let mut w = BitWriter::new();
        w.write_bs(2);
        w.write_bl(0);
        w.write_bs(1);
        w.write_rc(1);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let o = decode(&mut c).unwrap();
        assert!(o.data.is_empty());
        assert_eq!(o.oleobject_type, 1);
        assert_eq!(o.mode, 1);
    }

    #[test]
    fn rejects_negative_data_length() {
        let mut w = BitWriter::new();
        w.write_bs(2);
        w.write_bl(-1);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("negative data_length")),
            "err={err:?}"
        );
    }

    #[test]
    fn rejects_excessive_data_length() {
        let mut w = BitWriter::new();
        w.write_bs(2);
        // Claim 32 MiB — exceeds OLE2FRAME_MAX_DATA_BYTES (16 MiB).
        w.write_bl(32 * 1024 * 1024);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("exceeds cap")),
            "err={err:?}"
        );
    }
}
