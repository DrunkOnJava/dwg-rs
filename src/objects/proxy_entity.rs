//! Proxy entity pass-through (spec §19.4.91).
//!
//! A proxy entity is a graphical object produced by an AutoCAD ARX
//! application that the reader does not have installed. AutoCAD
//! stores the original class id plus the raw serialized body so the
//! data survives a save / reload cycle without loss even on a
//! machine that cannot construct the class.
//!
//! # Stream shape
//!
//! ```text
//! BL             proxy_class_id        -- matches a class_number in the class map
//! BL             proxy_data_length     -- bytes in raw_proxy_data (≤ 16 MiB)
//! RC × N         raw_proxy_data
//! BL             num_object_handles    -- ≤ 10 000
//! H × M          handles               -- cross-references embedded in the proxy
//! ```
//!
//! # Opaque pass-through
//!
//! This decoder preserves the raw serialized bytes verbatim. A full
//! typed parse would require knowing the original class's schema,
//! which lives in the installing application and is generally not
//! shipped with the file.

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};

/// Sanity cap on embedded proxy payload size (16 MiB).
pub const MAX_PROXY_DATA_BYTES: usize = 16 * 1024 * 1024;

/// Sanity cap on handle references embedded in a proxy.
pub const MAX_PROXY_HANDLES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEntity {
    pub proxy_class_id: u32,
    pub raw_proxy_data: Vec<u8>,
    pub handles: Vec<Handle>,
}

/// Decodes the `ProxyEntity` payload that follows the common object header.
pub fn decode(c: &mut BitCursor<'_>, _version: crate::version::Version) -> Result<ProxyEntity> {
    let proxy_class_id = c.read_bl()? as u32;
    let data_length = c.read_bl()? as usize;
    if data_length > MAX_PROXY_DATA_BYTES {
        return Err(Error::SectionMap(format!(
            "ProxyEntity data length {data_length} exceeds {MAX_PROXY_DATA_BYTES} sanity cap"
        )));
    }
    let mut raw_proxy_data = Vec::with_capacity(data_length);
    for _ in 0..data_length {
        raw_proxy_data.push(c.read_rc()?);
    }
    let num_handles = c.read_bl()? as usize;
    if num_handles > MAX_PROXY_HANDLES {
        return Err(Error::SectionMap(format!(
            "ProxyEntity claims {num_handles} handles (>{MAX_PROXY_HANDLES} sanity cap)"
        )));
    }
    let mut handles = Vec::with_capacity(num_handles);
    for _ in 0..num_handles {
        handles.push(c.read_handle()?);
    }
    Ok(ProxyEntity {
        proxy_class_id,
        raw_proxy_data,
        handles,
    })
}
