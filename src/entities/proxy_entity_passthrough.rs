//! PROXY ENTITY (graphical) pass-through decoder (§19.4.91).
//!
//! Third-party AutoCAD ARX applications can register custom graphical
//! entity classes. When AutoCAD saves a drawing whose author had a
//! custom-class plugin installed but the reader does not, the runtime
//! stores the opaque serialized body as a proxy entity so the data
//! survives round-tripping without loss.
//!
//! A sibling decoder already exists at [`crate::objects::proxy_entity`]
//! for the non-graphical proxy object form (§19.4.91 object variant);
//! this module covers the *entity* stream shape as it appears inline
//! in the object stream.
//!
//! # Stream shape
//!
//! ```text
//! BL    proxy_class_id        -- class_number in AcDb:Classes
//! BL    application_version   -- author-defined, opaque
//! BL    data_size_bits        -- payload bit-length (not byte-length!)
//! RC × ceil(data_size_bits / 8)    -- raw payload, byte-padded
//! (R2007+)
//!   H   original_object_data_handle  -- optional reactor/owner handle
//! ```
//!
//! # Opaque pass-through
//!
//! The raw bytes are preserved verbatim as `Vec<u8>` — no attempt is
//! made to interpret them. Consumers that need the typed form require
//! the original ARX plugin's schema, which is generally not shipped
//! with the drawing.

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};
use crate::version::Version;

/// Hard cap on the embedded proxy payload. 16 MiB matches the sibling
/// [`crate::objects::proxy_entity`] object-variant cap — real proxy
/// entities are well under 1 MiB; 16 MiB is already defensive territory.
pub const MAX_PROXY_DATA_BYTES: usize = 16 * 1024 * 1024;

/// Decoded proxy entity — class id, application version, opaque bytes,
/// and (R2007+) an optional owner handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEntityPassthrough {
    pub proxy_class_id: u32,
    pub application_version: u32,
    /// On-disk bit-length of the raw payload.
    pub data_size_bits: u32,
    /// Raw bytes as they appear on the stream (byte-padded).
    pub raw_proxy_data: Vec<u8>,
    /// Optional owner handle — R2007+ only.
    pub original_object_handle: Option<Handle>,
}

/// Decodes the `ProxyEntityPassthrough` payload that follows the common entity header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<ProxyEntityPassthrough> {
    let proxy_class_id = c.read_bl()? as u32;
    let application_version = c.read_bl()? as u32;
    let data_size_bits_signed = c.read_bl()?;
    if data_size_bits_signed < 0 {
        return Err(Error::SectionMap(format!(
            "PROXY entity negative data_size_bits {data_size_bits_signed}"
        )));
    }
    let data_size_bits = data_size_bits_signed as u32;

    // Byte count — ceil(bits / 8), capped defensively.
    let data_size_bytes = (data_size_bits as usize).div_ceil(8);
    if data_size_bytes > MAX_PROXY_DATA_BYTES {
        return Err(Error::SectionMap(format!(
            "PROXY entity data size {data_size_bytes} bytes exceeds cap \
             {MAX_PROXY_DATA_BYTES}"
        )));
    }
    // Coarse remaining-bits sanity — bytes * 8 bits needed at minimum.
    let remaining = c.remaining_bits();
    if data_size_bytes.saturating_mul(8) > remaining {
        return Err(Error::SectionMap(format!(
            "PROXY entity data_size_bits {data_size_bits} exceeds remaining_bits {remaining}"
        )));
    }

    let mut raw_proxy_data = Vec::with_capacity(data_size_bytes);
    for _ in 0..data_size_bytes {
        raw_proxy_data.push(c.read_rc()?);
    }

    // R2007+ may append an optional owner handle. We do a best-effort
    // read: if the stream has at least 8 bits left, try to read a
    // handle; otherwise skip. This is intentionally liberal — the
    // per-format handle table is what the reader uses to cross-check
    // handle integrity, not this decode site.
    let original_object_handle = if version.is_r2007_plus() && c.remaining_bits() >= 8 {
        c.read_handle().ok()
    } else {
        None
    };

    Ok(ProxyEntityPassthrough {
        proxy_class_id,
        application_version,
        data_size_bits,
        raw_proxy_data,
        original_object_handle,
    })
}
