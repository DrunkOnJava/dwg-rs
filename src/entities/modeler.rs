//! Shared helpers for ACIS-backed modeler entities (3DSOLID, BODY,
//! REGION, and the SURFACE family) — ODA Open Design Specification
//! v5.4.1 §19.4.78 and siblings.
//!
//! Autodesk stores parametric solids and surfaces as an opaque ACIS
//! SAT (Standard ACIS Text) byte stream wrapped in a thin DWG-level
//! envelope. The envelope shape that every `AcDbModelerGeometry`
//! subclass shares is:
//!
//! ```text
//! B     acis_empty           -- 1 ⇒ no blob; everything below is skipped
//! BS    acis_version         -- 1 = legacy, 2 = newer encrypted form
//! loop:
//!   BL  chunk_len            -- 0 terminates the loop
//!   RC* chunk_bytes          -- chunk_len raw bytes
//! ```
//!
//! This crate does NOT interpret the SAT bytes — parsing ACIS is out
//! of scope, and the bytes are frequently XOR-masked with the
//! per-character pattern defined by the spec. Callers that want
//! geometry from a SAT stream should hand the raw bytes to a separate
//! ACIS parser (none ships with dwg-rs today).
//!
//! # Safety
//!
//! - Each chunk length is capped at 32 MiB individually.
//! - The accumulated blob is capped at 32 MiB total.
//! - The chunk loop iteration count is capped at `MAX_SAT_CHUNKS` to
//!   prevent a zero-progress adversarial stream from looping forever
//!   (zero-length chunks are forbidden — they either terminate the
//!   loop or signal a malformed file).
//! - Every length is cross-checked against `remaining_bits()` so we
//!   reject counts that are provably larger than the payload.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};

/// Maximum total SAT blob size that this helper will assemble.
///
/// 32 MiB is generous — the largest ACIS streams observed in real
/// drawings are a few megabytes. Cap exists to bound worst-case
/// allocation on adversarial or truncated files.
pub const MAX_SAT_BYTES: usize = 32 * 1024 * 1024;

/// Maximum number of chunks the loop will process before bailing.
///
/// Each real chunk is ≥ 1 byte; a 32 MiB cap plus a 1-byte-per-chunk
/// floor is already `32 * 1024 * 1024` iterations worst case, but in
/// practice chunks are kilobytes each, so 1M is a safe hard cap that
/// still accommodates pathological-but-legal inputs.
pub const MAX_SAT_CHUNKS: usize = 1_000_000;

/// ACIS-envelope data decoded from the stream.
///
/// `bytes` is the concatenation of every non-terminator chunk. If
/// `empty` is true, the drawing stored no SAT payload for this
/// entity (a legitimate state — some procedural surfaces retain only
/// their parametric definition, not a cached ACIS body).
#[derive(Debug, Clone, PartialEq)]
pub struct SatBlob {
    /// Was the `acis_empty` flag set? If so, no payload followed.
    pub empty: bool,
    /// ACIS version code (§19.4.78): 1 = legacy ASCII, 2 = masked.
    /// Undefined when `empty == true` — the stream skips it.
    pub version: i16,
    /// Raw concatenated SAT bytes. May be XOR-masked; this helper
    /// does not demask.
    pub bytes: Vec<u8>,
}

