//! XRECORD object (spec §19.6.5 / §19.5.67) — opaque key/value
//! storage under a parent DICTIONARY.
//!
//! XRECORD is how the DWG format extends itself without changing the
//! spec: each XRECORD is a list of (group-code, value) pairs like
//! DXF group codes (10 = 3D point, 40 = double, 1 = string, etc.).
//! The consuming application knows what group codes mean for its
//! specific dictionary entry.
//!
//! # Stream shape
//!
//! ```text
//! BL     num_databytes          -- size of the raw body (≤ 16 MiB)
//! // body: concatenated raw bytes of DXF-style (code, value) pairs
//! // encoded per group-code type (see spec §19.6.5 Table 20).
//! BS     cloning_flags          -- R2000+ only
//! ```
//!
//! # Opaque-body handling
//!
//! This initial decoder treats the body as opaque bytes. Full typed
//! (code, value) tuple decomposition mirrors the XData type map
//! (spec §3.5) and will land in a follow-up — the shape of each
//! pair depends on the specific dictionary entry that owns the
//! XRECORD, so many consumers will prefer the raw bytes anyway.
//!
//! Byte-count is capped at [`MAX_XRECORD_BYTES`] to bound allocation
//! from adversarial inputs.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::version::Version;

/// Sanity cap on XRECORD body size (16 MiB). Real dictionary payloads
/// are typically a few hundred bytes.
pub const MAX_XRECORD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XRecord {
    /// Opaque raw bytes of the DXF-style (code, value) pair sequence.
    pub data: Vec<u8>,
    /// R2000+ cloning flags (0 if not present — i.e. R14 file).
    pub cloning_flags: i16,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<XRecord> {
    let data_bytes = c.read_bl()? as usize;
    if data_bytes > MAX_XRECORD_BYTES {
        return Err(Error::SectionMap(format!(
            "XRECORD data bytes {data_bytes} exceeds {MAX_XRECORD_BYTES} sanity cap"
        )));
    }
    let mut data = Vec::with_capacity(data_bytes);
    for _ in 0..data_bytes {
        data.push(c.read_rc()?);
    }
    let cloning_flags = if version == Version::R14 {
        0
    } else {
        c.read_bs()?
    };
    Ok(XRecord {
        data,
        cloning_flags,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_empty_xrecord() {
        let mut w = BitWriter::new();
        w.write_bl(0);
        w.write_bs(1);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let x = decode(&mut c, Version::R2000).unwrap();
        assert!(x.data.is_empty());
        assert_eq!(x.cloning_flags, 1);
    }

    #[test]
    fn roundtrip_populated_xrecord() {
        let mut w = BitWriter::new();
        let payload: [u8; 5] = [1, 2, 3, 4, 5];
        w.write_bl(payload.len() as i32);
        for b in &payload {
            w.write_rc(*b);
        }
        w.write_bs(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let x = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(x.data, &payload);
        assert_eq!(x.cloning_flags, 0);
    }

    #[test]
    fn r14_skips_cloning_flags() {
        let mut w = BitWriter::new();
        w.write_bl(3);
        w.write_rc(0xAA);
        w.write_rc(0xBB);
        w.write_rc(0xCC);
        // no cloning_flags for R14
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let x = decode(&mut c, Version::R14).unwrap();
        assert_eq!(x.data, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(x.cloning_flags, 0);
    }

    #[test]
    fn rejects_oversized_body() {
        let mut w = BitWriter::new();
        // Inflate declared size past the cap.
        w.write_bl((MAX_XRECORD_BYTES + 1) as i32);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("XRECORD")));
    }
}
