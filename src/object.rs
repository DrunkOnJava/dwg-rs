//! Object stream walker for the `AcDb:AcDbObjects` section (spec §20).
//!
//! The section bytes obtained from `DwgFile::read_section("AcDb:AcDbObjects")`
//! hold one variable-length record per drawing object. Each record opens
//! with a **modular short (MS)** byte count — the declared number of
//! content bytes excluding the trailing 2-byte CRC — followed by a bit-
//! level payload and byte-aligned CRC.
//!
//! # Two navigation modes
//!
//! The on-disk stream is **not strictly sequential**: objects are aligned
//! and may have gaps. The authoritative way to enumerate every object in
//! a file is the `AcDb:Handles` offset table (spec §21, Phase D-3+):
//! each entry pairs a handle with a byte offset into this stream. Until
//! that parser lands, this walker operates in "first-pass" mode —
//! starting from the 4-byte `0x0dca` prefix and reading one object to
//! extract its type code and handle. That suffices for top-level
//! metadata probing (e.g. confirming the presence of `BLOCK_CONTROL`,
//! handle 0x01, the root of every DWG object graph).

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};
use crate::object_type::ObjectType;
use crate::version::Version;

/// A single undecoded object extracted from the R2004+ object stream.
#[derive(Debug, Clone)]
pub struct RawObject {
    /// Byte offset into the decompressed section where this object begins.
    pub stream_offset: usize,
    /// Size of the object in bytes (from the leading MS; excludes CRC).
    pub size_bytes: u32,
    /// Raw type code as encoded in the stream.
    pub type_code: u16,
    /// Classified form of `type_code`.
    pub kind: ObjectType,
    /// Object's handle — the ID other objects use to reference this one.
    pub handle: Handle,
    /// The entity/object's raw bytes as they appear on disk (for consumers
    /// that want to run their own entity-specific decoder).
    pub raw: Vec<u8>,
}

impl RawObject {
    /// Whether this object is an entity (drawable) or a non-entity
    /// (control, table, dictionary, ...).
    pub fn is_entity(&self) -> bool {
        self.kind.is_entity()
    }
}

/// Iterator over every object in an `AcDb:AcDbObjects` byte stream.
///
/// For R18+ files the stream is prefixed by an undocumented 4-byte RL
/// value (spec: "starts with a RL value of 0x0dca"); the walker skips
/// it automatically.
#[derive(Debug)]
pub struct ObjectWalker<'a> {
    bytes: &'a [u8],
    pos: usize,
    version: Version,
}

impl<'a> ObjectWalker<'a> {
    pub fn new(bytes: &'a [u8], version: Version) -> Self {
        // R18+ (= AC1032 and R2004/R2010/R2013 with the same layout) opens
        // with 4 bytes (0x0dca or similar) per spec §20. Skip them.
        let initial = if bytes.len() >= 4
            && matches!(version, Version::R2004 | Version::R2010 | Version::R2013 | Version::R2018)
        {
            4
        } else {
            0
        };
        Self {
            bytes,
            pos: initial,
            version,
        }
    }

    /// Read all objects into a Vec. Errors on the first malformed record
    /// that can't be stepped past; by design this should be rare because
    /// MS sizes are authoritative.
    pub fn collect_all(mut self) -> Result<Vec<RawObject>> {
        let mut out = Vec::new();
        while let Some(raw) = self.next()? {
            out.push(raw);
        }
        Ok(out)
    }
}

impl<'a> ObjectWalker<'a> {
    fn next(&mut self) -> Result<Option<RawObject>> {
        if self.pos >= self.bytes.len() {
            return Ok(None);
        }
        let start = self.pos;
        // Read MS (modular short) = object size in bytes (not counting CRC).
        // We need a byte-level reader first for MS (byte-aligned), then a
        // bit-level reader for the type + handle inside the payload.
        let (size_bytes, ms_consumed) = read_ms_bytealigned(&self.bytes[start..])?;
        if size_bytes == 0 {
            // Zero-length record — treat as stream end (observed trailing
            // padding in some R2018 files).
            self.pos = self.bytes.len();
            return Ok(None);
        }
        // Record spans MS + size_bytes payload + 2-byte CRC.
        let payload_start = start + ms_consumed;
        let payload_end = payload_start + size_bytes as usize;
        let crc_end = payload_end + 2;
        if crc_end > self.bytes.len() {
            // Malformed — truncate gracefully.
            self.pos = self.bytes.len();
            return Ok(None);
        }

        // Inside the payload we need bit-level reads to extract object
        // type and handle. For R2010+, the payload leads with an MC
        // (handle stream size in bits); we skip over it.
        let payload = &self.bytes[payload_start..payload_end];
        let mut cur = BitCursor::new(payload);

        if self.version.is_r2010_plus() {
            // MC — modular char, byte-aligned (spec §2.6). The high bit
            // on each byte flags continuation; the final byte's 0x40
            // bit IS NOT interpreted as sign here (spec §20.1 note).
            let _handle_stream_bits = read_mc_unsigned(&mut cur)?;
        }

        let type_code = read_object_type(&mut cur, self.version)?;

        // For R2000 only (not R2010+): next is an RL Obj size-in-bits.
        if matches!(self.version, Version::R2000) {
            let _obj_size_bits = cur.read_rl()?;
        }

        // Common: handle (code + counter + bytes).
        let handle = cur.read_handle()?;

        let kind = ObjectType::from_code(type_code);
        let raw = self.bytes[payload_start..payload_end].to_vec();

        self.pos = crc_end;
        Ok(Some(RawObject {
            stream_offset: start,
            size_bytes,
            type_code,
            kind,
            handle,
            raw,
        }))
    }
}

