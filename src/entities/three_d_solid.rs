//! 3DSOLID entity (§19.4.42) — ACIS-modeled 3D solid body.
//!
//! 3DSOLID, REGION (§19.4.43), and BODY (§19.4.44) share an identical
//! on-wire shape: an `acis_empty` flag that, when false, is followed by
//! a version field and a loop of length-prefixed payload chunks. The
//! concatenated chunk payloads form a Standard ACIS Text (SAT) blob —
//! a textual representation of the ACIS geometry kernel's B-rep data.
//!
//! This decoder extracts the opaque blob and the version header but
//! does NOT parse the SAT text itself. SAT is a separate file format
//! (the Spatial Corp. "Standard ACIS Text" format) with its own spec
//! and its own parser. Callers who need solid geometry from a DWG
//! should feed [`ThreeDSolid::sat_blob`] to a SAT parser downstream.
//!
//! # Stream shape
//!
//! ```text
//! B   acis_empty                  -- 1 bit: true = no body attached
//! (if !acis_empty:)
//!   BS  version                   -- ACIS format version (70 = v7.0, etc.)
//!   loop:
//!     B   has_more_blocks         -- 1 bit: true = another chunk follows
//!     (if has_more_blocks:)
//!       BL  chunk_size            -- bytes in this chunk
//!       RC  payload[chunk_size]   -- raw bytes (SAT text fragment)
//!   (loop ends when has_more_blocks is false)
//! ```
//!
//! # Shared helper
//!
//! `read_sat_blob` extracts `(version, blob)` per the shape above.
//! It is re-used by [`crate::entities::region::decode`] and
//! [`crate::entities::body::decode`] — the three ODA spec sections
//! §19.4.42/43/44 define identical encodings, so one implementation
//! backs all three entities.
//!
//! # Defensive cap
//!
//! Accumulated SAT payload is capped at [`MAX_SAT_BLOB_BYTES`] (32 MiB).
//! Real-world ACIS solids in DWG files typically fall under 1 MiB; the
//! 32 MiB ceiling is a generous safety bound chosen to accommodate
//! unusually complex assemblies while refusing adversarial input that
//! would force an unbounded allocation.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};

/// Maximum accumulated SAT blob size across all chunks (32 MiB).
///
/// Exceeding this cap returns [`Error::SectionMap`]. See module docs
/// for rationale; also referenced by [`crate::entities::region`] and
/// [`crate::entities::body`] which share the same decoder.
pub const MAX_SAT_BLOB_BYTES: usize = 32 * 1024 * 1024;

/// 3DSOLID entity — §19.4.42.
///
/// `sat_blob` is `None` when the on-wire `acis_empty` flag is set.
/// When present, `sat_blob` is the concatenation of every chunk's raw
/// payload bytes in stream order and `version` is the ACIS format
/// version reported by the writer.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreeDSolid {
    /// `true` when the entity was written with no body attached
    /// (`acis_empty` bit set).
    pub acis_empty: bool,
    /// ACIS format version reported before the chunk loop, e.g. `70`
    /// for ACIS 7.0. `None` when `acis_empty` is `true`.
    pub version: Option<u16>,
    /// Concatenated SAT payload bytes. `None` when `acis_empty` is
    /// `true`; `Some(vec![])` when the chunk loop terminated
    /// immediately (valid but unusual).
    pub sat_blob: Option<Vec<u8>>,
}

/// Decode a 3DSOLID entity per ODA spec v5.4.1 §19.4.42.
pub fn decode(c: &mut BitCursor<'_>) -> Result<ThreeDSolid> {
    let (acis_empty, version, sat_blob) = read_sat_blob(c)?;
    Ok(ThreeDSolid {
        acis_empty,
        version,
        sat_blob,
    })
}

