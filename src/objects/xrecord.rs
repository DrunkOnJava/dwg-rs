//! XRECORD object (§19.5.67) — opaque key/value storage under a
//! parent DICTIONARY.
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
//! BL     data_bytes           -- size of the raw body
//! // body: concatenated raw bytes of DXF-style (code, value) pairs
//! // encoded per group-code type (see spec §19.5.67 Table 20).
//! BS     cloning_flag
//! ```
//!
//! For this initial release, we capture the raw data bytes and the
//! cloning flag — not the DXF tuple decomposition, which is
//! application-specific and best handled by the caller who owns the
//! dictionary entry referencing this XRECORD.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XRecord {
    pub data: Vec<u8>,
    pub cloning_flag: i16,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<XRecord> {
    let data_bytes = c.read_bl()? as usize;
    if data_bytes > 16 * 1024 * 1024 {
        return Err(Error::SectionMap(format!(
            "XRECORD data bytes {data_bytes} exceeds 16MB sanity cap"
        )));
    }
    let mut data = Vec::with_capacity(data_bytes);
    for _ in 0..data_bytes {
        data.push(c.read_rc()?);
    }
    let cloning_flag = c.read_bs()?;
    Ok(XRecord { data, cloning_flag })
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
        let x = decode(&mut c).unwrap();
        assert!(x.data.is_empty());
        assert_eq!(x.cloning_flag, 1);
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
        let x = decode(&mut c).unwrap();
        assert_eq!(x.data, &payload);
        assert_eq!(x.cloning_flag, 0);
    }
}
