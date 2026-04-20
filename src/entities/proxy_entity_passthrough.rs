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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_empty_proxy_entity() {
        let mut w = BitWriter::new();
        w.write_bl(500); // class id
        w.write_bl(1); // app version
        w.write_bl(0); // 0 bits of data
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(p.proxy_class_id, 500);
        assert_eq!(p.application_version, 1);
        assert_eq!(p.data_size_bits, 0);
        assert!(p.raw_proxy_data.is_empty());
        assert!(p.original_object_handle.is_none());
    }

    #[test]
    fn roundtrip_proxy_entity_with_payload() {
        let mut w = BitWriter::new();
        w.write_bl(501);
        w.write_bl(2);
        let payload: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];
        w.write_bl((payload.len() * 8) as i32);
        for b in &payload {
            w.write_rc(*b);
        }
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(p.proxy_class_id, 501);
        assert_eq!(p.application_version, 2);
        assert_eq!(p.data_size_bits, 32);
        assert_eq!(p.raw_proxy_data, &payload);
    }

    #[test]
    fn rejects_oversized_data_size_bits() {
        let mut w = BitWriter::new();
        w.write_bl(500);
        w.write_bl(1);
        // 17 MiB × 8 bits/byte = 142 606 336 — above the cap.
        w.write_bl(142_606_336);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("PROXY")));
    }

    #[test]
    fn rejects_data_size_exceeding_remaining() {
        let mut w = BitWriter::new();
        w.write_bl(500);
        w.write_bl(1);
        // Claim 1000 bytes of payload but write nothing.
        w.write_bl(8000);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("remaining_bits")));
    }

    #[test]
    fn r2007_attempts_owner_handle_read() {
        // With padding room for a handle after the payload, R2007 should
        // opportunistically read one. If the handle is malformed we
        // fall back to None — but a well-formed one decodes.
        let mut w = BitWriter::new();
        w.write_bl(500);
        w.write_bl(1);
        w.write_bl(0); // 0 payload bits
        w.write_handle(5, 0x42);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let p = decode(&mut c, Version::R2007).unwrap();
        let h = p.original_object_handle.expect("handle should decode");
        assert_eq!(h.code, 5);
        assert_eq!(h.value, 0x42);
    }
}