/// Read a byte-aligned modular short (MS) from a raw byte slice.
/// Returns `(value, bytes_consumed)`. An MS encodes values 0..=0x7FFF
/// per module; continuation flag is bit 15 (0x8000) of the 16-bit module.
fn read_ms_bytealigned(bytes: &[u8]) -> Result<(u32, usize)> {
    let mut value: u32 = 0;
    let mut shift: u32 = 0;
    let mut i = 0usize;
    loop {
        if i + 1 >= bytes.len() {
            return Err(Error::Truncated {
                offset: i as u64,
                wanted: 2,
                len: bytes.len() as u64,
            });
        }
        let lo = bytes[i] as u32;
        let hi = bytes[i + 1] as u32;
        i += 2;
        let module = (hi << 8) | lo;
        let cont = (module & 0x8000) != 0;
        let data = module & 0x7FFF;
        value |= data << shift;
        shift += 15;
        if !cont || shift >= 32 {
            return Ok((value, i));
        }
    }
}

/// Read an unsigned modular char (MC) — the 0x40 bit is NOT sign, per
/// spec §20.1 note on the handle stream size field.
fn read_mc_unsigned(r: &mut BitCursor<'_>) -> Result<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let b = r.read_rc()? as u64;
        let cont = (b & 0x80) != 0;
        let data = b & 0x7F;
        value |= data << shift;
        shift += 7;
        if !cont || shift >= 64 {
            return Ok(value);
        }
    }
}

/// Read the object type field.
///
/// Pre-R2010: BS (bit-short), value range 0..=0xFFFF.
///
/// R2010+ (spec §2.12): 2-bit dispatch followed by 1-2 bytes.
///   00 → next byte            (0x00..=0xFF)
///   01 → next byte + 0x1F0    (0x1F0..=0x2EF)
///   10 → next 2 bytes (raw short, LE)
///   11 → same as 10 (spec says this should never occur)
fn read_object_type(c: &mut BitCursor<'_>, version: Version) -> Result<u16> {
    if version.is_r2010_plus() {
        let tag = c.read_bb()?;
        match tag {
            0 => Ok(c.read_rc()? as u16),
            1 => Ok((c.read_rc()? as u16) + 0x1F0),
            _ => {
                let lsb = c.read_rc()? as u16;
                let msb = c.read_rc()? as u16;
                Ok((msb << 8) | lsb)
            }
        }
    } else {
        Ok(c.read_bs_u()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ms_single_module_zero() {
        // Size = 0 → module = 0x0000 → cont=0.
        let bytes = [0x00, 0x00, 0xDE, 0xAD];
        let (v, n) = read_ms_bytealigned(&bytes).unwrap();
        assert_eq!(v, 0);
        assert_eq!(n, 2);
    }

    #[test]
    fn ms_small_value() {
        // Size = 10 → module = 0x000A → LE bytes 0x0A, 0x00.
        let bytes = [0x0A, 0x00, 0xFF];
        let (v, n) = read_ms_bytealigned(&bytes).unwrap();
        assert_eq!(v, 10);
        assert_eq!(n, 2);
    }

    #[test]
    fn ms_large_value_single_module() {
        // Size = 0x7FFF → module = 0x7FFF → LE bytes 0xFF, 0x7F.
        let bytes = [0xFF, 0x7F, 0x00];
        let (v, n) = read_ms_bytealigned(&bytes).unwrap();
        assert_eq!(v, 0x7FFF);
        assert_eq!(n, 2);
    }

    #[test]
    fn ms_multi_module() {
        // Size = 0x8000: 15 data bits = 0, continuation set.
        // Module 0: data=0, cont=1 → word = 0x8000 → LE bytes 0x00, 0x80.
        // Module 1: data=1, cont=0 → word = 0x0001 → LE bytes 0x01, 0x00.
        // Result: 0 << 0 | 1 << 15 = 0x8000.
        let bytes = [0x00, 0x80, 0x01, 0x00];
        let (v, n) = read_ms_bytealigned(&bytes).unwrap();
        assert_eq!(v, 0x8000);
        assert_eq!(n, 4);
    }
}