/// Shared SAT-blob extractor used by 3DSOLID, REGION, and BODY.
///
/// Returns `(acis_empty, version, blob)`. `version` and `blob` are
/// both `None` when `acis_empty` is `true`.
///
/// # Errors
///
/// - [`Error::SectionMap`] if the accumulated blob exceeds
///   [`MAX_SAT_BLOB_BYTES`] or if a declared `chunk_size` exceeds the
///   remaining bytes in the cursor.
/// - Any [`BitCursor`] read error propagated from an underlying field.
pub(crate) fn read_sat_blob(c: &mut BitCursor<'_>) -> Result<(bool, Option<u16>, Option<Vec<u8>>)> {
    let acis_empty = c.read_b()?;
    if acis_empty {
        return Ok((true, None, None));
    }

    let version = c.read_bs_u()?;
    let mut blob: Vec<u8> = Vec::new();

    // Loop while another block follows. Each iteration reads a flag
    // first; a false flag terminates the loop before any size/payload
    // read. This matches the shape documented in ODA spec §19.4.42.
    loop {
        let has_more_blocks = c.read_b()?;
        if !has_more_blocks {
            break;
        }

        // `read_bl` returns i32; negative counts are invalid here.
        let chunk_size_signed = c.read_bl()?;
        if chunk_size_signed < 0 {
            return Err(Error::SectionMap(format!(
                "SAT chunk_size is negative ({chunk_size_signed}); \
                 entity stream is malformed"
            )));
        }
        let chunk_size = chunk_size_signed as usize;

        // A chunk_size that exceeds the remaining cursor bytes cannot
        // be real. Each RC consumes 8 bits, so divide remaining bits by
        // 8 for the byte-level ceiling.
        let remaining_bytes = c.remaining_bits() / 8;
        if chunk_size > remaining_bytes {
            return Err(Error::SectionMap(format!(
                "SAT chunk_size ({chunk_size}) exceeds remaining cursor \
                 bytes ({remaining_bytes})"
            )));
        }

        // Check the accumulated total before growing, so an over-sized
        // first chunk is rejected without a large allocation.
        if blob.len().saturating_add(chunk_size) > MAX_SAT_BLOB_BYTES {
            return Err(Error::SectionMap(format!(
                "SAT blob exceeds {MAX_SAT_BLOB_BYTES}-byte cap \
                 (accumulated {} + chunk {chunk_size})",
                blob.len()
            )));
        }

        blob.reserve(chunk_size);
        for _ in 0..chunk_size {
            blob.push(c.read_rc()?);
        }
    }

    Ok((false, Some(version), Some(blob)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_empty_body() {
        // acis_empty = true ⇒ no version, no chunks.
        let mut w = BitWriter::new();
        w.write_b(true);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert!(s.acis_empty);
        assert_eq!(s.version, None);
        assert_eq!(s.sat_blob, None);
    }

    #[test]
    fn roundtrip_single_chunk() {
        // acis_empty=false, version=70, one 4-byte chunk, then stop.
        let payload: [u8; 4] = *b"SAT!";
        let mut w = BitWriter::new();
        w.write_b(false);
        w.write_bs_u(70);
        w.write_b(true); // has_more_blocks
        w.write_bl(payload.len() as i32);
        for b in payload {
            w.write_rc(b);
        }
        w.write_b(false); // no more blocks
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert!(!s.acis_empty);
        assert_eq!(s.version, Some(70));
        assert_eq!(s.sat_blob.as_deref(), Some(&payload[..]));
    }

    #[test]
    fn roundtrip_multi_chunk() {
        // Two chunks that together form "hello, world!".
        let mut w = BitWriter::new();
        w.write_b(false);
        w.write_bs_u(700);
        w.write_b(true);
        w.write_bl(7);
        for b in b"hello, " {
            w.write_rc(*b);
        }
        w.write_b(true);
        w.write_bl(6);
        for b in b"world!" {
            w.write_rc(*b);
        }
        w.write_b(false);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let s = decode(&mut c).unwrap();
        assert_eq!(s.version, Some(700));
        assert_eq!(s.sat_blob.as_deref(), Some(&b"hello, world!"[..]));
    }

    #[test]
    fn chunk_size_over_remaining_rejected() {
        // Claim 1_000_000 bytes in a single chunk but provide almost
        // nothing. Must return SectionMap, not allocate 1M.
        let mut w = BitWriter::new();
        w.write_b(false);
        w.write_bs_u(70);
        w.write_b(true);
        w.write_bl(1_000_000);
        // No payload follows — cursor runs out.
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c).unwrap_err();
        match err {
            Error::SectionMap(msg) => {
                assert!(msg.contains("exceeds remaining cursor"), "msg: {msg}");
            }
            other => panic!("expected SectionMap, got {other:?}"),
        }
    }

    #[test]
    fn max_sat_blob_cap_constant() {
        // Compile-time sanity: 32 MiB.
        assert_eq!(MAX_SAT_BLOB_BYTES, 32 * 1024 * 1024);
    }
}
