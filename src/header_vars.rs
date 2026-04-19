//! `AcDb:Header` variable table (spec §19.3).
//!
//! The drawing header is a fixed-order list of ~200 system
//! variables: global units, scales, viewport defaults, current
//! layer handle, UCS origin, etc. The order is determined by
//! version; new variables were added with each release while older
//! ones were retained for backward compatibility.
//!
//! # Philosophy
//!
//! A full decoder would map each variable to its spec-declared
//! name, type, and default value. That mapping is ~2000 lines of
//! mechanical "emit a BD/BS/BL/TV/H/BD3 in this order" code. Most
//! consumers of a DWG reader only care about a handful of header
//! vars:
//!
//! - `INSUNITS` / `MEASUREMENT` — drawing unit (inches vs millimeters)
//! - `PEXTMIN` / `PEXTMAX` / `PLIMMIN` / `PLIMMAX` — drawing extents
//! - `CLAYER` — current layer handle
//! - `LTSCALE` — global linetype scale
//! - `DIMSCALE` — global dimension scale
//! - `TEXTSIZE` — default text height
//! - `CECOLOR` — current entity color
//!
//! This module provides an opaque header holding the raw
//! decompressed bytes of the section and a growing set of
//! targeted accessors that extract specific variables without
//! parsing the whole thing. For now the accessors are limited;
//! future work can populate the rest.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::version::Version;

/// Decoded form of the `AcDb:Header` section.
///
/// Holds the post-sentinel bit-stream bytes. Individual variables
/// are extracted lazily via methods.
#[derive(Debug, Clone)]
pub struct HeaderVars {
    /// R2004+ sentinel bytes (first 16 bytes of the section).
    pub sentinel: [u8; 16],
    /// Bit-stream byte count per the 4-byte size-in-bits header.
    pub size_in_bits: u32,
    /// Raw bit-stream body (post-sentinel + size header).
    pub body: Vec<u8>,
    /// Decoded format version so accessors can branch on R2004/R2007/R2010/R2018
    /// field order.
    pub version: Version,
}

impl HeaderVars {
    /// R2004+ section sentinel (spec §19.3) — precedes every copy
    /// of the `AcDb:Header` section.
    pub const SENTINEL: [u8; 16] = [
        0xCF, 0x7B, 0x1F, 0x23, 0xFD, 0xDE, 0x38, 0xA9, 0x5F, 0x7C, 0x68, 0xB8, 0x4E, 0x6D, 0x33,
        0x5F,
    ];

    /// Parse a decompressed `AcDb:Header` payload.
    ///
    /// Bytes 0..16 are the sentinel, 16..20 are a size-in-bits
    /// little-endian u32, and the remainder is the variable
    /// bit-stream proper. This method does not consume any
    /// variables — callers use [`Self::read_first_bd`] or similar
    /// targeted getters.
    pub fn parse(bytes: &[u8], version: Version) -> Result<Self> {
        if bytes.len() < 20 {
            return Ok(Self {
                sentinel: [0; 16],
                size_in_bits: 0,
                body: Vec::new(),
                version,
            });
        }
        let mut sentinel = [0u8; 16];
        sentinel.copy_from_slice(&bytes[..16]);
        let size_in_bits =
            u32::from_le_bytes(bytes[16..20].try_into().expect("4 bytes from slice"));
        let body = bytes[20..].to_vec();
        Ok(Self {
            sentinel,
            size_in_bits,
            body,
            version,
        })
    }

    /// Whether the section's leading sentinel matches the expected
    /// value for a valid `AcDb:Header` section.
    pub fn has_valid_sentinel(&self) -> bool {
        self.sentinel == Self::SENTINEL
    }

    /// Read the first BD in the header bit-stream. For R2004+ this
    /// is the `$UNKNOWN` 64-bit double field; for R2007+ it's
    /// typically a negative placeholder that indicates R2007
    /// section-mask is required. Used mostly for format detection.
    pub fn read_first_bd(&self) -> Option<f64> {
        let mut c = BitCursor::new(&self.body);
        c.read_bd().ok()
    }

    /// Iterate the first N BD-encoded values. Convenience for
    /// diagnostic dumps before a full field mapping exists.
    pub fn read_first_n_bds(&self, n: usize) -> Vec<f64> {
        let mut c = BitCursor::new(&self.body);
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            match c.read_bd() {
                Ok(v) => out.push(v),
                Err(_) => break,
            }
        }
        out
    }

    /// Iterate the first N BL-encoded (bitlong) values.
    pub fn read_first_n_bls(&self, n: usize) -> Vec<i32> {
        let mut c = BitCursor::new(&self.body);
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            match c.read_bl() {
                Ok(v) => out.push(v),
                Err(_) => break,
            }
        }
        out
    }

    /// Length of the bit-stream body (bytes post-sentinel-and-size).
    pub fn body_len(&self) -> usize {
        self.body.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_bytes() {
        let h = HeaderVars::parse(&[], Version::R2018).unwrap();
        assert_eq!(h.body_len(), 0);
        assert!(!h.has_valid_sentinel());
    }

    #[test]
    fn parse_short_bytes_is_lenient() {
        let h = HeaderVars::parse(&[0u8; 10], Version::R2018).unwrap();
        assert_eq!(h.body_len(), 0);
    }

    #[test]
    fn parse_reads_sentinel_and_size() {
        let mut bytes = HeaderVars::SENTINEL.to_vec();
        bytes.extend_from_slice(&42u32.to_le_bytes());
        bytes.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        let h = HeaderVars::parse(&bytes, Version::R2018).unwrap();
        assert!(h.has_valid_sentinel());
        assert_eq!(h.size_in_bits, 42);
        assert_eq!(h.body_len(), 3);
    }
}
