//! Proxy object pass-through (spec §19.4.91, non-entity variant).
//!
//! Structurally identical to [`super::proxy_entity::ProxyEntity`] but
//! attached via a DICTIONARY rather than a block record. AutoCAD uses
//! this class when an object-type ARX plug-in is unavailable — the
//! serialised body is preserved verbatim so a round-trip save doesn't
//! drop it.
//!
//! # Stream shape
//!
//! ```text
//! BL             proxy_class_id        -- matches a class_number in the class map
//! BL             proxy_data_length     -- bytes in raw_proxy_data (≤ 16 MiB)
//! RC × N         raw_proxy_data
//! BL             num_object_handles    -- ≤ 10 000
//! H × M          handles
//! ```
//!
//! The two proxy decoders share a layout but keep separate modules
//! and types to make the entity-vs-object distinction visible at the
//! API surface. Callers dispatching on `ObjectType` get the right
//! return type without a runtime tag.

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};

/// Sanity cap on embedded proxy payload size (16 MiB).
pub const MAX_PROXY_DATA_BYTES: usize = 16 * 1024 * 1024;

/// Sanity cap on handle references embedded in a proxy.
pub const MAX_PROXY_HANDLES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyObject {
    pub proxy_class_id: u32,
    pub raw_proxy_data: Vec<u8>,
    pub handles: Vec<Handle>,
}

pub fn decode(c: &mut BitCursor<'_>, _version: crate::version::Version) -> Result<ProxyObject> {
    let proxy_class_id = c.read_bl()? as u32;
    let data_length = c.read_bl()? as usize;
    if data_length > MAX_PROXY_DATA_BYTES {
        return Err(Error::SectionMap(format!(
            "ProxyObject data length {data_length} exceeds {MAX_PROXY_DATA_BYTES} sanity cap"
        )));
    }
    let mut raw_proxy_data = Vec::with_capacity(data_length);
    for _ in 0..data_length {
        raw_proxy_data.push(c.read_rc()?);
    }
    let num_handles = c.read_bl()? as usize;
    if num_handles > MAX_PROXY_HANDLES {
        return Err(Error::SectionMap(format!(
            "ProxyObject claims {num_handles} handles (>{MAX_PROXY_HANDLES} sanity cap)"
        )));
    }
    let mut handles = Vec::with_capacity(num_handles);
    for _ in 0..num_handles {
        handles.push(c.read_handle()?);
    }
    Ok(ProxyObject {
        proxy_class_id,
        raw_proxy_data,
        handles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::version::Version;

    #[test]
    fn roundtrip_empty_proxy() {
        let mut w = BitWriter::new();
        w.write_bl(600);
        w.write_bl(0);
        w.write_bl(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(p.proxy_class_id, 600);
        assert!(p.raw_proxy_data.is_empty());
        assert!(p.handles.is_empty());
    }

    #[test]
    fn roundtrip_populated_proxy_object() {
        let mut w = BitWriter::new();
        w.write_bl(601);
        let payload: [u8; 3] = [0xC0, 0xDE, 0x42];
        w.write_bl(payload.len() as i32);
        for b in &payload {
            w.write_rc(*b);
        }
        w.write_bl(1);
        w.write_handle(3, 0x55);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(p.proxy_class_id, 601);
        assert_eq!(p.raw_proxy_data, &payload);
        assert_eq!(p.handles.len(), 1);
        assert_eq!(p.handles[0].value, 0x55);
    }

    #[test]
    fn rejects_oversized_data() {
        let mut w = BitWriter::new();
        w.write_bl(600);
        w.write_bl((MAX_PROXY_DATA_BYTES + 1) as i32);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("ProxyObject")));
    }

    #[test]
    fn rejects_excessive_handle_count() {
        let mut w = BitWriter::new();
        w.write_bl(600);
        w.write_bl(0);
        w.write_bl((MAX_PROXY_HANDLES + 1) as i32);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("handles")));
    }
}