/// Decode one ACIS envelope per §19.4.78.
///
/// Consumes:
///
/// - 1 bit (`acis_empty`)
/// - If not empty: one BS (version) followed by a chunk loop
///
/// On success the cursor is positioned immediately after the
/// zero-length chunk terminator (or immediately after the empty flag
/// if `empty == true`).
pub fn decode_sat_blob(c: &mut BitCursor<'_>) -> Result<SatBlob> {
    let empty = c.read_b()?;
    if empty {
        return Ok(SatBlob {
            empty: true,
            version: 0,
            bytes: Vec::new(),
        });
    }

    let version = c.read_bs()?;
    let mut bytes: Vec<u8> = Vec::new();

    for chunk_ix in 0..MAX_SAT_CHUNKS {
        let len_signed = c.read_bl()?;
        if len_signed == 0 {
            return Ok(SatBlob {
                empty: false,
                version,
                bytes,
            });
        }
        if len_signed < 0 {
            return Err(Error::SectionMap(format!(
                "ACIS chunk {chunk_ix} has negative length {len_signed}"
            )));
        }
        let len = len_signed as usize;

        // Two guards:
        //   1. No single chunk may exceed the total cap.
        //   2. Post-concat size may not exceed the total cap.
        if len > MAX_SAT_BYTES {
            return Err(Error::SectionMap(format!(
                "ACIS chunk {chunk_ix} length {len} exceeds MAX_SAT_BYTES {MAX_SAT_BYTES}"
            )));
        }
        let projected = bytes.len().saturating_add(len);
        if projected > MAX_SAT_BYTES {
            return Err(Error::SectionMap(format!(
                "ACIS accumulated payload {projected} exceeds MAX_SAT_BYTES {MAX_SAT_BYTES}"
            )));
        }

        // Each byte is 8 bits; remaining-bits floor lets us reject
        // absurd counts before we try to allocate.
        let remaining_bits = c.remaining_bits();
        if len.saturating_mul(8) > remaining_bits {
            return Err(Error::SectionMap(format!(
                "ACIS chunk {chunk_ix} claims {len} bytes but only \
                 {remaining_bits} bits remain in payload"
            )));
        }

        bytes.reserve(len);
        for _ in 0..len {
            bytes.push(c.read_rc()?);
        }
    }

    Err(Error::SectionMap(format!(
        "ACIS chunk loop exceeded MAX_SAT_CHUNKS {MAX_SAT_CHUNKS}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Helper: encode a SAT blob in the same wire shape the decoder
    /// expects. Keeps the per-entity tests focused on their own
    /// fields without re-stating the chunk loop each time.
    pub(crate) fn write_sat_blob(w: &mut BitWriter, blob: &SatBlob) {
        if blob.empty {
            w.write_b(true);
            return;
        }
        w.write_b(false);
        w.write_bs(blob.version);
        // Single chunk then terminator is the common case and easiest
        // to verify.
        if !blob.bytes.is_empty() {
            w.write_bl(blob.bytes.len() as i32);
            for b in &blob.bytes {
                w.write_rc(*b);
            }
        }
        w.write_bl(0); // terminator
    }

    #[test]
    fn roundtrip_empty_blob() {
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: true,
                version: 0,
                bytes: Vec::new(),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode_sat_blob(&mut c).unwrap();
        assert!(b.empty);
        assert!(b.bytes.is_empty());
    }

    #[test]
    fn roundtrip_one_chunk_blob() {
        let payload = b"ACIS DUMMY BODY DATA".to_vec();
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: false,
                version: 2,
                bytes: payload.clone(),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode_sat_blob(&mut c).unwrap();
        assert!(!b.empty);
        assert_eq!(b.version, 2);
        assert_eq!(b.bytes, payload);
    }

    #[test]
    fn rejects_chunk_larger_than_remaining() {
        // Craft a stream that claims a huge chunk length but has no
        // actual bytes behind it.
        let mut w = BitWriter::new();
        w.write_b(false); // not empty
        w.write_bs(1); // version
        w.write_bl(i32::MAX / 2); // claimed length — absurd
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode_sat_blob(&mut c).unwrap_err();
        match err {
            Error::SectionMap(m) => assert!(
                m.contains("bits remain") || m.contains("exceeds MAX_SAT_BYTES"),
                "unexpected message: {m}"
            ),
            other => panic!("expected SectionMap, got {other:?}"),
        }
    }

    #[test]
    fn rejects_negative_chunk_length() {
        let mut w = BitWriter::new();
        w.write_b(false);
        w.write_bs(1);
        w.write_bl(-1);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode_sat_blob(&mut c).unwrap_err();
        assert!(matches!(err, Error::SectionMap(_)));
    }
}
